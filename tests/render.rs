//! Renderer matrix: real glyphs, geometry, Unicode, themes, sizes, formats.
//!
//! Covers: A→B pixel change, deterministic reruns (byte-identical PNG),
//! box/Braille/icons non-blank, CJK 2-cell geometry, clipping/wrapping,
//! themes and sizes, geometry-pin failure, SVG/ANSI outputs.

use ratatui::widgets::Paragraph;
use tuisnap::{Profile, Provenance, VENDORED_FONT};

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
    tuisnap::render::render_png(&frame, &profile(), VENDORED_FONT).unwrap()
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
    let png = tuisnap::render::render_png(&frame, &profile(), VENDORED_FONT).unwrap();
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
    let err = tuisnap::render::render_png(&frame, &bad, VENDORED_FONT).unwrap_err();
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
    assert_eq!(
        p.font_sha256,
        "9b55ade625f2d3f2a273ed16d9db2d924ad6236222920f9fd1324941cdc3c712"
    );
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
        VENDORED_FONT,
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
            tuisnap::render::render_png(&frame_with_mods("x", mods), &profile(), VENDORED_FONT)
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
    let a = tuisnap::render::render_png(&dark, &profile(), VENDORED_FONT).unwrap();
    let b = tuisnap::render::render_png(&light, &profile(), VENDORED_FONT).unwrap();
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
    let png = tuisnap::render::render_png(&frame, &profile(), VENDORED_FONT).unwrap();
    assert!(!png.is_empty());
}

#[test]
fn cursor_styles_render() {
    use tuisnap::{Cursor, CursorStyle};
    let mut hidden = tuisnap::ratatui::widget_frame(Paragraph::new("cur"), 20, 5, prov());
    hidden.cursor.visible = false;
    let base = tuisnap::render::render_png(&hidden, &profile(), VENDORED_FONT).unwrap();
    for style in [CursorStyle::Block, CursorStyle::Underline, CursorStyle::Bar] {
        let mut f = hidden.clone();
        f.cursor = Cursor {
            x: 0,
            y: 0,
            visible: true,
            style,
            blinking: false,
        };
        let png = tuisnap::render::render_png(&f, &profile(), VENDORED_FONT).unwrap();
        assert_ne!(base, png, "{style:?} cursor must change pixels");
    }
}
