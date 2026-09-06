//! Cell + Frame model: the single artifact every exporter reads.
//!
//! Borrowed from `cellshot` (versioned `Frame` JSON) and `vt100` (screen
//! semantics), kept dependency-free so both the PTY path and the Ratatui
//! `TestBackend` path converge here.

use serde::{Deserialize, Serialize};

/// RGB color. `None` on a cell means "terminal default".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Map xterm-256 index to RGB (same table as tcc `tools/ansi2html.py`).
    #[must_use]
    pub fn from_256(n: u8) -> Self {
        const BASIC: [[u8; 3]; 16] = [
            [0, 0, 0],
            [205, 49, 49],
            [13, 188, 121],
            [229, 229, 16],
            [36, 114, 200],
            [188, 63, 188],
            [17, 168, 205],
            [229, 229, 229],
            [102, 102, 102],
            [241, 76, 76],
            [35, 209, 139],
            [245, 245, 67],
            [59, 142, 234],
            [214, 112, 214],
            [41, 184, 219],
            [255, 255, 255],
        ];
        if n < 16 {
            let c = BASIC[n as usize];
            return Self::new(c[0], c[1], c[2]);
        }
        if n < 232 {
            let n = n - 16;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            return Self::new(conv(n / 36), conv((n / 6) % 6), conv(n % 6));
        }
        let v = 8 + (n - 232) * 10;
        Self::new(v, v, v)
    }
}

/// One terminal cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub symbol: String,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: " ".to_string(),
            fg: None,
            bg: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            reverse: false,
        }
    }
}

/// Visible screen: `rows` of `cols` cells + cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub version: u8,
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Option<(u16, u16)>,
    pub cursor_visible: bool,
}

impl Frame {
    #[must_use]
    pub fn blank(cols: u16, rows: u16) -> Self {
        Self {
            version: 1,
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
            cursor: None,
            cursor_visible: false,
        }
    }

    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.cols && y < self.rows {
            Some(y as usize * self.cols as usize + x as usize)
        } else {
            None
        }
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = cell;
        }
    }

    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.idx(x, y).map(|i| &self.cells[i])
    }

    /// Plain text, rows joined with `\n`, trailing spaces trimmed per row
    /// (matches `tmux capture-pane -p` semantics used by tcc `shots/*.txt`).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for y in 0..self.rows {
            if y > 0 {
                out.push('\n');
            }
            let mut row = String::new();
            for x in 0..self.cols {
                if let Some(c) = self.get(x, y) {
                    row.push_str(&c.symbol);
                }
            }
            out.push_str(row.trim_end());
        }
        out
    }

    /// Build from a `vt100` screen (the PTY path: cellshot/ratatui-testlib approach).
    #[must_use]
    pub fn from_vt100(screen: &vt100::Screen) -> Self {
        fn conv(c: vt100::Color) -> Option<Color> {
            match c {
                vt100::Color::Default => None,
                vt100::Color::Idx(i) => Some(Color::from_256(i)),
                vt100::Color::Rgb(r, g, b) => Some(Color::new(r, g, b)),
            }
        }
        let (rows, cols) = screen.size();
        let mut f = Self::blank(cols, rows);
        for y in 0..rows {
            for x in 0..cols {
                if let Some(vt) = screen.cell(y, x) {
                    f.set(
                        x,
                        y,
                        Cell {
                            symbol: if vt.contents().is_empty() {
                                " ".to_string()
                            } else {
                                vt.contents().to_string()
                            },
                            fg: conv(vt.fgcolor()),
                            bg: conv(vt.bgcolor()),
                            bold: vt.bold(),
                            dim: false,
                            italic: vt.italic(),
                            underline: vt.underline(),
                            reverse: vt.inverse(),
                        },
                    );
                }
            }
        }
        let (cy, cx) = screen.cursor_position();
        f.cursor = Some((cx, cy));
        f.cursor_visible = !screen.hide_cursor();
        f
    }
}
