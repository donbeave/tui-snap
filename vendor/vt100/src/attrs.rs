use crate::term::BufWrite as _;

/// Represents a foreground or background color for cells.
#[derive(Eq, PartialEq, Debug, Copy, Clone, Default)]
pub enum Color {
    /// The default terminal color.
    #[default]
    Default,

    /// An indexed terminal color.
    Idx(u8),

    /// An RGB terminal color. The parameters are (red, green, blue).
    Rgb(u8, u8, u8),
}

const TEXT_MODE_INTENSITY: u8 = 0b0000_0011;
const TEXT_MODE_BOLD: u8 = 0b0000_0001;
const TEXT_MODE_DIM: u8 = 0b0000_0010;
const TEXT_MODE_ITALIC: u8 = 0b0000_0100;
const TEXT_MODE_UNDERLINE: u8 = 0b0000_1000;
const TEXT_MODE_INVERSE: u8 = 0b0001_0000;
const TEXT_MODE_BLINK: u8 = 0b0010_0000;
const TEXT_MODE_HIDDEN: u8 = 0b0100_0000;
const TEXT_MODE_STRIKE: u8 = 0b1000_0000;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Attrs {
    pub fgcolor: Color,
    pub bgcolor: Color,
    pub mode: u8,
}

impl Attrs {
    pub fn bold(&self) -> bool {
        self.mode & TEXT_MODE_BOLD != 0
    }

    pub fn dim(&self) -> bool {
        self.mode & TEXT_MODE_DIM != 0
    }

    fn intensity(&self) -> u8 {
        self.mode & TEXT_MODE_INTENSITY
    }

    pub fn set_bold(&mut self) {
        self.mode |= TEXT_MODE_BOLD;
    }

    pub fn set_dim(&mut self) {
        self.mode |= TEXT_MODE_DIM;
    }

    pub fn set_normal_intensity(&mut self) {
        self.mode &= !TEXT_MODE_INTENSITY;
    }

    pub fn italic(&self) -> bool {
        self.mode & TEXT_MODE_ITALIC != 0
    }

    pub fn set_italic(&mut self, italic: bool) {
        if italic {
            self.mode |= TEXT_MODE_ITALIC;
        } else {
            self.mode &= !TEXT_MODE_ITALIC;
        }
    }

    pub fn underline(&self) -> bool {
        self.mode & TEXT_MODE_UNDERLINE != 0
    }

    pub fn set_underline(&mut self, underline: bool) {
        if underline {
            self.mode |= TEXT_MODE_UNDERLINE;
        } else {
            self.mode &= !TEXT_MODE_UNDERLINE;
        }
    }

    pub fn inverse(&self) -> bool {
        self.mode & TEXT_MODE_INVERSE != 0
    }

    pub fn set_inverse(&mut self, inverse: bool) {
        if inverse {
            self.mode |= TEXT_MODE_INVERSE;
        } else {
            self.mode &= !TEXT_MODE_INVERSE;
        }
    }

    pub fn blink(&self) -> bool { self.mode & TEXT_MODE_BLINK != 0 }
    pub fn set_blink(&mut self, value: bool) {
        if value { self.mode |= TEXT_MODE_BLINK; } else { self.mode &= !TEXT_MODE_BLINK; }
    }

    pub fn hidden(&self) -> bool { self.mode & TEXT_MODE_HIDDEN != 0 }
    pub fn set_hidden(&mut self, value: bool) {
        if value { self.mode |= TEXT_MODE_HIDDEN; } else { self.mode &= !TEXT_MODE_HIDDEN; }
    }

    pub fn strikethrough(&self) -> bool { self.mode & TEXT_MODE_STRIKE != 0 }
    pub fn set_strikethrough(&mut self, value: bool) {
        if value { self.mode |= TEXT_MODE_STRIKE; } else { self.mode &= !TEXT_MODE_STRIKE; }
    }

    pub fn write_escape_code_diff(
        &self,
        contents: &mut Vec<u8>,
        other: &Self,
    ) {
        if self != other && self == &Self::default() {
            crate::term::ClearAttrs.write_buf(contents);
            return;
        }

        let attrs = crate::term::Attrs::default();

        let attrs = if self.fgcolor == other.fgcolor {
            attrs
        } else {
            attrs.fgcolor(self.fgcolor)
        };
        let attrs = if self.bgcolor == other.bgcolor {
            attrs
        } else {
            attrs.bgcolor(self.bgcolor)
        };
        let attrs = if self.intensity() == other.intensity() {
            attrs
        } else {
            attrs.intensity(match self.intensity() {
                0 => crate::term::Intensity::Normal,
                TEXT_MODE_BOLD => crate::term::Intensity::Bold,
                TEXT_MODE_DIM => crate::term::Intensity::Dim,
                TEXT_MODE_INTENSITY => crate::term::Intensity::BoldDim,
                _ => unreachable!(),
            })
        };
        let attrs = if self.italic() == other.italic() {
            attrs
        } else {
            attrs.italic(self.italic())
        };
        let attrs = if self.underline() == other.underline() {
            attrs
        } else {
            attrs.underline(self.underline())
        };
        let attrs = if self.inverse() == other.inverse() {
            attrs
        } else {
            attrs.inverse(self.inverse())
        };

        attrs.write_buf(contents);
        for (changed, enabled, on, off) in [
            (self.blink() != other.blink(), self.blink(), b"\x1b[5m".as_slice(), b"\x1b[25m".as_slice()),
            (self.hidden() != other.hidden(), self.hidden(), b"\x1b[8m".as_slice(), b"\x1b[28m".as_slice()),
            (self.strikethrough() != other.strikethrough(), self.strikethrough(), b"\x1b[9m".as_slice(), b"\x1b[29m".as_slice()),
        ] {
            if changed { contents.extend_from_slice(if enabled { on } else { off }); }
        }
    }
}
