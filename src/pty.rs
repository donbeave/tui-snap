//! Interactive tests: the real executable in a real PTY (feature `pty`).
//!
//! The engine is [`termlens`](https://docs.rs/termlens) 0.9 (pure-cargo,
//! sync, no daemon): it owns PTY lifetime, background reads, waits, and
//! cleanup. This module is a thin fixture-oriented adapter that converts
//! termlens screens into canonical [`Frame`]s and enforces the test contract:
//!
//! - waits **fail on timeout** (the error embeds the screen at timeout —
//!   failure evidence for free);
//! - stability waits are **style-aware** (`wait_stable`, not text-only idle);
//! - resources are **bounded** (deadlines on every wait, capped scrollback);
//! - cleanup is **guaranteed**: `Session` owns the termlens `Terminal`,
//!   whose `Drop` kills and reaps the child even when a test panics.
//!
//! are not represented in [`Frame`] (frozen-frame semantics); see
//! [`crate::frame`].

use crate::frame::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance, Rgb};
use anyhow::{Context, Result};
use std::time::Duration;

/// Mouse input types for [`Session::click_with`]/[`Session::scroll_with`],
/// re-exported so callers need no direct termlens dependency for them.
pub use termlens::{MouseButton, MouseChord, Scroll, ScrollChord};

/// PTY session options.
#[derive(Debug, Clone)]
pub struct PtyOptions {
    /// Viewport columns (termlens bounds every axis to 2..=1000, at spawn
    /// and at [`Session::resize`]).
    pub cols: u16,
    /// Viewport rows (same 2..=1000 bound).
    pub rows: u16,
    /// Default deadline for every wait.
    pub timeout: Duration,
    /// Scrollback rows retained (bounded).
    pub scrollback: usize,
    /// Extra environment entries, applied after the
    /// TERM/COLORTERM/LINES/COLUMNS presets (a later entry for the same key
    /// wins).
    pub env: Vec<(String, String)>,
    /// Inherited environment variables stripped from the child — e.g.
    /// `NO_COLOR`, whose ambient presence on a developer machine or CI
    /// runner must not leak into a snapshot. Applies to the *inherited*
    /// environment only: the presets and [`Self::env`] entries are always
    /// set. Implemented as clear-then-re-add inside termlens, so semantics
    /// match `std::process::Command::env_remove` for every other variable.
    pub env_remove: Vec<String>,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 40,
            timeout: Duration::from_secs(5),
            scrollback: 1000,
            env: Vec::new(),
            env_remove: Vec::new(),
        }
    }
}

impl PtyOptions {
    /// Chainable [`env`](Self::env) entry:
    /// `PtyOptions::default().with_env("HOLLA_NO_HISTORY", "1")`.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Chainable [`env_remove`](Self::env_remove) entry:
    /// `PtyOptions::default().without_env("NO_COLOR")`.
    #[must_use]
    pub fn without_env(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }
}

fn convert_color(c: termlens::Color) -> Color {
    match c {
        termlens::Color::Default => Color::Default,
        termlens::Color::Indexed(i) => Color::Indexed(i),
        termlens::Color::Rgb(r, g, b) => Color::Rgb(Rgb::new(r, g, b)),
    }
}

