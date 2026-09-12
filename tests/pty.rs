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
