//! In-process Ratatui dumps: the refactoring safety net.
//!
//! Pattern (from `ratatui` docs + tcc `Harness`):
//! ```rust,no_run
//! use ratatui::{backend::TestBackend, Terminal, widgets::Paragraph};
//! use tuisnap::ratatui_shot::widget_frame;
//! let frame = widget_frame(Paragraph::new("hello"), 80, 24);
//! assert!(frame.text().contains("hello"));
//! ```
//!
//! For full apps, drive your `App` state machine headlessly (key events as
//! method calls), then `widget_frame(app_widget, w, h)` per screen and
//! `Baseline::assert_frame` each one. No pty, no flake, deterministic.

use crate::frame::{Cell, Color, Frame};
use ratatui::{backend::TestBackend, Terminal};

fn rat_color(c: ratatui::style::Color) -> Option<Color> {
    use ratatui::style::Color as C;
    match c {
        C::Reset => None,
        C::Black => Some(Color::from_256(0)),
        C::Red => Some(Color::from_256(1)),
        C::Green => Some(Color::from_256(2)),
        C::Yellow => Some(Color::from_256(3)),
        C::Blue => Some(Color::from_256(4)),
        C::Magenta => Some(Color::from_256(5)),
        C::Cyan => Some(Color::from_256(6)),
        C::Gray => Some(Color::from_256(7)),
        C::DarkGray => Some(Color::from_256(8)),
        C::LightRed => Some(Color::from_256(9)),
        C::LightGreen => Some(Color::from_256(10)),
        C::LightYellow => Some(Color::from_256(11)),
        C::LightBlue => Some(Color::from_256(12)),
        C::LightMagenta => Some(Color::from_256(13)),
        C::LightCyan => Some(Color::from_256(14)),
        C::White => Some(Color::from_256(15)),
        C::Indexed(i) => Some(Color::from_256(i)),
        C::Rgb(r, g, b) => Some(Color::new(r, g, b)),
    }
}

/// Render any `Widget` into a [`Frame`] at `cols`x`rows`.
pub fn widget_frame<W: ratatui::widgets::Widget>(w: W, cols: u16, rows: u16) -> Frame {
    let backend = TestBackend::new(cols, rows);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| f.render_widget(w, f.area())).expect("draw");
    buffer_frame(term.backend().buffer(), cols, rows)
}

/// Render via a draw closure (full-app frames, layouts, multi-widget screens).
pub fn draw_frame(cols: u16, rows: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> Frame {
    let backend = TestBackend::new(cols, rows);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(draw).expect("draw");
    buffer_frame(term.backend().buffer(), cols, rows)
}

fn buffer_frame(buf: &ratatui::buffer::Buffer, cols: u16, rows: u16) -> Frame {
    use ratatui::style::Modifier;
    let mut f = Frame::blank(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            if let Some(cell) = buf.cell((x, y)) {
                f.set(
                    x,
                    y,
                    Cell {
                        symbol: cell.symbol().to_string(),
                        fg: rat_color(cell.fg),
                        bg: rat_color(cell.bg),
                        bold: cell.modifier.contains(Modifier::BOLD),
                        dim: cell.modifier.contains(Modifier::DIM),
                        italic: cell.modifier.contains(Modifier::ITALIC),
                        underline: cell.modifier.contains(Modifier::UNDERLINED),
                        reverse: cell.modifier.contains(Modifier::REVERSED),
                    },
                );
            }
        }
    }
    f
}
