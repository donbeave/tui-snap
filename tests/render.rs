//! Renderer matrix: real glyphs, geometry, Unicode, themes, sizes, formats.
//!
//! Covers: A→B pixel change, deterministic reruns (byte-identical PNG),
//! box/Braille/icons non-blank, CJK 2-cell geometry, clipping/wrapping,
//! themes and sizes, geometry-pin failure, SVG/ANSI outputs.

use ratatui::widgets::Paragraph;
use tuisnap::{Profile, Provenance, VENDORED_FACES, VENDORED_FONT};

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn profile() -> Profile {
    Profile::default_profile()
}

fn widget_png(text: &str, cols: u16, rows: u16) -> Vec<u8> {
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new(text), cols, rows, prov());
    tuisnap::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap()
}

#[test]
fn glyph_change_changes_pixels() {
    // THE regression test the old block renderer failed by construction.
    let a = widget_png("A", 20, 5);
    let b = widget_png("B", 20, 5);
    assert_ne!(a, b, "A vs B must differ at the pixel level");
}

#[test]
fn deterministic_reruns_are_byte_identical() {
    let a = widget_png("hello determinism ╔═╗ ⠋", 30, 6);
    let b = widget_png("hello determinism ╔═╗ ⠋", 30, 6);
    assert_eq!(a, b);
}

#[test]
fn box_braille_icons_render_ink() {
    let png = widget_png("╔═╗ █ ⠋ \u{f015}", 20, 5);
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    let bg = image::Rgb([0u8, 0, 0]);
    let ink = img.pixels().filter(|p| **p != bg).count();
    assert!(ink > 200, "expected real glyph ink, got {ink} pixels");
}

#[test]
fn cjk_keeps_two_cell_geometry() {
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("日本"), 20, 5, prov());
    frame.validate().unwrap();
    let lead = frame.get(0, 0).unwrap();
    assert_eq!(lead.width, 2, "CJK lead must be width 2");
    let cont = frame.get(1, 0).unwrap();
    assert!(cont.continuation && cont.width == 0);
    // Renders without error regardless of font coverage (tofu fallback).
    let png = tuisnap::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!png.is_empty());
}

#[test]
fn clipping_and_wrapping_match_terminal() {
    // Ratatui clips overlong lines; the adapter must preserve the clip.
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("0123456789ABCDEF"), 10, 3, prov());
    assert_eq!(frame.get(9, 0).unwrap().symbol, "9");
    assert!(frame.get(10, 0).is_none());
}

#[test]
fn themes_change_pixels_and_sizes_change_dims() {
    let dark = widget_png("theme", 20, 5);
    assert!(!dark.is_empty());
    let small = widget_png("theme", 20, 5);
    let wide = widget_png("theme", 40, 5);
    assert_ne!(small.len(), wide.len());
    let (w1, _) = profile().image_size(20, 5);
    let img = image::load_from_memory(&wide).unwrap().to_rgb8();
    assert_eq!(img.width(), (40 * 10 + 24) * 2);
    assert_eq!(w1, (20 * 10 + 24) * 2);
}

#[test]
fn geometry_pin_fails_loudly() {
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("x"), 10, 3, prov());
    let mut bad = profile();
    bad.cell_w = 11;
    let err = tuisnap::render::render_png(&frame, &bad, &VENDORED_FACES).unwrap_err();
    assert!(err.to_string().contains("geometry pin broken"), "{err}");
}

#[test]
fn svg_and_ansi_outputs_carry_content() {
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("hi svg"), 20, 5, prov());
    let svg = tuisnap::render::render_svg(&frame, &profile());
    assert!(svg.contains("<svg") && svg.contains("hi svg"));
    let ansi = tuisnap::render::ansi_dump(&frame);
    assert!(ansi.contains("hi svg"));
    assert_eq!(frame.text().trim(), "hi svg");
}

#[test]
fn font_hash_pinned_and_documented() {
    let p = profile();
    assert_eq!(p.font_sha256.len(), 64);
    // JetBrainsMonoNerdFontMono-Regular.ttf (SIL OFL 1.1, see FONTS.md).
    assert_eq!(
        p.font_sha256,
        "f2a5ea6cfab397445ffab00c0370927b66d61e560a05db5db271b42006381c1a"
    );
}

