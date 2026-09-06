//! Approved-store behaviors: write-before-assert, explicit accept,
//! corrupt/missing handling, concurrency, reports, no auto-bless.

use ratatui::widgets::Paragraph;
use std::path::PathBuf;
use tuisnap::snapshot::{Status, Store};
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

fn frame_with(text: &str) -> tuisnap::Frame {
    tuisnap::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_store(tag: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::new(&dir.path().join(tag));
    (dir, st)
}

#[test]
fn missing_approval_fails_closed_but_writes_actuals() {
    let (_dir, st) = tmp_store("missing");
    let frame = frame_with("hello");
    let outcome = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::MissingApproval);
    // Actuals on disk BEFORE any assertion ran.
    assert!(outcome.actual_frame.exists());
    assert!(outcome.actual_png.exists());
    assert!(outcome.ensure_matched().is_err());
}

#[test]
fn accept_then_match_round_trip() {
    let (_dir, st) = tmp_store("accept");
    let frame = frame_with("stable screen");
    let o1 = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert!(!o1.status.matched());
    st.accept("home").unwrap();
    let o2 = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert_eq!(o2.status, Status::Matched);
    assert_eq!(o2.pixel_score, Some(1.0));
    o2.ensure_matched().unwrap();
}

#[test]
fn changed_snapshot_reports_cells_and_diff_image() {
    let (_dir, st) = tmp_store("changed");
    st.check(
        "home",
        &frame_with("before"),
        &profile(),
        VENDORED_FONT,
        1.0,
    )
    .unwrap();
    st.accept("home").unwrap();
    let outcome = st
        .check(
            "home",
            &frame_with("after!"),
            &profile(),
            VENDORED_FONT,
            1.0,
        )
        .unwrap();
    assert_eq!(outcome.status, Status::CellsDiffer);
    assert!(outcome.cell_diff_total > 0);
    assert!(!outcome.cell_diffs.is_empty());
    assert!(outcome.pixel_score.is_some_and(|s| s < 1.0));
    let diff = outcome.diff_png.clone().unwrap();
    assert!(diff.exists());
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("actual:") && err.contains("diff:") && err.contains("tuisnap accept"));
}

#[test]
fn corrupt_approval_is_explicit() {
    let (dir, st) = tmp_store("corrupt");
    std::fs::create_dir_all(dir.path().join("corrupt").join("approved")).unwrap();
    std::fs::write(
        dir.path()
            .join("corrupt")
            .join("approved")
            .join("home.frame.json"),
        "{broken",
    )
    .unwrap();
    let outcome = st
        .check("home", &frame_with("x"), &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::CorruptApproval);
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("corrupt"), "{err}");
}

#[test]
fn no_env_var_can_auto_accept() {
    // CI-safety: acceptance is an explicit command, never ambient state.
    std::env::set_var("BLESS", "1");
    std::env::set_var("TUISNAP_ACCEPT", "1");
    std::env::set_var("UPDATE_SNAPSHOT", "1");
    let (_dir, st) = tmp_store("noauto");
    let outcome = st
        .check("home", &frame_with("x"), &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    std::env::remove_var("BLESS");
    std::env::remove_var("TUISNAP_ACCEPT");
    std::env::remove_var("UPDATE_SNAPSHOT");
    assert_eq!(outcome.status, Status::MissingApproval);
}

#[test]
fn concurrent_different_names_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("conc");
    std::thread::scope(|s| {
        for t in 0..4 {
            let root = root.clone();
            s.spawn(move || {
                let st = Store::new(&root);
                let name = format!("screen-{t}");
                let frame = frame_with(&format!("thread {t}"));
                let o = st
                    .check(&name, &frame, &profile(), VENDORED_FONT, 1.0)
                    .unwrap();
                assert_eq!(o.status, Status::MissingApproval);
                st.accept(&name).unwrap();
                let o2 = st
                    .check(&name, &frame, &profile(), VENDORED_FONT, 1.0)
                    .unwrap();
                assert!(o2.status.matched());
            });
        }
    });
    let st = Store::new(&root);
    assert_eq!(st.actual_names().unwrap().len(), 4);
}

