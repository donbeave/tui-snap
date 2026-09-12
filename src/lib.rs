//! tuisnap: Rust TUI visual-regression toolkit.
//!
//! Two capture paths share one canonical [`Frame`]:
//! - **Pure view tests** ([`ratatui`]): fixture model + view state +
//!   viewport + theme → the actual production Ratatui view → frame. No
//!   business logic, network, database, or PTY.
//! - **Interactive tests** ([`pty`], feature `pty`): the real executable in a
//!   real PTY (termlens engine), keyboard/mouse/resize, readiness waits that
//!   fail on timeout → frame.
//!
//! Both produce full approved frames + readable PNGs and portable HTML
//! expected/actual/diff reports ([`snapshot`]). A changed snapshot requires
//! explicit review ([`snapshot::Store::accept`]); CI never auto-blesses.
//! Equality only validates the fixtures covered — not every app state.

pub mod diff;
pub mod frame;
pub mod profile;
pub mod ratatui;
pub mod render;
pub mod snapshot;

#[cfg(feature = "pty")]
pub mod ansi;
#[cfg(feature = "pty")]
pub mod pty;

/// The pinned PTY engine, re-exported for callers constructing screens for
/// [`pty::frame_from_screen`]. Git/path consumers need no Cargo patches.
#[cfg(feature = "pty")]
pub use termlens;

pub use frame::{Cell, Color, Cursor, CursorStyle, Frame, FrameError, Mods, Provenance, Rgb};
pub use profile::{
    FontFaces, Profile, VENDORED_FACES, VENDORED_FONT, VENDORED_FONT_BOLD,
    VENDORED_FONT_BOLD_ITALIC, VENDORED_FONT_ITALIC,
};
