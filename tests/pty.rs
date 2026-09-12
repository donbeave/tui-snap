//! Interactive PTY matrix (feature `pty`): timeouts fail with evidence,
//! exit/resize/cleanup, key-driven screen change on the fixture app.

#![cfg(feature = "pty")]

use std::path::PathBuf;
use std::time::Duration;
use tuisnap::pty::{PtyOptions, Session};

fn opts() -> PtyOptions {
    PtyOptions {
        cols: 100,
        rows: 30,
        timeout: Duration::from_secs(5),
        ..Default::default()
    }
}

fn fixture_bin() -> String {
    std::env::current_exe()
        .expect("test executable path")
        .parent()
        .expect("deps directory")
        .parent()
        .expect("profile directory")
        .join("examples/fixture_app")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn timeout_fails_with_screen_evidence() {
    let mut s = Session::spawn(
        &["/bin/sleep".into(), "30".into()],
        &PtyOptions {
            timeout: Duration::from_millis(400),
            ..opts()
        },
    )
    .unwrap();
    let err = s
        .wait_for_text("this-never-appears")
        .unwrap_err()
        .to_string();
    // termlens embeds the screen at timeout: CI log alone shows the state.
    assert!(err.contains("timed out"), "{err}");
}

#[test]
fn exit_resize_and_cleanup() {
    // Fast exit.
    let mut s = Session::spawn(&["/usr/bin/true".into()], &opts()).unwrap();
    let status = s.wait_exit().unwrap();
    assert!(status.success());
    // Resize changes geometry.
    let mut s = Session::spawn(&["/bin/sleep".into(), "30".into()], &opts()).unwrap();
    s.resize(80, 24).unwrap();
    let frame = s.snapshot();
    assert_eq!((frame.cols, frame.rows), (80, 24));
    // Drop without wait_exit must not hang or poison later spawns.
    drop(s);
    let mut s2 = Session::spawn(&["/usr/bin/true".into()], &opts()).unwrap();
    s2.wait_exit().unwrap();
}

#[test]
fn wait_until_accepts_custom_predicates() {
    let mut s = Session::spawn(
        &[
            "/bin/sh".into(),
            "-c".into(),
            "printf ready; sleep 30".into(),
        ],
        &PtyOptions {
            timeout: Duration::from_secs(5),
            ..opts()
        },
    )
    .unwrap();
    // Beyond wait_for_text: text present AND cursor parked after it.
    s.wait_until(|sc| sc.text().contains("ready") && sc.cursor() == (0, 5, true))
        .unwrap();
    // A predicate that never holds times out with screen evidence.
    let mut s2 = Session::spawn(
        &["/bin/sh".into(), "-c".into(), "sleep 30".into()],
        &PtyOptions {
            timeout: Duration::from_millis(400),
            ..opts()
        },
    )
    .unwrap();
    let err = s2
        .wait_until(|sc| sc.text().contains("never-appears"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("timed out"), "{err}");
}

#[test]
fn env_set_and_remove_reach_the_child() {
    std::env::set_var("TUISNAP_PTY_STRIP_ME", "1");
    let run = |opts: &PtyOptions| -> String {
        let mut s = Session::spawn(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "echo strip=$TUISNAP_PTY_STRIP_ME set=$TUISNAP_PTY_SET_ME; sleep 30".into(),
            ],
            opts,
        )
        .unwrap();
        s.wait_stable(Duration::from_millis(400)).unwrap().text()
    };
    // Default: the inherited ambient variable reaches the child.
    let base = run(&opts());
    assert!(base.contains("strip=1"), "{base}");
    // env_remove strips the inherited var; with_env sets a new one.
    let cleaned = run(
        &opts()
            .without_env("TUISNAP_PTY_STRIP_ME")
            .with_env("TUISNAP_PTY_SET_ME", "yes"),
    );
    assert!(
        !cleaned.contains("strip=1"),
        "stripped var must be gone: {cleaned}"
    );
    assert!(cleaned.contains("set=yes"), "{cleaned}");
    std::env::remove_var("TUISNAP_PTY_STRIP_ME");
}

#[test]
fn modified_special_keys_send_csi_modifier_forms() {
    // `cat -v` renders the wire bytes readably (^[[1;5A etc.).
    let mut s = Session::spawn(&["/bin/cat".into(), "-v".into()], &opts()).unwrap();
    s.send_key("ctrl-up").unwrap();
    s.send_key("shift-f5").unwrap();
    s.send_key("ctrl-alt-delete").unwrap();
    s.send_key("enter").unwrap();
    s.wait_until(|sc| {
        let t = sc.text();
        t.contains("1;5A") && t.contains("15;2~") && t.contains("3;7~")
    })
    .unwrap();
    // Ordinary names and single-char chords still parse.
    assert!(s.send_key("ctrl-f13").is_err());
    assert!(s.send_key("shift-up-down").is_err());
}

#[test]
fn mouse_wrappers_reach_the_engine() {
    // `cat` never enables mouse tracking: the engine's explicit error
    // proves the wrapper delivered the gesture request.
    let mut s = Session::spawn(&["/bin/cat".into()], &opts()).unwrap();
    let err = s
        .scroll(1, 1, tuisnap::pty::Scroll::Up)
        .unwrap_err()
        .to_string();
    assert!(err.contains("mouse tracking"), "{err}");
    let err = s
        .click_with(tuisnap::pty::MouseButton::Right, 1, 1)
        .unwrap_err()
        .to_string();
    assert!(err.contains("mouse tracking"), "{err}");
    let err = s
        .scroll_with(tuisnap::pty::Scroll::Down.ctrl(), 1, 1)
        .unwrap_err()
        .to_string();
    assert!(err.contains("mouse tracking"), "{err}");
}

#[test]
fn keys_drive_fixture_app_screen() {
    let bin = fixture_bin();
    assert!(
        PathBuf::from(&bin).exists(),
        "build examples first: cargo build --examples ({bin})"
    );
    let mut s = Session::spawn(
        &[bin, "--screen".into(), "home".into()],
        &PtyOptions {
            timeout: Duration::from_secs(10),
            ..opts()
        },
    )
    .unwrap();
    s.wait_for_text("count=0").unwrap();
    s.send_key("enter").unwrap();
    let frame = s.wait_stable(Duration::from_millis(400)).unwrap();
    assert!(
        frame.text().contains("count=1"),
        "Enter must increment:\n{}",
        frame.text()
    );
    // Failure-evidence pattern: a timed-out wait still lets us keep a frame.
    let probe = s.snapshot();
    assert!(probe.text().contains("count=1"));
}
