//! Independent ANSI and buffer sentinels; expected values are not snapshots.
use tuisnap::{Cell, Color, Frame, Profile, Provenance, Rgb, VENDORED_FACES};
fn prov() -> Provenance {
    Provenance::now("qualification", "fixture", vec![])
}

#[test]
fn hidden_svg_cells_keep_whitespace_geometry() {
    let mut frame = Frame::blank(4, 2, prov());
    for (i, symbol) in ["H", "A", "H", "B"].into_iter().enumerate() {
        frame.cells[i].symbol = symbol.into();
        frame.cells[i].mods.hidden = i % 2 == 0;
    }
    let svg = tuisnap::render::render_svg(&frame, &Profile::default_profile());
    assert!(svg.contains("xml:space=\"preserve\""));
    assert!(svg.contains("> A B</text>"));
    assert!(!svg.contains('H'));
}

#[cfg(feature = "pty")]
#[test]
fn serialized_contents_restore_wrap_before_painting() {
    for target_disabled in [false, true] {
        let mut before = termpane::DamageGrid::new(3, 8, 0);
        before.process(b"\x1b[?7l");
        let mut after = termpane::DamageGrid::new(3, 8, 0);
        after.process(b"ABCDEFGHIJ");
        if target_disabled {
            after.process(b"\x1b[?7l");
        }
        for delta in [false, true] {
            let encoded = if delta {
                after.state_diff(&before)
            } else {
                after.state_formatted()
            };
            let mut replay = termpane::DamageGrid::new(3, 8, 0);
            replay.process(&before.state_formatted());
            replay.process(&encoded);
            assert!(replay.state_eq(&after));
            assert_eq!(replay.cursor_position(), after.cursor_position());
            assert_eq!(replay.input_mode_formatted(), after.input_mode_formatted());
        }
    }
}
#[test]
fn dim_blending_uses_full_precision_before_narrowing() {
    for fg in 0..=255u8 {
        for bg in 0..=255u8 {
            let mut c = Cell::blank(0, 0);
            c.fg = Color::Rgb(Rgb::new(fg, fg, fg));
            c.bg = Color::Rgb(Rgb::new(bg, bg, bg));
            c.mods.dim = true;
            let (actual, _) = Frame::resolve_cell(&c, Rgb::new(0, 0, 0), Rgb::new(0, 0, 0));
            let expected = ((u16::from(fg) * 6 + u16::from(bg) * 4) / 10) as u8;
            assert_eq!(actual, Rgb::new(expected, expected, expected));
        }
    }
}
#[test]
fn hidden_and_blink_are_canonical_and_hidden_does_not_paint() {
    let mut hidden = Frame::blank(4, 2, prov());
    hidden.cells[0].symbol = "H".into();
    hidden.cells[0].mods.hidden = true;
    hidden.cells[0].mods.blink = true;
    let copy = Frame::from_json(&hidden.to_json()).unwrap();
    assert!(copy.cells[0].mods.hidden && copy.cells[0].mods.blink);
    let blank = Frame::blank(4, 2, prov());
    assert_ne!(blank.digest(), hidden.digest());
    let profile = Profile::default_profile();
    assert_eq!(
        tuisnap::render::render_png(&hidden, &profile, &VENDORED_FACES).unwrap(),
        tuisnap::render::render_png(&blank, &profile, &VENDORED_FACES).unwrap()
    );
    assert!(!tuisnap::render::render_svg(&hidden, &profile).contains('H'));
}
#[test]
fn ratatui_preserves_combined_flags_and_wide_styles() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color as C, Modifier, Style},
    };
    let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
    let style = Style::default()
        .fg(C::Rgb(1, 2, 3))
        .bg(C::Rgb(4, 5, 6))
        .add_modifier(Modifier::BOLD | Modifier::DIM | Modifier::HIDDEN | Modifier::SLOW_BLINK);
    b.set_string(0, 0, "界", style);
    let f = tuisnap::ratatui::from_buffer(&b, 8, 2, None, prov());
    for x in [0, 1] {
        let c = f.get(x, 0).unwrap();
        assert_eq!(c.fg, Color::Rgb(Rgb::new(1, 2, 3)));
        assert_eq!(c.bg, Color::Rgb(Rgb::new(4, 5, 6)));
        assert!(c.mods.bold && c.mods.dim && c.mods.hidden && c.mods.blink);
    }
}
#[cfg(feature = "pty")]
#[test]
fn raw_ansi_preserves_combined_flags_and_wide_styles() {
    let f = tuisnap::ansi::replay_raw(
        b"\x1b[1;2;7;8;5;9mX\x1b[0m\x1b[2;1H\x1b[38;2;1;2;3;48;2;4;5;6m\xe7\x95\x8c",
        8,
        3,
        0,
        prov(),
    )
    .unwrap();
    let m = f.get(0, 0).unwrap().mods;
    assert!(m.bold && m.dim && m.reverse && m.hidden && m.blink && m.strikethrough);
    for x in [0, 1] {
        let c = f.get(x, 1).unwrap();
        assert_eq!(c.fg, Color::Rgb(Rgb::new(1, 2, 3)));
        assert_eq!(c.bg, Color::Rgb(Rgb::new(4, 5, 6)));
    }
}
#[cfg(feature = "pty")]
#[test]
fn autowrap_off_overwrites_last_cell_then_can_be_reenabled() {
    let f = tuisnap::ansi::replay_raw(b"\x1b[?7l\x1b[1;8HABC\x1b[?7hDE\x1b[3;1H", 8, 3, 0, prov())
        .unwrap();
    assert_eq!(f.get(7, 0).unwrap().symbol, "C");
    assert_eq!(f.get(0, 1).unwrap().symbol, "D");
    assert_eq!(f.get(1, 1).unwrap().symbol, "E");
}
#[cfg(feature = "pty")]
#[test]
fn formatted_intensity_roundtrip_clears_each_independent_flag() {
    let mut grid = termpane::DamageGrid::new(2, 8, 0);
    grid.process(b"\x1b[1;2mX\x1b[22;1mB\x1b[22;2mD\x1b[0mN");
    let encoded = grid.contents_formatted();
    let mut replay = termpane::DamageGrid::new(2, 8, 0);
    replay.process(&encoded);
    for (x, bold, dim) in [
        (0, true, true),
        (1, true, false),
        (2, false, true),
        (3, false, false),
    ] {
        let c = replay.cell(0, x).unwrap();
        assert_eq!((c.bold(), c.dim()), (bold, dim));
    }
}
#[test]
fn schema_two_is_not_silently_reinterpreted() {
    let f = Frame::blank(2, 2, prov());
    assert!(Frame::from_json(&f.to_json().replace("\"version\":3", "\"version\":2")).is_err());
}

