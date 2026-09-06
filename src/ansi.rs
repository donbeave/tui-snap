//! Minimal SGR parser: ANSI bytes -> [`Frame`], [`Frame`] -> ANSI bytes.
//!
//! Same coverage as tcc `tools/ansi2html.py` (16 colors, 256, truecolor,
//! bold/dim/italic/underline/reverse), operating line-wise with state carried
//! across rows like `tmux capture-pane -e -p` output.

use crate::frame::{Cell, Color, Frame};

#[derive(Debug, Clone, Default)]
struct State {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    reverse: bool,
}

impl State {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn apply_params(&mut self, params: &str) {
        if params.is_empty() {
            self.reset();
            return;
        }
        let toks: Vec<u16> = params
            .split([';', ':'])
            .map(|t| t.parse().unwrap_or(0))
            .collect();
        let mut i = 0;
        while i < toks.len() {
            let t = toks[i];
            match t {
                0 => self.reset(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                7 => self.reverse = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                27 => self.reverse = false,
                30..=37 => self.fg = Some(Color::from_256((t - 30) as u8)),
                90..=97 => self.fg = Some(Color::from_256((t - 90 + 8) as u8)),
                40..=47 => self.bg = Some(Color::from_256((t - 40) as u8)),
                100..=107 => self.bg = Some(Color::from_256((t - 100 + 8) as u8)),
                39 => self.fg = None,
                49 => self.bg = None,
                38 | 48 => {
                    let mode = toks.get(i + 1).copied().unwrap_or(0);
                    if mode == 2 && i + 4 < toks.len() {
                        let c = Color::new(
                            toks[i + 2].min(255) as u8,
                            toks[i + 3].min(255) as u8,
                            toks[i + 4].min(255) as u8,
                        );
                        if t == 38 {
                            self.fg = Some(c);
                        } else {
                            self.bg = Some(c);
                        }
                        i += 4;
                    } else if mode == 5 && i + 2 < toks.len() {
                        let c = Color::from_256(toks[i + 2].min(255) as u8);
                        if t == 38 {
                            self.fg = Some(c);
                        } else {
                            self.bg = Some(c);
                        }
                        i += 2;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn cell(&self, ch: char) -> Cell {
        Cell {
            symbol: ch.to_string(),
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            dim: self.dim,
            italic: self.italic,
            underline: self.underline,
            reverse: self.reverse,
        }
    }
}

/// Parse `tmux capture-pane -e -p` style output (or any SGR stream) into a Frame.
/// Wide chars occupy one cell here (v1 limitation, documented); use the PTY
/// path for CJK-exact widths.
pub fn parse_ansi(text: &str, cols: u16, rows: u16) -> Frame {
    let mut frame = Frame::blank(cols, rows);
    let mut state = State::default();
    for (y, line) in text.split('\n').take(rows as usize).enumerate() {
        let mut x: u16 = 0;
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < bytes.len() && !bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j < bytes.len() {
                    let kind = bytes[j] as char;
                    let params = &line[i + 2..j];
                    if kind == 'm' {
                        state.apply_params(params);
                    }
                    // swallow all other CSI (cursor moves etc.): captures are
                    // full-screen dumps, like tcc `shot`.
                    i = j + 1;
                    continue;
                }
                break;
            }
            let ch = line[i..].chars().next().unwrap_or(' ');
            if x < cols {
                frame.set(x, y as u16, state.cell(ch));
                x += 1;
            }
            i += ch.len_utf8();
        }
    }
    frame
}

/// Serialize a Frame back to an SGR stream (one SGR run per styled span).
pub fn to_ansi(frame: &Frame) -> String {
    let mut out = String::new();
    for y in 0..frame.rows {
        let mut cur = String::new();
        for x in 0..frame.cols {
            let Some(c) = frame.get(x, y) else { continue };
            let sgr = sgr_for(c);
            if sgr != cur {
                out.push_str("\x1b[0m");
                if !sgr.is_empty() {
                    out.push_str(&format!("\x1b[{sgr}m"));
                }
                cur = sgr;
            }
            out.push_str(&c.symbol);
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

fn sgr_for(c: &Cell) -> String {
    let mut p: Vec<String> = vec![];
    if c.bold {
        p.push("1".into());
    }
    if c.dim {
        p.push("2".into());
    }
    if c.italic {
        p.push("3".into());
    }
    if c.underline {
        p.push("4".into());
    }
    if c.reverse {
        p.push("7".into());
    }
    if let Some(fg) = c.fg {
        p.push(format!("38;2;{};{};{}", fg.r, fg.g, fg.b));
    }
    if let Some(bg) = c.bg {
        p.push(format!("48;2;{};{};{}", bg.r, bg.g, bg.b));
    }
    p.join(";")
}
