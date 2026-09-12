//! Grouped multi-artifact store behaviors: nested names, four-artifact
//! approvals, byte gates (ansi/txt/html), pixel gate, recursive accept and
//! report, name validation, determinism.

use ratatui::widgets::Paragraph;
use tuisnap::grouped::{validate_name, GroupedStore};
use tuisnap::snapshot::Status;
use tuisnap::{Profile, Provenance, VENDORED_FACES};

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

fn tmp_store(tag: &str) -> (tempfile::TempDir, GroupedStore) {
    let dir = tempfile::tempdir().unwrap();
    let st = GroupedStore::new(&dir.path().join(tag));
    (dir, st)
}

/// Every file below `dir`, relative paths sorted (for approved-tree audits).
fn tree_files(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out.sort();
    out
}

#[test]
fn missing_accept_match_round_trip_nested_name() {
    let name = "showcase/pages/overview_120x40_truecolor";
    let (_dir, st) = tmp_store("snapshots");
    let frame = frame_with("hello grouped");

    let o1 = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(o1.status(), Status::MissingApproval);
    assert!(o1.ensure_matched().is_err());
    assert!(o1.outcome.note.contains(".ansi"), "{}", o1.outcome.note);
    // All four actual artifacts + debug sidecars written BEFORE the gate ran,
    // in nested directories.
    assert!(o1.actual.ansi.exists());
    assert!(o1.actual.txt.exists());
    assert!(o1.actual.png.exists());
    assert!(o1.actual.html.exists());
    assert!(o1.actual.frame_json.exists());
    assert!(o1
        .actual
        .png
        .with_extension("png.fidelity.json")
        .exists());
    // Nothing approved yet (fail-closed).
    assert!(!o1.approved.ansi.exists());

    st.accept(name).unwrap();
    // The approved tree holds EXACTLY the four artifacts: no .frame.json,
    // no .fidelity.json, nothing else.
    assert_eq!(
        tree_files(st.approved_root()),
        vec![
            format!("{name}.ansi"),
            format!("{name}.html"),
            format!("{name}.png"),
            format!("{name}.txt"),
        ]
    );

    let o2 = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(o2.status(), Status::Matched);
    assert_eq!(o2.outcome.pixel_score, Some(1.0));
    assert_eq!(o2.ansi_match, Some(true));
    assert_eq!(o2.txt_match, Some(true));
    assert_eq!(o2.html_match, Some(true));
    o2.ensure_matched().unwrap();
}

#[test]
fn ansi_and_txt_and_html_are_byte_deterministic() {
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    // Same screen, different capture timestamps: provenance time must not
    // leak into any artifact (the HTML embed normalizes it).
    let mut p2 = prov();
    p2.created_unix = 1_700_000_000;
    let a = tuisnap::ratatui::widget_frame(Paragraph::new("deterministic ╔═╗"), 30, 6, prov());
    let b = tuisnap::ratatui::widget_frame(Paragraph::new("deterministic ╔═╗"), 30, 6, p2);
    assert_eq!(tuisnap::render::ansi_dump(&a), tuisnap::render::ansi_dump(&b));
    assert_eq!(a.text(), b.text());
    let ha = renderer.render_html(&a, "t").unwrap();
    let hb = renderer.render_html(&b, "t").unwrap();
    assert_eq!(ha, hb, "html must not embed the capture timestamp");
    // Re-rendered twice through render_artifacts: identical bytes.
    let r1 = renderer.render_artifacts(&a, "t").unwrap();
    let r2 = renderer.render_artifacts(&a, "t").unwrap();
    assert_eq!(r1.ansi, r2.ansi);
    assert_eq!(r1.txt, r2.txt);
    assert_eq!(r1.html, r2.html);
    assert_eq!(r1.png, r2.png);
    // The embedded frame JSON still re-imports losslessly (timestamp zeroed).
    let start = ha.find("<script type=\"application/json\">").unwrap()
        + "<script type=\"application/json\">".len();
    let end = ha[start..].find("</script>").unwrap() + start;
    let back = tuisnap::Frame::from_json(&ha[start..end]).unwrap();
    assert_eq!(back.digest(), a.digest(), "cells/cursor survive the embed");
    assert_eq!(back.provenance.created_unix, 0);
}

