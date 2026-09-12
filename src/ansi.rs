//! Raw ANSI stream replay through an established emulator (feature `pty`).
//!
//! Contract (see RESEARCH.md §6):
//! - **raw streams** (recorded PTY bytes, tmux `capture-pane -e` output)
//!   replay through `vt100` with explicit dimensions — cursor motion,
//!   alternate screen, and scrolling are interpreted, not discarded;
//! - **normalized dumps** ([`crate::render::ansi_dump`]) are debugging views
//!   generated FROM a frame and must never be re-parsed as state.
//!
//! The old hand-written SGR replay parser is gone on purpose.
//!
//! Raw replay preserves all cell attributes through the pinned vt100 patch.
//! Cursor appearance remains unsupported here: use the PTY path when shape
//! or blinking is part of the assertion. Position and visibility are retained.

use crate::frame::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance, Rgb};

fn convert_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(Rgb::new(r, g, b)),
    }
}

/// Replay raw terminal bytes into a canonical [`Frame`].
///
/// `scrollback` bounds retained history (0 keeps the visible screen only).
/// Fails on empty dimensions; never guesses.
pub fn replay_raw(
    bytes: &[u8],
    cols: u16,
    rows: u16,
    scrollback: usize,
    provenance: Provenance,
) -> anyhow::Result<Frame> {
    anyhow::ensure!(cols > 0 && rows > 0, "dimensions must be nonzero");
    let mut parser = vt100::Parser::new(rows, cols, scrollback);
    parser.process(bytes);
    let screen = parser.screen();
    let mut frame = Frame::blank(cols, rows, provenance);
    for r in 0..rows {
        for c in 0..cols {
            let Some(vt) = screen.cell(r, c) else {
                continue;
            };
            if vt.is_wide_continuation() {
                let mut cont = Cell::blank(c, r);
                cont.fg = convert_color(vt.fgcolor());
                cont.bg = convert_color(vt.bgcolor());
                cont.mods = Mods {
                    bold: vt.bold(),
                    dim: vt.dim(),
                    italic: vt.italic(),
                    underline: vt.underline(),
                    reverse: vt.inverse(),
                    strikethrough: vt.strikethrough(),
                    hidden: vt.hidden(),
                    blink: vt.blink(),
                };
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                continue;
            }
            let symbol = if vt.contents().is_empty() {
                " ".to_string()
            } else {
                vt.contents().to_string()
            };
            frame.set(Cell {
                x: c,
                y: r,
                symbol,
                width: if vt.is_wide() { 2 } else { 1 },
                continuation: false,
                fg: convert_color(vt.fgcolor()),
                bg: convert_color(vt.bgcolor()),
                mods: Mods {
                    bold: vt.bold(),
                    dim: vt.dim(),
                    italic: vt.italic(),
                    underline: vt.underline(),
                    strikethrough: vt.strikethrough(),
                    hidden: vt.hidden(),
                    blink: vt.blink(),
                    reverse: vt.inverse(),
                },
            });
        }
    }
    // vt100 reports (row, col). No clamping: `validate` below rejects
    // out-of-grid cursors explicitly.
    let (row, col) = screen.cursor_position();
    frame.cursor = Cursor {
        x: col,
        y: row,
        visible: !screen.hide_cursor(),
        style: CursorStyle::Block,
        blinking: false,
    };
    frame.validate().map_err(anyhow::Error::msg)?;
    Ok(frame)
}