/// Convert a termlens screen into a canonical [`Frame`].
pub fn frame_from_screen(screen: &termlens::Screen, provenance: Provenance) -> Frame {
    let cols = screen.cols();
    let rows = screen.rows();
    let mut frame = Frame::blank(cols, rows, provenance);
    for r in 0..rows {
        for c in 0..cols {
            let Some(tc) = screen.cell(r, c) else {
                continue;
            };
            if tc.is_wide_continuation() {
                let st = tc.style();
                let mut cont = Cell::blank(c, r);
                cont.fg = convert_color(st.fg);
                cont.bg = convert_color(st.bg);
                cont.mods = Mods {
                    bold: st.bold,
                    dim: st.dim,
                    italic: st.italic,
                    underline: st.underline,
                    strikethrough: st.strikethrough,
                    reverse: st.reverse,
                    hidden: st.conceal,
                    blink: st.blink,
                };
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                continue;
            }
            let st = tc.style();
            let symbol = if tc.contents().is_empty() {
                " ".to_string()
            } else {
                tc.contents().to_string()
            };
            frame.set(Cell {
                x: c,
                y: r,
                symbol,
                width: if tc.is_wide() { 2 } else { 1 },
                continuation: false,
                fg: convert_color(st.fg),
                bg: convert_color(st.bg),
                mods: Mods {
                    hidden: st.conceal,
                    blink: st.blink,
                    bold: st.bold,
                    dim: st.dim,
                    italic: st.italic,
                    underline: st.underline,
                    strikethrough: st.strikethrough,
                    reverse: st.reverse,
                },
            });
        }
    }
    // termlens reports (row, col, visible). Out-of-grid cursor positions are
    // NOT clamped: `validate` rejects them loudly so emulator drift surfaces
    // as an explicit error instead of a silently shifted cursor.
    let (row, col, visible) = screen.cursor();
    frame.cursor = Cursor {
        x: col,
        y: row,
        visible,
        style: match screen.cursor_shape() {
            termlens::CursorShape::Underline => CursorStyle::Underline,
            termlens::CursorShape::Bar => CursorStyle::Bar,
            _ => CursorStyle::Block,
        },
        blinking: screen.cursor_blink().unwrap_or(false),
    };
    frame
}

/// A live PTY session. Dropping it kills and reaps the child (via termlens).
pub struct Session {
    term: termlens::Terminal,
    argv: Vec<String>,
    provenance: Provenance,
}

impl Session {
    /// Spawn `argv[0]` with `argv[1..]` at `opts` geometry.
    pub fn spawn(argv: &[String], opts: &PtyOptions) -> Result<Self> {
        anyhow::ensure!(!argv.is_empty(), "empty command");
        let mut b = termlens::Terminal::builder();
        b = b.size(opts.cols, opts.rows);
        b = b.timeout(opts.timeout);
        b = b.scrollback(opts.scrollback);
        if !opts.env_remove.is_empty() {
            // termlens has no env_remove: clear the inherited environment,
            // then re-add every inherited variable minus the stripped names
            // (identical to `Command::env_remove` from the child's view).
            b = b.env_clear();
            for (k, v) in std::env::vars_os() {
                if opts.env_remove.iter().any(|r| std::ffi::OsStr::new(r) == k) {
                    continue;
                }
                b = b.env(k, v);
            }
        }
        b = b.env("TERM", "xterm-256color");
        b = b.env("COLORTERM", "truecolor");
        b = b.env("LINES", opts.rows.to_string());
        b = b.env("COLUMNS", opts.cols.to_string());
        for (k, v) in &opts.env {
            b = b.env(k, v);
        }
        if argv.len() > 1 {
            b = b.args(&argv[1..]);
        }
        let term = b.spawn(&argv[0]).context("spawn PTY")?;
        Ok(Self {
            term,
            argv: argv.to_vec(),
            provenance: Provenance::now("tuisnap-default", "pty", argv.to_vec()),
        })
    }

    /// Current visible frame (no waiting).
    pub fn snapshot(&self) -> Frame {
        frame_from_screen(&self.term.screen(), self.provenance.clone())
    }

    /// Send one named key: `enter escape tab backtab backspace insert delete
    /// up down left right home end pageup pagedown space f1..f12 ctrl-x alt-x`,
    /// modifier chords over special keys (`ctrl-up`, `alt-home`,
    /// `ctrl-shift-f5` — any subset of `ctrl`/`alt`/`shift` over the arrows,
    /// home/end, insert/delete, pageup/pagedown, f1..f12, sent in the xterm
    /// CSI-modifier form), single printable characters, or `text:<literal>`.
    pub fn send_key(&mut self, name: &str) -> Result<()> {
        use termlens::Key;
        if let Some(text) = name.strip_prefix("text:") {
            return Ok(self.term.send_str(text)?);
        }
        if let Some(chord) = parse_chord(name) {
            return Ok(self.term.send(chord)?);
        }
        let key = match name {
            "enter" => Key::Enter,
            "escape" | "esc" => Key::Esc,
            "tab" => Key::Tab,
            "backtab" => Key::BackTab,
            "backspace" => Key::Backspace,
            "insert" => Key::Insert,
            "delete" => Key::Delete,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "space" => Key::Char(' '),
            // Single printable characters first: "f" is a letter, "f5" is F5.
            s if s.chars().count() == 1 => Key::Char(s.chars().next().unwrap_or(' ')),
            s if s.starts_with('f') && (2..=3).contains(&s.len()) => {
                let n: u8 = s[1..]
                    .parse()
                    .with_context(|| format!("bad function key: {name}"))?;
                anyhow::ensure!((1..=12).contains(&n), "bad function key: {name}");
                Key::F(n)
            }
            s if s.starts_with("ctrl-") && s.len() == 6 => Key::Ctrl(s.as_bytes()[5] as char),
            s if s.starts_with("alt-") && s.len() == 5 => Key::Alt(s.as_bytes()[4] as char),
            _ => anyhow::bail!("unknown key: {name}"),
        };
        Ok(self.term.send(key)?)
    }

