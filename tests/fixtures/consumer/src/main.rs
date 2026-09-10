fn main() {
    let provenance = tuisnap::Provenance::now("consumer", "raw", vec![]);
    let frame = tuisnap::ansi::replay_raw(b"\x1b[1;2;8mX", 8, 3, 0, provenance).unwrap();
    let cell = frame.get(0, 0).unwrap();
    assert!(cell.mods.bold && cell.mods.dim && cell.mods.hidden);
    let _session_api = tuisnap::pty::Session::spawn;
    let _engine_type: Option<tuisnap::termlens::Screen> = None;
    println!("ordinary consumer: canonical engine and PTY types available without root patches");
}
