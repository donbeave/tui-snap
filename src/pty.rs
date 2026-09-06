//! Real-PTY capture without tmux: `portable-pty` + `vt100`.
//!
//! This is the `cellshot` / `ratatui-testlib` architecture (real pty,
//! terminal emulation, wait-for-text/idle, settle before snapshot) packaged
//! with tcc's ergonomics (fixed geometry, deterministic env, key scripts).

use crate::frame::Frame;
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// How long to wait for the app to settle before reading the screen.
#[derive(Debug, Clone)]
pub struct PtyOptions {
    pub cols: u16,
    pub rows: u16,
    pub timeout: Duration,
    pub settle: Duration,
    pub env_term: String,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 40,
            timeout: Duration::from_secs(5),
            settle: Duration::from_millis(300),
            env_term: "xterm-256color".into(),
        }
    }
}

/// One input step: typed text or a named key (`enter`, `escape`, `tab`, ...).
#[derive(Debug, Clone)]
pub enum WaitFor {
    Text(String),
    Idle,
}

/// A live PTY session (one-shot in v1; named sessions = roadmap).
pub struct PtySession {
    parser: vt100::Parser,
    #[allow(dead_code)]
    pair: portable_pty::PtyPair,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    opts: PtyOptions,
}

impl PtySession {
    /// Spawn `argv[0]` with `argv[1..]` in a pty of `opts` geometry.
    pub fn spawn(argv: &[String], opts: PtyOptions) -> Result<Self> {
        anyhow::ensure!(!argv.is_empty(), "empty command");
        let pty = NativePtySystem::default()
            .openpty(PtySize {
                rows: opts.rows,
                cols: opts.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;
        let mut cmd = CommandBuilder::new(&argv[0]);
        for a in &argv[1..] {
            cmd.arg(a);
        }
        cmd.env("TERM", &opts.env_term);
        cmd.env("COLORTERM", "truecolor");
        cmd.env("LINES", opts.rows.to_string());
        cmd.env("COLUMNS", opts.cols.to_string());
        // deterministic, tcc-style: callers can override via outer env if needed
        let child = pty.slave.spawn_command(cmd).context("spawn")?;
        let writer = pty.master.take_writer().context("writer")?;
        let mut reader = pty.master.try_clone_reader().context("reader")?;
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            parser: vt100::Parser::new(opts.rows, opts.cols, 0),
            pair: pty,
            child,
            writer,
            rx,
            opts,
        })
    }

    fn pump(&mut self, budget: Duration) {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            let wait = remaining.min(Duration::from_millis(20));
            match self.rx.recv_timeout(wait) {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    // drain anything already queued without extra waiting
                    while let Ok(more) = self.rx.try_recv() {
                        self.parser.process(&more);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Send raw bytes (text + `\r` for Enter).
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Send one named key. Covers the keys tcc `capture.sh keys/mouse` uses.
    pub fn send_key(&mut self, name: &str) -> Result<()> {
        let seq: &[u8] = match name {
            "enter" => b"\r",
            "escape" | "esc" => b"\x1b",
            "tab" => b"\t",
            "backtab" => b"\x1b[Z",
            "backspace" => b"\x7f",
            "up" => b"\x1b[A",
            "down" => b"\x1b[B",
            "right" => b"\x1b[C",
            "left" => b"\x1b[D",
            "home" => b"\x1b[H",
            "end" => b"\x1b[F",
            "pageup" => b"\x1b[5~",
            "pagedown" => b"\x1b[6~",
            "space" => b" ",
            s if s.starts_with("ctrl-") && s.len() == 6 => {
                let c = s.as_bytes()[5];
                let code = if c.is_ascii_lowercase() {
                    c - b'a' + 1
                } else {
                    anyhow::bail!("bad ctrl key: {name}");
                };
                self.send(&[code])?;
                return Ok(());
            }
            s if s.starts_with("text:") => {
                let t = s["text:".len()..].to_owned();
                self.send(t.as_bytes())?;
                return Ok(());
            }
            _ => anyhow::bail!(
                "unknown key: {name} (enter|escape|tab|backtab|up|down|left|right|space|ctrl-x|text:..)"
            ),
        };
        self.send(seq)
    }

    /// Wait until screen text contains `needle` or timeout.
    pub fn wait_for_text(&mut self, needle: &str, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.pump(Duration::from_millis(50));
            if self.frame().text().contains(needle) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Settle: no new bytes for `idle` within `timeout` (cellshot `waitForIdle`).
    pub fn wait_for_idle(&mut self, idle: Duration, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last_change = Instant::now();
        let mut last_text = self.frame().text();
        while Instant::now() < deadline {
            self.pump(Duration::from_millis(50));
            let t = self.frame().text();
            if t != last_text {
                last_text = t;
                last_change = Instant::now();
            }
            if last_change.elapsed() >= idle {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Current visible frame.
    #[must_use]
    pub fn frame(&self) -> Frame {
        Frame::from_vt100(self.parser.screen())
    }

    /// Settle then snapshot.
    pub fn snapshot(&mut self) -> Frame {
        self.pump(self.opts.settle);
        self.frame()
    }

    /// Kill the child (one-shot `run` cleanup; named `stop` = roadmap).
    pub fn stop(mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

/// One-shot: spawn, optionally send keys + wait, settle, return frame.
/// `sends`: `text:<..>` / key names / `sleep:<ms>` / `wait:<needle>`.
pub fn run_once(argv: &[String], opts: &PtyOptions, sends: &[String]) -> Result<Frame> {
    let mut s = PtySession::spawn(argv, opts.clone())?;
    s.pump(Duration::from_millis(300));
    for step in sends {
        if let Some(ms) = step.strip_prefix("sleep:") {
            let ms: u64 = ms.parse().context("sleep:<ms>")?;
            std::thread::sleep(Duration::from_millis(ms));
        } else if let Some(needle) = step.strip_prefix("wait:") {
            s.wait_for_text(needle, opts.timeout)?;
        } else if let Some(text) = step.strip_prefix("type:") {
            s.send(text.as_bytes())?;
            std::thread::sleep(Duration::from_millis(120));
        } else {
            s.send_key(step)?;
            std::thread::sleep(Duration::from_millis(120));
        }
        s.pump(Duration::from_millis(100));
    }
    s.wait_for_idle(Duration::from_millis(200), opts.timeout)?;
    let f = s.snapshot();
    let _ = s.stop();
    Ok(f)
}