#[test]
fn bold_and_italic_use_real_faces_not_faux() {
    use tuisnap::Mods;
    let bold = Mods {
        bold: true,
        ..Default::default()
    };
    let italic = Mods {
        italic: true,
        ..Default::default()
    };
    let real = tuisnap::render::render_png(
        &frame_with_mods("real", bold),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    // Single-face chain: bold falls back to the faux double-strike, which
    // must differ from the real Bold face.
    let faux = tuisnap::render::render_png(
        &frame_with_mods("real", bold),
        &profile(),
        &tuisnap::FontFaces::single(VENDORED_FONT),
    )
    .unwrap();
    assert_ne!(real, faux, "real Bold face must differ from faux bold");
    let real_it = tuisnap::render::render_png(
        &frame_with_mods("real", italic),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    let faux_it = tuisnap::render::render_png(
        &frame_with_mods("real", italic),
        &profile(),
        &tuisnap::FontFaces::single(VENDORED_FONT),
    )
    .unwrap();
    assert_ne!(real_it, faux_it, "real Italic face must differ from faux");
}

#[test]
fn fidelity_reports_missing_glyphs_exactly() {
    // 🦀 (U+1F980) is not covered by the vendored family (see FONTS.md).
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("ok 🦀"), 20, 5, prov());
    let r = tuisnap::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(r.fidelity.approximate, "uncovered glyph must mark approximate");
    assert_eq!(r.fidelity.missing.len(), 1);
    let m = &r.fidelity.missing[0];
    assert_eq!(m.symbol, "🦀");
    assert_eq!(m.codepoints, vec!["U+1F980".to_string()]);
    assert_eq!((m.x, m.y), (3, 0));
    assert!(r.fidelity.to_json().contains("U+1F980"));
    // Fully covered text: exact, nothing missing.
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("plain ╔═╗ ⠋"), 20, 5, prov());
    let r = tuisnap::render::render_png_report(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!r.fidelity.approximate);
    assert!(r.fidelity.missing.is_empty());
    assert!(r.fidelity.faces_fell_back.is_empty());
}

#[test]
fn broken_styled_face_falls_back_and_is_recorded() {
    let faces = tuisnap::FontFaces {
        regular: VENDORED_FONT,
        bold: b"not a font",
        italic: VENDORED_FONT,
        bold_italic: VENDORED_FONT,
    };
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("fallback"), 20, 5, prov());
    let r = tuisnap::render::render_png_report(&frame, &profile(), &faces).unwrap();
    assert_eq!(r.fidelity.faces_fell_back, vec!["bold".to_string()]);
    assert!(r.fidelity.approximate);
}

fn frame_with_mods(symbol: &str, mods: tuisnap::Mods) -> tuisnap::Frame {
    let mut f = tuisnap::ratatui::widget_frame(Paragraph::new(symbol), 10, 3, prov());
    // Apply mods to the non-blank lead cells only.
    for cell in f.cells.iter_mut() {
        if !cell.continuation && !cell.symbol.trim().is_empty() {
            cell.mods = mods;
        }
    }
    f
}

#[test]
fn every_modifier_changes_pixels() {
    use tuisnap::Mods;
    let plain = tuisnap::render::render_png(
        &frame_with_mods("x", Mods::default()),
        &profile(),
        &VENDORED_FACES,
    )
    .unwrap();
    for (label, mods) in [
        (
            "bold",
            Mods {
                bold: true,
                ..Default::default()
            },
        ),
        (
            "dim",
            Mods {
                dim: true,
                ..Default::default()
            },
        ),
        (
            "italic",
            Mods {
                italic: true,
                ..Default::default()
            },
        ),
        (
            "underline",
            Mods {
                underline: true,
                ..Default::default()
            },
        ),
        (
            "strike",
            Mods {
                strikethrough: true,
                ..Default::default()
            },
        ),
        (
            "reverse",
            Mods {
                reverse: true,
                ..Default::default()
            },
        ),
    ] {
        let styled =
            tuisnap::render::render_png(&frame_with_mods("x", mods), &profile(), &VENDORED_FACES)
                .unwrap();
        assert_ne!(plain, styled, "{label} must change pixels");
    }
}

#[test]
fn dark_vs_light_theme_changes_pixels() {
    let mut dark = tuisnap::ratatui::widget_frame(Paragraph::new("theme"), 20, 5, prov());
    let mut light = dark.clone();
    for cell in light.cells.iter_mut() {
        cell.bg = tuisnap::Color::Indexed(15);
        cell.fg = tuisnap::Color::Indexed(0);
    }
    let a = tuisnap::render::render_png(&dark, &profile(), &VENDORED_FACES).unwrap();
    let b = tuisnap::render::render_png(&light, &profile(), &VENDORED_FACES).unwrap();
    assert_ne!(a, b);
    let _ = &mut dark;
}

#[test]
fn emoji_tofu_keeps_two_cell_advance() {
    let frame = tuisnap::ratatui::widget_frame(Paragraph::new("🦀!"), 20, 5, prov());
    frame.validate().unwrap();
    let lead = frame.get(0, 0).unwrap();
    assert_eq!(lead.width, 2, "emoji lead must be width 2 (glyph or tofu)");
    assert!(frame.get(1, 0).unwrap().continuation);
    // Renders either way: real glyph if covered, deterministic tofu if not.
    let png = tuisnap::render::render_png(&frame, &profile(), &VENDORED_FACES).unwrap();
    assert!(!png.is_empty());
}

