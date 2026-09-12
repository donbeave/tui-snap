//! Visual gates: every fixture screen × theme × size, headless.
//!
//! First run writes actuals and fails missing-approval; inspect
//! `tests/visual/actual/*.png` + `report.html`, then accept explicitly:
//! `cargo run -q -- accept --store tests/visual --all`

#[path = "../examples/fixture_app.rs"]
mod fixture_app;

use fixture_app::{render_model, Model, Screen};
use std::path::PathBuf;
use tuisnap::snapshot::Store;
use tuisnap::{Profile, Provenance, VENDORED_FACES};

fn store() -> Store {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/visual");
    Store::new(&root)
}

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        profile: "tuisnap-default".into(),
        source: "fixture".into(),
        argv: vec![],
        created_unix: 0,
    }
}

#[test]
fn fixture_visual_gates() {
    let st = store();
    let profile = Profile::default_profile();
    let screens = [Screen::Home, Screen::Table, Screen::Dialog, Screen::Glyphs];
    let themes = [(true, "dark"), (false, "light")];
    let sizes = [(80u16, 24u16), (120, 40), (160, 50)];
    let mut outcomes = Vec::new();
    for screen in screens {
        for (dark, theme) in themes {
            for (cols, rows) in sizes {
                let name = format!("{screen:?}-{theme}-{cols}x{rows}").to_lowercase();
                let model = Model::new(screen, dark);
                let frame =
                    tuisnap::ratatui::draw_frame(cols, rows, prov(), |f| render_model(f, &model));
                let outcome = st
                    .check(&name, &frame, &profile, &VENDORED_FACES, 1.0)
                    .unwrap();
                outcomes.push((name, outcome));
            }
        }
    }
    // Report always written (reviewable even on failure).
    let entries: Vec<_> = outcomes
        .iter()
        .map(|(_, o)| {
            use base64::Engine;
            let b64 = &base64::engine::general_purpose::STANDARD;
            tuisnap::snapshot::ReportEntry {
                outcome: o.clone(),
                expected_png_b64: o
                    .expected_png
                    .as_ref()
                    .and_then(|p| std::fs::read(p).ok())
                    .map(|b| b64.encode(&b)),
                actual_png_b64: b64.encode(std::fs::read(&o.actual_png).unwrap()),
                diff_png_b64: o
                    .diff_png
                    .as_ref()
                    .and_then(|p| std::fs::read(p).ok())
                    .map(|b| b64.encode(&b)),
                expected_frame_json: std::fs::read_to_string(&o.expected_frame).ok(),
                actual_frame_json: {
                    let c = std::fs::read_to_string(&o.actual_frame).unwrap();
                    tuisnap::Frame::from_json(&c).unwrap().to_json_pretty()
                },
                actual_frame_compact: std::fs::read_to_string(&o.actual_frame).unwrap(),
                profile_desc: profile.name.clone(),
                font_sha256: profile.font_sha256.clone(),
            }
        })
        .collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/visual");
    tuisnap::snapshot::write_report(&Store::new(&root), "fixture visual gates", &entries).unwrap();
    let mut failures = Vec::new();
    for (name, o) in &outcomes {
        if let Err(e) = o.ensure_matched() {
            failures.push(format!("{name}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} snapshot(s) require review:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