    /// Type literal text (no key interpretation).
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        Ok(self.term.send_str(text)?)
    }

    /// Bracketed paste.
    pub fn paste(&mut self, text: &str) -> Result<()> {
        Ok(self.term.paste(text)?)
    }

    /// Send a bracketed paste preserving literal newlines.
    ///
    /// Unlike [`Self::paste`], this does not emulate terminal newline conversion.
    /// Fails unless bracketed paste is enabled, or if the payload contains paste
    /// delimiters: such text could escape the bracket and become ordinary input.
    pub fn paste_literal(&mut self, text: &str) -> Result<()> {
        anyhow::ensure!(
            self.term.screen().bracketed_paste(),
            "bracketed paste is not enabled"
        );
        anyhow::ensure!(
            !text.contains("\x1b[200~") && !text.contains("\x1b[201~"),
            "payload contains paste delimiter"
        );
        Ok(self.term.send_str(&format!("\x1b[200~{text}\x1b[201~"))?)
    }

    /// Left-click at `(col, row)`.
    pub fn click(&mut self, col: u16, row: u16) -> Result<()> {
        Ok(self.term.click(col, row)?)
    }

    /// Click with any button or chord — `MouseButton::Right`,
    /// `MouseButton::Left.ctrl()`, … (re-exported from termlens).
    pub fn click_with(
        &mut self,
        button: impl Into<termlens::MouseChord>,
        col: u16,
        row: u16,
    ) -> Result<()> {
        Ok(self.term.click_with(button, col, row)?)
    }

    /// Mouse-wheel scroll at `(col, row)`.
    pub fn scroll(&mut self, col: u16, row: u16, direction: termlens::Scroll) -> Result<()> {
        Ok(self.term.scroll(col, row, direction)?)
    }

    /// Scroll with modifiers — `Scroll::Up.ctrl()`, `Scroll::Down.shift()`.
    pub fn scroll_with(
        &mut self,
        direction: impl Into<termlens::ScrollChord>,
        col: u16,
        row: u16,
    ) -> Result<()> {
        Ok(self.term.scroll_with(direction, col, row)?)
    }

    /// Drag with the primary button from `(c0, r0)` to `(c1, r1)`.
    pub fn drag(&mut self, c0: u16, r0: u16, c1: u16, r1: u16) -> Result<()> {
        Ok(self
            .term
            .drag(termlens::MouseButton::Left, (c0, r0), (c1, r1))?)
    }

    /// Resize the viewport (bounded by termlens to 2..=1000 per axis).
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        Ok(self.term.resize(cols, rows)?)
    }

    /// Fail unless the screen contains `needle` before the deadline.
    /// Timeout errors embed the screen at timeout (failure evidence).
    pub fn wait_for_text(&mut self, needle: &str) -> Result<()> {
        Ok(self.term.wait_until(|s| s.text().contains(needle))?)
    }

    /// Fail unless `pred` holds on the live screen before the deadline —
    /// the passthrough for custom predicates (text disappearance, cursor
    /// position, per-cell styles, …) that `wait_for_text` cannot express.
    /// Same timeout contract: the error embeds the screen at timeout.
    pub fn wait_until(&mut self, pred: impl FnMut(&termlens::Screen) -> bool) -> Result<()> {
        Ok(self.term.wait_until(pred)?)
    }

    /// Fail unless the full screen (content AND styles) is stable for `quiet`.
    /// Prefer over text-only idle: color/cursor-only activity resets it.
    pub fn wait_stable(&mut self, quiet: Duration) -> Result<Frame> {
        let screen = self.term.wait_stable(quiet)?;
        Ok(frame_from_screen(&screen, self.provenance.clone()))
    }

    /// Fail unless output is quiet for `quiet` (fallback when no predicate).
    pub fn wait_idle(&mut self, quiet: Duration) -> Result<()> {
        Ok(self.term.wait_idle(quiet)?)
    }

    /// Fail unless the child exits; returns its status.
    pub fn wait_exit(&mut self) -> Result<termlens::ExitStatus> {
        Ok(self.term.wait_exit()?)
    }

    /// The spawned command line (for provenance/diagnostics).
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// Parse `ctrl-up`, `alt-home`, `ctrl-shift-f5`: any subset of
/// `ctrl`/`alt`/`shift` over a chord-capable special key (arrows, home/end,
/// insert/delete, pageup/pagedown, f1..f12), encoded in the xterm
/// CSI-modifier form. Returns `None` for everything else — ordinary names
/// and single-character `ctrl-x`/`alt-x` chords keep their existing
/// handling in [`Session::send_key`].
fn parse_chord(name: &str) -> Option<termlens::Chord> {
    use termlens::Key;
    if !name.contains('-') {
        return None;
    }
    let mut parts = name.split('-');
    let last = parts.next_back()?;
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut any = false;
    for part in parts {
        any = true;
        match part {
            "ctrl" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    if !any {
        return None;
    }
    let key = match last {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        s if s.starts_with('f') && (2..=3).contains(&s.len()) => {
            let n: u8 = s[1..].parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            Key::F(n)
        }
        _ => return None,
    };
    // `Key::{ctrl,alt,shift}` each construct a chord carrying that modifier;
    // start from any present one, then set the rest (idempotent setters).
    let mut chord = if ctrl {
        key.ctrl()
    } else if alt {
        key.alt()
    } else {
        key.shift()
    };
    if ctrl {
        chord = chord.ctrl();
    }
    if alt {
        chord = chord.alt();
    }
    if shift {
        chord = chord.shift();
    }
    Some(chord)
}

/// One-shot scripted capture: spawn, run `sends` steps, settle, snapshot.
///
/// Steps: `type:<text>`, `sleep:<ms>`, `wait:<needle>`, anything else is a
/// [`Session::send_key`] name. Every wait fails the run on timeout — a
/// scenario never continues past a screen that never appeared.
///
/// Boot: if `sends` starts with `wait:<needle>`, that needle is readiness
/// and the 200 ms quiet window is skipped. Live clocks and reduced-motion
/// ticks keep emitting bytes after the prompt is on screen, so
/// [`Session::wait_idle`] never holds. Otherwise wait 200 ms of silence
/// (apps whose boot is a burst then idle).
pub fn run_once(
    argv: &[String],
    opts: &PtyOptions,
    sends: &[String],
    settle: Duration,
) -> Result<Frame> {
    let mut s = Session::spawn(argv, opts)?;
    let rest = if let Some(needle) = sends.first().and_then(|st| st.strip_prefix("wait:")) {
        s.wait_for_text(needle)?;
        &sends[1..]
    } else {
        s.wait_idle(Duration::from_millis(200))?;
        sends
    };
    apply_sends(&mut s, rest)?;
    s.wait_stable(settle)?;
    Ok(s.snapshot())
}

fn apply_sends(s: &mut Session, sends: &[String]) -> Result<()> {
    for step in sends {
        if let Some(ms) = step.strip_prefix("sleep:") {
            let ms: u64 = ms.parse().context("sleep:<ms>")?;
            std::thread::sleep(Duration::from_millis(ms));
        } else if let Some(needle) = step.strip_prefix("wait:") {
            s.wait_for_text(needle)?;
        } else if let Some(text) = step.strip_prefix("type:") {
            s.type_text(text)?;
            std::thread::sleep(Duration::from_millis(120));
        } else {
            s.send_key(step)?;
            std::thread::sleep(Duration::from_millis(120));
        }
    }
    Ok(())
}
