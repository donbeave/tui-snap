//! The `termpane`-crate backend. Public types never leak from here: every
//! snapshot converts termpane's grid into termlens's own [`Screen`].

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::seq::{SeqEvent, SeqTracker};
use super::{Emulator, FrameSpan, InputModes, ModeState, MouseEncoding, Processed, Stop};
use crate::graphics::{GraphicsPayload, GraphicsSeen, HISTORY};
use crate::screen::{Cell, Color, MouseMode, Screen, Style, TermState};

pub(crate) struct TermpaneEmulator {
    grid: ::termpane::DamageGrid,
    tracker: SeqTracker,
    /// How many rows of history to retain (0 disables it entirely).
    scrollback_len: usize,
    /// When the current synchronized update began, stamped at the byte that
    /// opened it. `None` outside a frame.
    frame_started: Option<Instant>,
    /// Rows that have scrolled off the top, oldest first, as text.
    history: VecDeque<Arc<str>>,
    /// Rows of the grid's own scrollback already copied into `history`.
    captured: usize,
    graphics: Arc<Vec<GraphicsPayload>>,
    graphics_bytes: usize,
    capture: usize,
    staged: Vec<u8>,
}

impl TermpaneEmulator {
    fn close_frame(&mut self) -> FrameSpan {
        let started = self.frame_started.take();
        debug_assert!(
            started.is_some(),
            "a frame only ends where a Begin was seen, so the start must exist"
        );
        FrameSpan {
            duration: started.map_or(Duration::ZERO, |at| at.elapsed()),
            printable: self.tracker.take_frame_printable(),
        }
    }

    pub(crate) fn new(rows: u16, cols: u16, scrollback_len: usize, capture: usize) -> Self {
        Self {
            grid: ::termpane::DamageGrid::new(rows, cols, scrollback_len),
            tracker: SeqTracker::new(capture),
            scrollback_len,
            frame_started: None,
            history: VecDeque::new(),
            captured: 0,
            graphics: Arc::new(Vec::new()),
            graphics_bytes: 0,
            capture,
            staged: Vec::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.grid.process(bytes);
        // Drain typed events so the buffer stays bounded. Out-of-band state
        // for snapshots comes from the SeqTracker, keeping the grid and the
        // tracker on the same byte stream.
        let _ = self.grid.drain_passthrough();
        self.capture_scrolled_rows();
    }

    fn feed_staged(&mut self, tail: &[u8]) {
        if self.staged.is_empty() {
            self.feed(tail);
            return;
        }
        let mut staged = std::mem::take(&mut self.staged);
        staged.extend_from_slice(tail);
        self.feed(&staged);
        staged.clear();
        self.staged = staged;
    }

    fn record_graphics(&mut self, mut payload: GraphicsPayload) {
        let (row, col) = self.grid.cursor_position();
        let (_, cols) = self.grid.size();
        let col = col.min(cols.saturating_sub(1));
        payload.place((row, col));
        let kept = payload.data().map_or(0, <[u8]>::len);
        let log = Arc::make_mut(&mut self.graphics);
        log.push(payload);
        self.graphics_bytes += kept;
        while log.len() > HISTORY || (self.graphics_bytes > self.capture && log.len() > 1) {
            let dropped = log.remove(0);
            self.graphics_bytes -= dropped.data().map_or(0, <[u8]>::len);
        }
    }

    fn capture_scrolled_rows(&mut self) {
        if self.scrollback_len == 0 {
            return;
        }
        // The alternate screen owns no history.
        if self.grid.alternate_screen() {
            return;
        }
        let len = self.grid.scrollback_len().min(self.scrollback_len);
        let at_cap = len == self.scrollback_len;
        if !at_cap && len == self.captured {
            return;
        }
        let from = if at_cap {
            self.history.clear();
            0
        } else {
            self.captured
        };
        if len > from {
            let offset = len - from;
            for row in self.grid.scrollback_rows_at_offset(offset, offset) {
                self.history.push_back(Arc::from(row_text(row).as_ref()));
            }
        }
        while self.history.len() > self.scrollback_len {
            self.history.pop_front();
        }
        self.captured = len;
    }
}

fn row_text(row: &[::termpane::Cell]) -> Arc<str> {
    let mut out = String::new();
    for cell in row {
        if cell.is_wide_continuation {
            continue;
        }
        if cell.contents().is_empty() {
            out.push(' ');
        } else {
            out.push_str(cell.contents());
        }
    }
    Arc::from(out.trim_end())
}

impl Emulator for TermpaneEmulator {
    fn process(&mut self, bytes: &[u8]) -> Processed {
        let mut fed = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            if let Some(glyph) = self.tracker.charset_glyph(byte) {
                self.staged.extend_from_slice(&bytes[fed..i]);
                self.staged.extend_from_slice(glyph.as_bytes());
                fed = i + 1;
            }
            let stop = match self.tracker.step(byte) {
                SeqEvent::SyncEnd => Some(Stop::FrameComplete(self.close_frame())),
                SeqEvent::Query(query) => Some(Stop::Query(query)),
                SeqEvent::Graphics(payload) => {
                    self.feed_staged(&bytes[fed..=i]);
                    fed = i + 1;
                    self.record_graphics(*payload);
                    None
                }
                SeqEvent::SoftReset => {
                    self.feed_staged(&bytes[fed..=i]);
                    fed = i + 1;
                    self.feed(SOFT_RESET_REPLAY);
                    None
                }
                SeqEvent::None => None,
                SeqEvent::SyncBegin => {
                    self.frame_started = Some(Instant::now());
                    None
                }
            };
            if let Some(stop) = stop {
                self.feed_staged(&bytes[fed..=i]);
                return Processed {
                    consumed: i + 1,
                    stop: Some(stop),
                };
            }
        }
        self.feed_staged(&bytes[fed..]);
        Processed {
            consumed: bytes.len(),
            stop: None,
        }
    }

