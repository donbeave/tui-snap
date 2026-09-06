//! tuisnap: capture any TUI to reviewable snapshots, and dump Ratatui views headlessly.
//!
//! Two surfaces, one artifact model:
//! - **Black-box PTY** (`pty`): spawn any binary in a real pty (no tmux needed),
//!   drive it with keys/text, wait for text/idle, read the visible [`Frame`].
//! - **In-process Ratatui** (`ratatui_shot`): render a `Widget` into a
//!   `TestBackend` and convert the buffer into the same [`Frame`].
//!
//! A [`Frame`] exports `txt / ansi / json / svg / html / png` via [`render`],
//! and is pinned by an FNV-1a [`digest`] checked into a [`Baseline`] file
//! (`BLESS=1` to regenerate, like `UPDATE_EXPECT=1` / `cargo-insta`).

pub mod ansi;
pub mod baseline;
pub mod digest;
pub mod frame;
pub mod pty;
pub mod ratatui_shot;
pub mod render;

pub use baseline::Baseline;
pub use digest::digest_frame;
pub use frame::{Cell, Color, Frame};
pub use pty::{PtyOptions, PtySession, WaitFor};