#[cfg(feature = "pty")]
#[test]
fn literal_paste_preserves_lf_and_rejects_protocol_delimiters() {
    use std::time::Duration;
    use tuisnap::pty::{PtyOptions, Session};
    let script = r#"import os,tty

tty.setraw(0)
os.write(1,b'\x1b[?2004hREADY')
data=b''
while not data.endswith(b'\x1b[201~'):
 data+=os.read(0,1024)
os.write(1,data.hex().encode())
"#;
    let mut s = Session::spawn(
        &["python3".into(), "-c".into(), script.into()],
        &PtyOptions {
            timeout: Duration::from_secs(3),
            ..Default::default()
        },
    )
    .unwrap();
    s.wait_for_text("READY").unwrap();
    assert!(s.paste_literal("bad\x1b[201~suffix").is_err());
    s.paste_literal("a\nb").unwrap();
    s.wait_for_text("1b5b3230307e610a621b5b3230317e").unwrap();
    assert!(s.wait_exit().unwrap().success());
    let mut plain =
        Session::spawn(&["/bin/sleep".into(), "2".into()], &PtyOptions::default()).unwrap();
    assert!(plain.paste_literal("x").is_err());
}

#[cfg(feature = "pty")]
#[test]
fn autowrap_off_does_not_shift_wide_glyph_left_at_margin() {
    let f = tuisnap::ansi::replay_raw("\x1b[?7l\x1b[2;8H界\x1b[3;1H".as_bytes(), 8, 3, 0, prov())
        .unwrap();
    for x in 0..8 {
        assert_eq!(f.get(x, 1).unwrap().symbol, " ");
    }
}

#[cfg(feature = "pty")]
#[test]
fn formatted_terminal_modes_preserve_autowrap_disable_and_restore() {
    let mut grid = termpane::DamageGrid::new(2, 8, 0);
    let mut original = termpane::DamageGrid::new(2, 8, 0);
    original.process(&grid.state_formatted());
    grid.process(b"\x1b[?7l");
    let mut replay = termpane::DamageGrid::new(2, 8, 0);
    replay.process(&grid.input_mode_formatted());
    replay.process(b"\x1b[1;8HABC");
    assert_eq!(replay.cell(0, 7).unwrap().contents(), "C");
    replay.process(&original.input_mode_diff(&grid));
    replay.process(b"D");
    assert_eq!(replay.cell(1, 0).unwrap().contents(), "D");
}

#[cfg(feature = "pty")]
#[test]
fn right_margin_cursor_is_physical_without_losing_pending_wrap() {
    for mode in [b"\x1b[?7l".as_slice(), b"\x1b[?7h".as_slice()] {
        let mut bytes = mode.to_vec();
        bytes.extend_from_slice(b"\x1b[1;8HA");
        let f = tuisnap::ansi::replay_raw(&bytes, 8, 3, 0, prov()).unwrap();
        assert_eq!((f.cursor.x, f.cursor.y), (7, 0));
        bytes.extend_from_slice("\u{301}".as_bytes());
        let f = tuisnap::ansi::replay_raw(&bytes, 8, 3, 0, prov()).unwrap();
        assert_eq!(f.get(7, 0).unwrap().symbol, "A\u{301}");
        bytes.push(b'B');
        let f = tuisnap::ansi::replay_raw(&bytes, 8, 3, 0, prov()).unwrap();
        if mode.ends_with(b"h") {
            assert_eq!(f.get(0, 1).unwrap().symbol, "B");
            assert_eq!((f.cursor.x, f.cursor.y), (1, 1));
        } else {
            assert_eq!(f.get(7, 0).unwrap().symbol, "B");
            assert_eq!((f.cursor.x, f.cursor.y), (7, 0));
        }
    }
}