#[test]
fn name_validation_rejects_unsafe_names() {
    for bad in [
        "",
        "/absolute/path",
        "a/../b",
        "..",
        "../escape",
        "a//b",
        "/",
        "a/",
        "a\\b",
        "C:\\snapshots\\x",
        "a/./b",
    ] {
        assert!(validate_name(bad).is_err(), "must reject {bad:?}");
        let (_dir, st) = tmp_store("validate");
        let err = st
            .check(bad, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid snapshot name"), "{err}");
        assert!(st.accept(bad).is_err(), "accept must reject {bad:?}");
    }
    for good in ["home", "a/b/c", "showcase/pages/overview_120x40_truecolor"] {
        validate_name(good).unwrap();
    }
}

#[test]
fn cell_change_fails_ansi_gate_as_cells_differ() {
    let name = "flows/checkout/step1";
    let (_dir, st) = tmp_store("cells");
    st.check(name, &frame_with("before"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    let outcome = st
        .check(name, &frame_with("after!"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status(), Status::CellsDiffer);
    assert_eq!(outcome.ansi_match, Some(false));
    assert_eq!(outcome.txt_match, Some(false));
    assert_eq!(outcome.html_match, Some(false));
    // Pixel mismatch wrote a diff PNG under the DIFF root (not approved/).
    let diff = outcome.outcome.diff_png.clone().unwrap();
    assert!(diff.exists());
    assert!(diff.starts_with(st.diff_root()), "{}", diff.display());
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("cells-differ"), "{err}");
    assert!(err.contains("tuisnap accept"), "{err}");
}

#[test]
fn style_only_change_keeps_txt_equal() {
    let name = "flows/checkout/step2";
    let (_dir, st) = tmp_store("style");
    st.check(name, &frame_with("same text"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    let mut styled = frame_with("same text");
    styled.cells[0].mods.bold = true;
    let outcome = st
        .check(name, &styled, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status(), Status::CellsDiffer);
    assert_eq!(outcome.ansi_match, Some(false), "SGR run changed");
    assert_eq!(outcome.txt_match, Some(true), "plain text unchanged");
}

#[test]
fn png_pixel_gate_honors_threshold() {
    let name = "pages/overview";
    let (_dir, st) = tmp_store("pixels");
    let frame = frame_with("pixel gate");
    st.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    // Sabotage ONLY the approved PNG (render of a different screen): the
    // byte gates still pass, isolating the decoded-pixel gate.
    let other_png = {
        let profile = profile();
        let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
        renderer.render(&frame_with("pixel gate!")).unwrap().png
    };
    std::fs::write(st.approved_root().join(format!("{name}.png")), &other_png).unwrap();

    let strict = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(strict.status(), Status::PixelsDiffer);
    assert_eq!(strict.ansi_match, Some(true));
    assert_eq!(strict.html_match, Some(true));
    let score = strict.outcome.pixel_score.unwrap();
    assert!(score < 1.0, "score {score}");
    assert!(strict.outcome.diff_png.as_ref().unwrap().exists());

    // Same comparison passes under a threshold at/below the score.
    let relaxed = st
        .check(name, &frame, &profile(), &VENDORED_FACES, 0.0)
        .unwrap();
    assert_eq!(relaxed.status(), Status::Matched);
    assert_eq!(relaxed.outcome.pixel_score, Some(score));
}

#[test]
fn missing_single_artifact_fails_closed() {
    let name = "a/b";
    let (_dir, st) = tmp_store("partial");
    st.check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    std::fs::remove_file(st.approved_root().join(format!("{name}.txt"))).unwrap();
    let outcome = st
        .check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status(), Status::MissingApproval);
    assert!(outcome.outcome.note.contains(".txt"), "{}", outcome.outcome.note);
}

#[test]
fn corrupt_approved_png_is_an_explicit_error() {
    let name = "a/b";
    let (_dir, st) = tmp_store("corruptpng");
    st.check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    std::fs::write(st.approved_root().join(format!("{name}.png")), b"not a png").unwrap();
    let err = st
        .check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot decode expected PNG"), "{err}");
}

#[test]
fn accept_all_walks_nested_names_recursively() {
    let (_dir, st) = tmp_store("acceptall");
    let names = [
        "showcase/pages/overview_120x40_truecolor",
        "showcase/pages/detail_120x40_truecolor",
        "showcase/modals/confirm",
        "home",
    ];
    for (i, name) in names.iter().enumerate() {
        st.check(
            name,
            &frame_with(&format!("screen {i}")),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    }
    let mut listed = st.actual_names().unwrap();
    assert_eq!(listed.len(), 4, "{listed:?}");
    let accepted = st.accept_all().unwrap();
    assert_eq!(accepted, listed);
    assert_eq!(st.approved_names().unwrap(), listed);
    for name in &names {
        let outcome = st
            .check(name, &frame_with("placeholder"), &profile(), &VENDORED_FACES, 1.0)
            .unwrap();
        assert_eq!(outcome.status(), Status::CellsDiffer, "{name} was approved");
    }
    listed.sort();
    let mut sorted = names.to_vec();
    sorted.sort();
    assert_eq!(listed, sorted);
}

#[test]
fn report_reverifies_nested_actuals_outside_approved_tree() {
    let (_dir, st) = tmp_store("report");
    st.check(
        "suite/one",
        &frame_with("alpha"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    st.accept("suite/one").unwrap();
    st.check(
        "suite/nested/two",
        &frame_with("beta"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "grouped suite")
        .unwrap();
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.failed(), 1, "`suite/nested/two` has no approval");
    assert!(report.path.exists());
    // Default report path: under the actual scratch root, NEVER approved/.
    assert!(report.path.starts_with(st.actual_root()));
    assert!(!report.path.starts_with(st.approved_root()));
    let html = std::fs::read_to_string(&report.path).unwrap();
    assert!(html.contains("suite/one — matched"), "{html}");
    assert!(html.contains("suite/nested/two — missing-approval"), "{html}");
    // The approved tree is still exactly the four artifacts of `suite/one`.
    assert_eq!(
        tree_files(st.approved_root()),
        vec![
            "suite/one.ansi".to_string(),
            "suite/one.html".to_string(),
            "suite/one.png".to_string(),
            "suite/one.txt".to_string(),
        ]
    );
}

#[test]
fn report_path_is_configurable() {
    let (dir, st) = tmp_store("reportcfg");
    let custom = dir.path().join("target").join("grouped-report.html");
    let st = st.with_report_path(&custom);
    st.check("a/b", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "custom path")
        .unwrap();
    assert_eq!(report.path, custom);
    assert!(custom.exists());
}

#[test]
fn custom_actual_and_diff_roots_are_honored() {
    let dir = tempfile::tempdir().unwrap();
    let approved = dir.path().join("approved-tree");
    let st = GroupedStore::new(&approved)
        .with_actual_root(&dir.path().join("scratch/actual"))
        .with_diff_root(&dir.path().join("scratch/diff"));
    let name = "deep/nested/name";
    let outcome = st
        .check(name, &frame_with("roots"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(outcome.actual.ansi.starts_with(dir.path().join("scratch/actual")));
    assert_eq!(outcome.status(), Status::MissingApproval);
    st.accept(name).unwrap();
    assert!(approved.join(format!("{name}.ansi")).exists());
    let outcome = st
        .check(name, &frame_with("changed"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let diff = outcome.outcome.diff_png.unwrap();
    assert!(diff.starts_with(dir.path().join("scratch/diff")), "{}", diff.display());
}

#[test]
fn default_roots_are_siblings_of_approved() {
    let dir = tempfile::tempdir().unwrap();
    let st = GroupedStore::new(&dir.path().join("snapshots"));
    assert_eq!(st.actual_root(), &dir.path().join("snapshots.actual"));
    assert_eq!(st.diff_root(), &dir.path().join("snapshots.diff"));
    assert_eq!(
        st.report_path(),
        dir.path().join("snapshots.actual").join("report.html")
    );
}

#[test]
fn accept_without_actuals_is_an_error() {
    let (_dir, st) = tmp_store("noactual");
    let err = st.accept("never/checked").unwrap_err().to_string();
    assert!(err.contains("nothing to accept"), "{err}");
}

#[test]
fn check_with_reuses_one_renderer_across_nested_checks() {
    let (_dir, st) = tmp_store("checkwith");
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    let frame = frame_with("cached grouped");
    let o1 = st
        .check_with(&mut renderer, "g/s", &frame, 1.0)
        .unwrap();
    assert_eq!(o1.status(), Status::MissingApproval);
    st.accept("g/s").unwrap();
    let o2 = st
        .check_with(&mut renderer, "g/s", &frame, 1.0)
        .unwrap();
    assert_eq!(o2.status(), Status::Matched);
    assert_eq!(o2.outcome.pixel_score, Some(1.0));
    o2.ensure_matched().unwrap();
}

#[test]
fn html_artifact_is_a_standalone_colored_render() {
    let name = "docs/preview";
    let (_dir, st) = tmp_store("htmlview");
    let outcome = st
        .check(name, &frame_with("standalone"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let html = std::fs::read_to_string(&outcome.actual.html).unwrap();
    assert!(html.contains("<svg"), "{html}");
    assert!(html.contains("data:image/png;base64,"), "{html}");
    assert!(html.contains("<script type=\"application/json\">"), "{html}");
    assert!(html.contains("<title>docs/preview</title>"), "{html}");
}