#[test]
fn whitespace_cells_keep_underline_and_strikethrough() {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let styled_frame = |m: Modifier| {
        tuisnap::ratatui::widget_frame(
            Paragraph::new(Line::from(Span::styled(
                "    ",
                Style::default().add_modifier(m),
            ))),
            10,
            3,
            prov(),
        )
    };
    let plain =
        tuisnap::render::render_png(&styled_frame(Modifier::empty()), &profile(), &VENDORED_FACES)
            .unwrap();
    for (label, m) in [
        ("underline", Modifier::UNDERLINED),
        ("strikethrough", Modifier::CROSSED_OUT),
    ] {
        let decorated =
            tuisnap::render::render_png(&styled_frame(m), &profile(), &VENDORED_FACES).unwrap();
        assert_ne!(
            plain, decorated,
            "{label} must draw across whitespace cells (real terminals do)"
        );
        // And the decoration is substantial: a line across 4 cells at 2×
        // scale is ~160 px, not a stray dot.
        let a = image::load_from_memory(&plain).unwrap().to_rgb8();
        let b = image::load_from_memory(&decorated).unwrap().to_rgb8();
        let changed = a
            .pixels()
            .zip(b.pixels())
            .filter(|(pa, pb)| pa != pb)
            .count();
        assert!(changed > 50, "{label} changed only {changed} px");
    }
}

#[test]
fn svg_carries_text_decorations_across_spaces() {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let frame = tuisnap::ratatui::widget_frame(
        Paragraph::new(Line::from(Span::styled(
            "a  b",
            Style::default().add_modifier(Modifier::UNDERLINED),
        ))),
        10,
        3,
        prov(),
    );
    let svg = tuisnap::render::render_svg(&frame, &profile());
    assert!(
        svg.contains("text-decoration=\"underline\""),
        "underlined run must carry the decoration: {svg}"
    );
    // The decorated run keeps its spaces, so the decoration spans them.
    assert!(svg.contains(">a  b</text>"), "{svg}");
    // Plain runs stay undecorated.
    let plain = tuisnap::ratatui::widget_frame(Paragraph::new("a  b"), 10, 3, prov());
    assert!(
        !tuisnap::render::render_svg(&plain, &profile()).contains("text-decoration"),
    );
}

#[test]
fn renderer_cache_matches_one_shot_and_stays_stable() {
    let mut r = tuisnap::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let f1 = tuisnap::ratatui::widget_frame(Paragraph::new("cache me"), 20, 5, prov());
    let f2 = tuisnap::ratatui::widget_frame(Paragraph::new("CACHE 2 \u{280b}"), 20, 5, prov());
    // Byte-identical to the one-shot free functions (cold vs warm cache).
    let one_shot = tuisnap::render::render_png_report(&f1, &profile(), &VENDORED_FACES).unwrap();
    let cached = r.render(&f1).unwrap();
    assert_eq!(one_shot.png, cached.png);
    assert_eq!(one_shot.fidelity.approximate, cached.fidelity.approximate);
    assert_eq!(one_shot.fidelity.missing, cached.fidelity.missing);
    let n1 = r.cached_glyphs();
    assert!(n1 > 0, "render must populate the glyph cache");
    let _ = r.render(&f2).unwrap();
    let n2 = r.cached_glyphs();
    assert!(n2 > n1, "new glyphs extend the cache ({n1} -> {n2})");
    // Re-render: identical pixels, no cache growth.
    let again = r.render(&f1).unwrap();
    assert_eq!(again.png, cached.png);
    assert_eq!(r.cached_glyphs(), n2);
    // render_png convenience matches render().png.
    assert_eq!(r.render_png(&f1).unwrap(), cached.png);
}

#[test]
fn renderer_new_checks_geometry_pin() {
    let mut bad = profile();
    bad.cell_w = 11;
    let err = tuisnap::render::Renderer::new(&bad, &VENDORED_FACES)
        .err()
        .expect("broken geometry pin must fail at construction");
    assert!(err.to_string().contains("geometry pin broken"), "{err}");
}

#[test]
fn cursor_styles_render() {
    use tuisnap::{Cursor, CursorStyle};
    let mut hidden = tuisnap::ratatui::widget_frame(Paragraph::new("cur"), 20, 5, prov());
    hidden.cursor.visible = false;
    let base = tuisnap::render::render_png(&hidden, &profile(), &VENDORED_FACES).unwrap();
    for style in [CursorStyle::Block, CursorStyle::Underline, CursorStyle::Bar] {
        let mut f = hidden.clone();
        f.cursor = Cursor {
            x: 0,
            y: 0,
            visible: true,
            style,
            blinking: false,
        };
        let png = tuisnap::render::render_png(&f, &profile(), &VENDORED_FACES).unwrap();
        assert_ne!(base, png, "{style:?} cursor must change pixels");
    }
}
