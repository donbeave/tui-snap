//! Refactoring-guard pattern: dump Ratatui screens headlessly, pin with baseline.

use ratatui::widgets::Paragraph;
use tuisnap::{ratatui_shot::widget_frame, Baseline};

#[test]
fn widget_dump_contains_text_and_exports() {
    let frame = widget_frame(Paragraph::new("hello tuisnap"), 20, 5);
    assert!(frame.text().contains("hello tuisnap"));
    assert_eq!((frame.cols, frame.rows), (20, 5));
    // every exporter produces non-empty output
    assert!(!tuisnap::ansi::to_ansi(&frame).is_empty());
    assert!(!tuisnap::render::to_svg(&frame).is_empty());
    assert!(!tuisnap::render::to_html(&frame).is_empty());
    assert!(!tuisnap::render::to_png_bytes(&frame).unwrap().is_empty());
    // digest is stable
    let a = tuisnap::digest_frame(&frame);
    let b = tuisnap::digest_frame(&widget_frame(Paragraph::new("hello tuisnap"), 20, 5));
    assert_eq!(a, b);
}

#[test]
fn baseline_bless_and_verify_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tui.txt");
    let base = Baseline::new(path.to_str().unwrap());
    let frame = widget_frame(Paragraph::new("baseline me"), 20, 5);
    // missing baseline fails closed
    assert!(base.assert_frame("demo", &frame).is_err());
    // bless records
    std::env::set_var("BLESS", "1");
    base.assert_frame("demo", &frame).unwrap();
    std::env::remove_var("BLESS");
    // now verifies
    base.assert_frame("demo", &frame).unwrap();
    // changed content fails loudly
    let other = widget_frame(Paragraph::new("changed!!"), 20, 5);
    assert!(base.assert_frame("demo", &other).is_err());
}

#[test]
fn ansi_roundtrip_preserves_text_and_color() {
    let raw = "\x1b[38;2;72;224;84mhi\x1b[0m there\nsecond";
    let frame = tuisnap::ansi::parse_ansi(raw, 20, 5);
    assert!(frame.text().contains("hi there"));
    let fg = frame.get(0, 0).unwrap().fg.unwrap();
    assert_eq!((fg.r, fg.g, fg.b), (72, 224, 84));
    let back = tuisnap::ansi::to_ansi(&frame);
    let frame2 = tuisnap::ansi::parse_ansi(&back, 20, 5);
    assert_eq!(frame.text(), frame2.text());
}
