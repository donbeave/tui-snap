//! Direct Ratatui buffer adapter: production view → canonical [`Frame`].
//!
//! No ANSI, no subprocesses, no PTY in unit tests. Render the real view
//! (widget or draw closure, including stateful widgets) into a `TestBackend`,
//! then convert the buffer — styles and cursor preserved.
//!
//! ```rust,no_run
//! use ratatui::{backend::TestBackend, Terminal, widgets::Paragraph};
//! use tuisnap::{Provenance, ratatui as tuisnap_ratatui};
//!
//! let backend = TestBackend::new(80, 24);
//! let mut term = Terminal::new(backend).unwrap();
//! term.draw(|f| f.render_widget(Paragraph::new("hi"), f.area())).unwrap();
//! let frame = tuisnap_ratatui::capture(
//!     &mut term,
//!     Provenance::now("default", "ratatui", vec![]),
//! );
//! assert!(frame.text().contains("hi"));
//! ```

use crate::frame::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance};
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell as RCell};
use ratatui::layout::Position;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

fn convert_color(c: ratatui::style::Color) -> Color {
    use ratatui::style::Color as C;
    match c {
        C::Reset => Color::Default,
        C::Black => Color::Indexed(0),
        C::Red => Color::Indexed(1),
        C::Green => Color::Indexed(2),
        C::Yellow => Color::Indexed(3),
        C::Blue => Color::Indexed(4),
        C::Magenta => Color::Indexed(5),
        C::Cyan => Color::Indexed(6),
        C::Gray => Color::Indexed(7),
        C::DarkGray => Color::Indexed(8),
        C::LightRed => Color::Indexed(9),
        C::LightGreen => Color::Indexed(10),
        C::LightYellow => Color::Indexed(11),
        C::LightBlue => Color::Indexed(12),
        C::LightMagenta => Color::Indexed(13),
        C::LightCyan => Color::Indexed(14),
        C::White => Color::Indexed(15),
        C::Indexed(i) => Color::Indexed(i),
        C::Rgb(r, g, b) => Color::Rgb(crate::frame::Rgb::new(r, g, b)),
    }
}

fn convert_mods(m: Modifier) -> Mods {
    Mods {
        bold: m.contains(Modifier::BOLD),
        dim: m.contains(Modifier::DIM),
        italic: m.contains(Modifier::ITALIC),
        underline: m.contains(Modifier::UNDERLINED),
        strikethrough: m.contains(Modifier::CROSSED_OUT),
        reverse: m.contains(Modifier::REVERSED),
    }
}

/// Convert one buffer cell. Wide symbols (width 2) produce the lead cell;
/// the caller emits the continuation follower.
fn lead_cell(x: u16, y: u16, rc: &RCell) -> (Cell, u8) {
    let symbol = rc.symbol().to_string();
    let width = UnicodeWidthStr::width(symbol.as_str()).min(2) as u8;
    let width = width.max(1);
    (
        Cell {
            x,
            y,
            symbol,
            width,
            continuation: false,
            fg: convert_color(rc.fg),
            bg: convert_color(rc.bg),
            mods: convert_mods(rc.modifier),
        },
        width,
    )
}

/// Convert a buffer plus explicit cursor state into a [`Frame`].
///
/// `cursor`: `(position, visible)`. Read it from
/// `TestBackend::get_cursor_position` after draw; `None` hides the cursor.
pub fn from_buffer(
    buf: &Buffer,
    cols: u16,
    rows: u16,
    cursor: Option<(Position, bool)>,
    provenance: Provenance,
) -> Frame {
    let mut frame = Frame::blank(cols, rows, provenance);
    // Straightforward row-major conversion with wide-cell continuations.
    for y in 0..rows {
        let mut x = 0u16;
        while x < cols {
            let Some(rc) = buf.cell((x, y)) else {
                x += 1;
                continue;
            };
            // Ratatui marks wide-cell followers with an empty symbol.
            if rc.symbol().is_empty() {
                let mut cont = Cell::blank(x, y);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 1;
                continue;
            }
            let (lead, w) = lead_cell(x, y, rc);
            if w == 2 && x + 1 >= cols {
                // Wide grapheme at the exact row end: no room for its
                // continuation (a terminal would wrap or clip it). Downgrade
                // to width 1 so the frame stays valid; geometry still shows
                // the full symbol in one cell.
                let mut narrow = lead;
                narrow.width = 1;
                frame.set(narrow);
                x += 1;
                continue;
            }
            frame.set(lead);
            if w == 2 && x + 1 < cols {
                let mut cont = Cell::blank(x + 1, y);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 2;
            } else {
                x += 1;
            }
        }
    }
    if let Some((pos, visible)) = cursor {
        frame.cursor = Cursor {
            x: pos.x,
            y: pos.y,
            visible,
            style: CursorStyle::Block,
            blinking: false,
        };
    }
    // Blank filler cells already carry width 1; normalize any untouched cell
    // that the buffer left as default (ratatui guarantees full coverage, but
    // stay total here).
    frame
}

/// Capture the current state of a `TestBackend` terminal: buffer + cursor.
///
/// Reads `get_cursor_position` post-draw so cursor-only changes are gated.
pub fn capture(
    term: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    provenance: Provenance,
) -> Frame {
    let backend = term.backend_mut();
    let area = backend.buffer().area;
    let cursor = backend
        .get_cursor_position()
        .ok()
        .map(|pos| (pos, backend.cursor_visible()));
    // `Buffer::clone` via re-read: TestBackend exposes `buffer()`.
    let buf = backend.buffer().clone();
    from_buffer(&buf, area.width, area.height, cursor, provenance)
}

/// Render any `Widget` into a [`Frame`] at `cols`×`rows`.
///
/// The hardware cursor is forced hidden: widget unit tests pin content, and
/// a backend-default cursor at (0,0) would make the gate depend on backend
/// defaults instead of the view. Use [`draw_frame`]/[`capture`] for
/// stateful cursor placement.
pub fn widget_frame<W>(widget: W, cols: u16, rows: u16, provenance: Provenance) -> Frame
where
    W: ratatui::widgets::Widget,
{
    let mut frame = draw_frame(cols, rows, provenance, |f| {
        f.render_widget(widget, f.area());
    });
    frame.cursor.visible = false;
    frame
}

/// Render via a draw closure (full-app frames, layouts, stateful widgets).
/// Cursor is captured post-draw, so stateful cursor placement is preserved.
pub fn draw_frame(
    cols: u16,
    rows: u16,
    provenance: Provenance,
    draw: impl FnOnce(&mut ratatui::Frame),
) -> Frame {
    let backend = ratatui::backend::TestBackend::new(cols, rows);
    let mut term = ratatui::Terminal::new(backend).expect("test terminal");
    term.draw(draw).expect("draw");
    capture(&mut term, provenance)
}