    fn snapshot(&self) -> Screen {
        let (rows, cols) = self.grid.size();
        let mut cells: Vec<Cell> = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        for row in 0..rows {
            for col in 0..cols {
                let mut converted = self.grid.cell(row, col).map_or_else(
                    || Cell::new(String::new(), Style::default(), false, false),
                    convert_cell,
                );
                if col > 0 && converted.is_wide_continuation() {
                    if let Some(lead) = cells.last() {
                        converted = Cell::new(String::new(), *lead.style(), false, true);
                    }
                }
                cells.push(converted);
            }
        }
        let (cursor_row, cursor_col) = self.grid.cursor_position();
        let cursor_col = cursor_col.min(cols.saturating_sub(1));
        let state = TermState {
            title: self.tracker.title(),
            alternate_screen: self.grid.alternate_screen(),
            bracketed_paste: self.grid.bracketed_paste(),
            application_cursor: self.grid.application_cursor(),
            mouse: convert_mouse(self.grid.mouse_protocol_mode()),
            mouse_modes: self.tracker.mouse_tracking(),
            clipboard: self.tracker.clipboard(),
            bells: self.tracker.bells(),
            focus_events: self.tracker.focus_events(),
            cursor_style: self.tracker.cursor_style(),
            links: self.tracker.links(),
            graphics: GraphicsSeen::new(self.tracker.graphics(), Arc::clone(&self.graphics)),
            repaints: 0,
            scrollback: self.history.iter().cloned().collect(),
        };
        Screen::from_parts(
            cols,
            rows,
            cursor_row,
            cursor_col,
            !self.grid.hide_cursor(),
            cells,
            state,
        )
    }

    fn mid_sequence(&self) -> bool {
        self.grid.mid_sequence() || self.tracker.mid_sequence()
    }

    fn in_sync_update(&self) -> bool {
        self.tracker.in_sync_update()
    }