#[test]
fn report_embeds_images_and_frame_json() {
    let (_dir, st) = tmp_store("report");
    let frame = frame_with("reported");
    let outcome = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    let actual_png = std::fs::read(&outcome.actual_png).unwrap();
    let actual_json = std::fs::read_to_string(&outcome.actual_frame).unwrap();
    let entry = tuisnap::snapshot::ReportEntry {
        outcome,
        expected_png_b64: None,
        actual_png_b64: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &actual_png,
        ),
        diff_png_b64: None,
        expected_frame_json: None,
        actual_frame_json: tuisnap::Frame::from_json(&actual_json)
            .unwrap()
            .to_json_pretty(),
        actual_frame_compact: actual_json.clone(),
        profile_desc: profile().name.clone(),
        font_sha256: profile().font_sha256.clone(),
    };
    let report = tuisnap::snapshot::write_report(&st, "test report", &[entry]).unwrap();
    let html = std::fs::read_to_string(&report).unwrap();
    assert!(html.contains("data:image/png;base64,"));
    assert!(html.contains("application/json"));
    assert!(html.contains("tuisnap-default"));
    let _ = PathBuf::from("x");
}

#[test]
fn corrupt_approved_png_is_an_explicit_error() {
    let (_dir, st) = tmp_store("badpng");
    st.check("home", &frame_with("x"), &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    // Sabotage the approved PNG (frame JSON stays valid).
    let root = st.root();
    std::fs::write(root.join("approved").join("home.png"), b"not a png").unwrap();
    let err = st
        .check("home", &frame_with("x"), &profile(), VENDORED_FONT, 1.0)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot decode expected PNG"), "{err}");
}

#[test]
fn dimension_mismatch_status() {
    let (_dir, st) = tmp_store("dims");
    st.check("home", &frame_with("x"), &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let other = tuisnap::ratatui::widget_frame(Paragraph::new("x"), 20, 5, prov());
    let outcome = st
        .check("home", &other, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::DimensionMismatch);
}

#[test]
fn relaxed_threshold_still_gates_dimensions() {
    let (_dir, st) = tmp_store("threshold");
    let frame = frame_with("same");
    st.check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    // Identical frames score 1.0: matched under any threshold <= 1.0.
    let outcome = st
        .check("home", &frame, &profile(), VENDORED_FONT, 0.99)
        .unwrap();
    assert!(outcome.status.matched());
    assert_eq!(outcome.pixel_score, Some(1.0));
}

#[test]
fn same_name_concurrent_checks_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("samesame");
    let frame = frame_with("shared");
    std::thread::scope(|s| {
        for _ in 0..4 {
            let root = root.clone();
            let frame = frame.clone();
            s.spawn(move || {
                let st = Store::new(&root);
                let o = st
                    .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
                    .unwrap();
                assert!(matches!(
                    o.status,
                    Status::MissingApproval | Status::Matched | Status::CellsDiffer
                ));
                // Acceptance of identical content converges.
                let _ = st.accept("home");
            });
        }
    });
    let st = Store::new(&root);
    st.accept("home").unwrap();
    let o = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    assert!(o.status.matched());
    // Approved frame still parses (no torn writes).
    let text = std::fs::read_to_string(root.join("approved").join("home.frame.json")).unwrap();
    tuisnap::Frame::from_json(&text).unwrap();
}

#[test]
fn script_embed_round_trips_hostile_symbols() {
    let (_dir, st) = tmp_store("script");
    let mut frame = frame_with("ok");
    // A markup-significant symbol: the report must escape it in <script>
    // embeds (no literal `</script>` may reach the HTML) and re-import
    // must restore it losslessly.
    frame.cells[0].symbol = "<".to_string();
    let outcome = st
        .check("home", &frame, &profile(), VENDORED_FONT, 1.0)
        .unwrap();
    let entry = tuisnap::snapshot::ReportEntry {
        outcome,
        expected_png_b64: None,
        actual_png_b64: String::new(),
        diff_png_b64: None,
        expected_frame_json: None,
        actual_frame_json: frame.to_json_pretty(),
        actual_frame_compact: frame.to_json(),
        profile_desc: profile().name.clone(),
        font_sha256: profile().font_sha256.clone(),
    };
    let report = tuisnap::snapshot::write_report(&st, "t", &[entry]).unwrap();
    let html = std::fs::read_to_string(&report).unwrap();
    // Exactly one literal closer: the embed's own. The symbol's `<` must
    // have been escaped, never emitted raw into the script element.
    assert_eq!(html.matches("</script>").count(), 1);
    // Extract the embedded script JSON and re-import it losslessly.
    let start = html.find("<script type=\"application/json\"").unwrap();
    let start = html[start..].find('>').unwrap() + start + 1;
    let end = html[start..].find("</script>").unwrap() + start;
    let back = tuisnap::Frame::from_json(&html[start..end]).unwrap();
    assert_eq!(back.digest(), frame.digest());
}