    fn input_modes(&self) -> InputModes {
        InputModes {
            mouse: convert_mouse(self.grid.mouse_protocol_mode()),
            mouse_encoding: match self.grid.mouse_protocol_encoding() {
                ::termpane::MouseProtocolEncoding::Sgr => MouseEncoding::Sgr,
                ::termpane::MouseProtocolEncoding::Utf8 => MouseEncoding::Utf8,
                ::termpane::MouseProtocolEncoding::Default => MouseEncoding::Legacy,
                ::termpane::MouseProtocolEncoding::Urxvt => MouseEncoding::Legacy,
            },
            bracketed_paste: self.grid.bracketed_paste(),
            application_cursor: self.grid.application_cursor(),
            focus_events: self.tracker.focus_events(),
        }
    }

    fn mode_state(&self, mode: u32) -> ModeState {
        let on = |set: bool| {
            if set {
                ModeState::Set
            } else {
                ModeState::Reset
            }
        };
        match mode {
            2026 => on(self.tracker.in_sync_update()),
            1 => on(self.grid.application_cursor()),
            25 => on(!self.grid.hide_cursor()),
            47 | 1047 | 1049 => on(self.grid.alternate_screen()),
            2004 => on(self.grid.bracketed_paste()),
            1004 => on(self.tracker.focus_events()),
            1006 => on(matches!(
                self.grid.mouse_protocol_encoding(),
                ::termpane::MouseProtocolEncoding::Sgr
            )),
            1005 => on(matches!(
                self.grid.mouse_protocol_encoding(),
                ::termpane::MouseProtocolEncoding::Utf8
            )),
            9 => on(self.tracker.mouse_tracking().contains(MouseMode::Press)),
            1000 => on(self
                .tracker
                .mouse_tracking()
                .contains(MouseMode::PressRelease)),
            1002 => on(self
                .tracker
                .mouse_tracking()
                .contains(MouseMode::ButtonMotion)),
            1003 => on(self.tracker.mouse_tracking().contains(MouseMode::AnyMotion)),
            _ => ModeState::NotRecognized,
        }
    }

    fn set_size(&mut self, rows: u16, cols: u16) {
        self.grid.set_size(rows, cols);
        self.capture_scrolled_rows();
    }
}

const SOFT_RESET_REPLAY: &[u8] =
    b"\x1b[?1l\x1b[?2004l\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?25h";

fn convert_mouse(mode: ::termpane::MouseProtocolMode) -> MouseMode {
    match mode {
        ::termpane::MouseProtocolMode::None => MouseMode::None,
        ::termpane::MouseProtocolMode::Press => MouseMode::Press,
        ::termpane::MouseProtocolMode::PressRelease => MouseMode::PressRelease,
        ::termpane::MouseProtocolMode::ButtonMotion => MouseMode::ButtonMotion,
        ::termpane::MouseProtocolMode::AnyEvent => MouseMode::AnyMotion,
        ::termpane::MouseProtocolMode::AnyMotion => MouseMode::AnyMotion,
    }
}

fn convert_cell(cell: &::termpane::Cell) -> Cell {
    let style = Style {
        fg: convert_color(cell.fgcolor()),
        bg: convert_color(cell.bgcolor()),
        bold: cell.bold(),
        dim: cell.dim(),
        italic: cell.italic(),
        underline: cell.underline(),
        reverse: cell.inverse(),
        blink: cell.slow_blink() || cell.rapid_blink(),
        conceal: cell.conceal(),
        strikethrough: cell.strikethrough(),
    };
    Cell::new(
        cell.contents().to_string(),
        style,
        cell.is_wide,
        cell.is_wide_continuation,
    )
}

fn convert_color(color: ::termpane::Color) -> Color {
    match color {
        ::termpane::Color::Default => Color::Default,
        ::termpane::Color::Idx(i) => Color::Indexed(i),
        ::termpane::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
