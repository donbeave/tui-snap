//! Pinned-profile rendering: canonical [`Frame`] → PNG / SVG / ANSI / text.
//!
//! The PNG path rasterizes **real glyphs** with `fontdue` from pinned font
//! bytes — never placeholder blocks. [`verify_geometry`] fails loudly if the
//! font's measured advance/line-height drifts from the profile constants, so
//! a font change reads as a renderer change, not an app regression.
//!
//! Fidelity contract (measured, terminal-like — NOT pixel-identity with any
//! particular terminal emulator):
//! - layout from frame widths (wide = 2 cells, continuation = 0); CJK keeps
//!   2-cell geometry even when the glyph is missing (tofu fallback);
//! - faux-bold via double-strike, faux-italic via shear (documented
//!   approximations; the cell data stays authoritative for styles);
//! - underline / strikethrough drawn at fixed offsets from the baseline;
//! - blink frozen as visible; concealment unsupported (see [`crate::frame`]).

use crate::frame::{Frame, Rgb};
use crate::profile::Profile;
use fontdue::{Font, FontSettings};

/// Import/render failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError(pub String);

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "render error: {}", self.0)
    }
}

impl std::error::Error for RenderError {}

/// A loaded raster font with line metrics.
pub struct LoadedFont {
    font: Font,
    /// Pixels above baseline.
    pub ascent: f32,
    /// Pixels below baseline (nonnegative).
    pub descent: f32,
    pub px: f32,
}

/// Load + measure a font.
pub fn load_font(bytes: &[u8], px: f32) -> Result<LoadedFont, RenderError> {
    let font = Font::from_bytes(bytes, FontSettings::default())
        .map_err(|e| RenderError(format!("cannot parse font: {e}")))?;
    let lm = font
        .horizontal_line_metrics(px)
        .ok_or_else(|| RenderError("font has no horizontal metrics".to_string()))?;
    Ok(LoadedFont {
        font,
        ascent: lm.ascent,
        descent: lm.descent.abs(),
        px,
    })
}

/// Measured advance of `M` and line height at profile size.
pub fn measure(loaded: &LoadedFont) -> (f32, f32) {
    let adv = loaded.font.rasterize('M', loaded.px).0.advance_width;
    (adv, loaded.ascent + loaded.descent)
}

/// Fail unless the font measures exactly like the profile pins.
/// Call before every gate render.
pub fn verify_geometry(loaded: &LoadedFont, profile: &Profile) -> Result<(), RenderError> {
    let (adv, line_h) = measure(loaded);
    if adv.round() as u32 != profile.cell_w || line_h.round() as u32 != profile.cell_h {
        return Err(RenderError(format!(
            "font/geometry pin broken: measured advance {adv:.2} line {line_h:.2}, profile pins {}x{} — refusing to render",
            profile.cell_w, profile.cell_h
        )));
    }
    Ok(())
}

fn blend(dst: &mut image::RgbImage, x: u32, y: u32, fg: Rgb, cov: u8) {
    if cov == 0 {
        return;
    }
    let (w, h) = (dst.width(), dst.height());
    if x >= w || y >= h {
        return;
    }
    let p = dst.get_pixel_mut(x, y);
    let a = u32::from(cov);
    p[0] = ((u32::from(fg.r) * a + u32::from(p[0]) * (255 - a)) / 255) as u8;
    p[1] = ((u32::from(fg.g) * a + u32::from(p[1]) * (255 - a)) / 255) as u8;
    p[2] = ((u32::from(fg.b) * a + u32::from(p[2]) * (255 - a)) / 255) as u8;
}

fn fill_rect(dst: &mut image::RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb) {
    let (dw, dh) = (dst.width(), dst.height());
    for dy in 0..h {
        for dx in 0..w {
            let (px, py) = (x + dx, y + dy);
            if px < dw && py < dh {
                dst.put_pixel(px, py, image::Rgb([c.r, c.g, c.b]));
            }
        }
    }
}

/// Deterministic tofu box for missing glyphs (spans `span_px` wide).
fn draw_tofu(dst: &mut image::RgbImage, x0: i32, top: i32, span_px: u32, h: u32, fg: Rgb) {
    let w = span_px.saturating_sub(2).max(3);
    for dx in 0..w {
        blend(
            dst,
            (x0 + 1 + dx as i32).max(0) as u32,
            top.max(0) as u32,
            fg,
            255,
        );
        blend(
            dst,
            (x0 + 1 + dx as i32).max(0) as u32,
            (top + h as i32 - 1).max(0) as u32,
            fg,
            255,
        );
    }
    for dy in 0..h {
        blend(
            dst,
            (x0 + 1).max(0) as u32,
            (top + dy as i32).max(0) as u32,
            fg,
            255,
        );
        blend(
            dst,
            (x0 + 1 + w as i32 - 1).max(0) as u32,
            (top + dy as i32).max(0) as u32,
            fg,
            255,
        );
    }
}

/// Draw one lead-cell symbol at pen origin. Combining scalars overlay at the
/// same origin (documented approximation of terminal combining behavior).
#[allow(clippy::too_many_arguments)]
fn draw_symbol(
    dst: &mut image::RgbImage,
    loaded: &LoadedFont,
    symbol: &str,
    pen_x: i32,
    baseline: i32,
    span_px: u32,
    cell_top: i32,
    cell_h: u32,
    fg: Rgb,
    bold: bool,
    italic: bool,
) {
    for c in symbol.chars() {
        if loaded.font.lookup_glyph_index(c) == 0 {
            draw_tofu(
                dst,
                pen_x,
                cell_top + 2,
                span_px,
                cell_h.saturating_sub(4),
                fg,
            );
            continue;
        }
        let (m, bmp) = loaded.font.rasterize(c, loaded.px);
        if m.width == 0 || m.height == 0 {
            continue;
        }
        // ymin = offset of the bitmap's BOTTOM edge from the baseline, so the
        // top edge sits at baseline - (ymin + height).
        let top = baseline - (m.ymin + m.height as i32);
        for (i, &cov) in bmp.iter().enumerate() {
            if cov == 0 {
                continue;
            }
            let bx = (i % m.width) as i32;
            let by = (i / m.width) as i32;
            // Faux italic: shear top rows right (documented approximation).
            let shear = if italic {
                ((m.height as i32 - 1 - by) as f32 * 0.15) as i32
            } else {
                0
            };
            let dx = pen_x + m.xmin + bx + shear;
            let dy = top + by;
            if dx >= 0 && dy >= 0 {
                blend(dst, dx as u32, dy as u32, fg, cov);
                // Faux bold: double-strike one pixel right.
                if bold {
                    blend(dst, (dx + 1) as u32, dy as u32, fg, cov);
                }
            }
        }
    }
}

/// Render a validated frame to PNG bytes under `profile`.
pub fn render_png(
    frame: &Frame,
    profile: &Profile,
    font_bytes: &[u8],
) -> Result<Vec<u8>, RenderError> {
    frame
        .validate()
        .map_err(|e| RenderError(format!("refusing to render: {e}")))?;
    let loaded = load_font(font_bytes, profile.font_px)?;
    verify_geometry(&loaded, profile)?;

    let w1 = frame.cols as u32 * profile.cell_w + profile.pad * 2;
    let h1 = frame.rows as u32 * profile.cell_h + profile.pad * 2;
    let bg = profile.default_bg;
    let mut img = image::RgbImage::from_pixel(w1, h1, image::Rgb([bg.r, bg.g, bg.b]));

    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let Some(cell) = frame.get(x, y) else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            let (fg, cbg) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let span = u32::from(cell.width.max(1)) * profile.cell_w;
            let cx = profile.pad + x as u32 * profile.cell_w;
            let cy = profile.pad + y as u32 * profile.cell_h;
            if cbg != profile.default_bg {
                fill_rect(&mut img, cx, cy, span, profile.cell_h, cbg);
            }
            if cell.symbol.trim().is_empty() {
                continue;
            }
            let baseline = cy as i32 + loaded.ascent.round() as i32;
            draw_symbol(
                &mut img,
                &loaded,
                &cell.symbol,
                cx as i32,
                baseline,
                span,
                cy as i32,
                profile.cell_h,
                fg,
                cell.mods.bold,
                cell.mods.italic,
            );
            if cell.mods.underline {
                let uy = (baseline + 2).min((cy + profile.cell_h - 1) as i32);
                let th = if cell.mods.bold { 2 } else { 1 };
                for t in 0..th {
                    for dx in 0..span {
                        blend(&mut img, cx + dx, (uy + t) as u32, fg, 255);
                    }
                }
            }
            if cell.mods.strikethrough {
                let sy = baseline - (loaded.ascent * 0.35) as i32;
                for dx in 0..span {
                    blend(&mut img, cx + dx, sy.max(0) as u32, fg, 255);
                }
            }
        }
    }

    // Block cursor: fill cell with fg, redraw glyph in bg (classic terminal).
    if frame.cursor.visible && profile.cursor_visible {
        let mut cx = frame.cursor.x;
        if frame
            .get(cx, frame.cursor.y)
            .is_some_and(|c| c.continuation)
        {
            cx = cx.saturating_sub(1);
        }
        if let Some(cell) = frame.get(cx, frame.cursor.y) {
            let (fg, cbg) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let span = u32::from(cell.width.max(1)) * profile.cell_w;
            let px = profile.pad + cx as u32 * profile.cell_w;
            let py = profile.pad + frame.cursor.y as u32 * profile.cell_h;
            let style = frame.cursor.style;
            match style {
                crate::frame::CursorStyle::Block => {
                    fill_rect(&mut img, px, py, span, profile.cell_h, fg);
                    if !cell.symbol.trim().is_empty() {
                        let baseline = py as i32 + loaded.ascent.round() as i32;
                        draw_symbol(
                            &mut img,
                            &loaded,
                            &cell.symbol,
                            px as i32,
                            baseline,
                            span,
                            py as i32,
                            profile.cell_h,
                            cbg,
                            false,
                            false,
                        );
                    }
                }
                crate::frame::CursorStyle::Underline => {
                    let uy = (py + profile.cell_h - 2) as i32;
                    for dx in 0..span {
                        blend(&mut img, px + dx, uy as u32, fg, 255);
                        blend(&mut img, px + dx, (uy + 1) as u32, fg, 255);
                    }
                }
                crate::frame::CursorStyle::Bar => {
                    for dy in 0..profile.cell_h {
                        blend(&mut img, px, py + dy, fg, 255);
                        blend(&mut img, px + 1, py + dy, fg, 255);
                    }
                }
            }
        }
    }

    let (w, h) = (w1 * profile.scale, h1 * profile.scale);
    let big = image::imageops::resize(&img, w, h, image::imageops::FilterType::Nearest);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(big)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| RenderError(format!("PNG encode: {e}")))?;
    Ok(out)
}

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Selectable-text SVG (secondary evidence: viewer fonts apply, so the PNG
/// stays authoritative for pixel gates).
pub fn render_svg(frame: &Frame, profile: &Profile) -> String {
    let cw = profile.cell_w;
    let ch = profile.cell_h;
    let pad = profile.pad;
    let w = frame.cols as u32 * cw + pad * 2;
    let h = frame.rows as u32 * ch + pad * 2;
    let bg = profile.default_bg.to_hex();
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" font-family=\"'DejaVuSansM Nerd Font Mono','DejaVu Sans Mono',monospace\" font-size=\"{}\">\n<rect width=\"100%\" height=\"100%\" fill=\"{bg}\"/>\n",
        profile.font_px as u32
    );
    for y in 0..frame.rows {
        let mut x = 0u16;
        while x < frame.cols {
            let Some(cell) = frame.get(x, y) else {
                x += 1;
                continue;
            };
            if cell.continuation {
                x += 1;
                continue;
            }
            // Coalesce the maximal run of identical-style cells so words stay
            // selectable as one <text> element. Spaces join the run (same
            // advance in monospace); continuations break it (the lead's wide
            // advance is handled by cell geometry, not font metrics).
            let (fg0, bg0) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let key = (
                fg0,
                bg0,
                cell.mods.bold,
                cell.mods.italic,
                cell.mods.underline,
                cell.mods.strikethrough,
            );
            let mut run = String::new();
            let mut nx = x;
            while nx < frame.cols {
                let Some(c) = frame.get(nx, y) else { break };
                if c.continuation {
                    break;
                }
                let (fg, bg) = Frame::resolve_cell(c, profile.default_fg, profile.default_bg);
                if (
                    fg,
                    bg,
                    c.mods.bold,
                    c.mods.italic,
                    c.mods.underline,
                    c.mods.strikethrough,
                ) != key
                {
                    break;
                }
                run.push_str(&c.symbol);
                // Advance by display width (wide cells occupy 2 columns but
                // hold one grapheme in the lead cell).
                nx += u16::from(c.width.max(1));
            }
            let span_cols = nx - x;
            let px = pad + x as u32 * cw;
            let py = pad + y as u32 * ch;
            if bg0 != profile.default_bg {
                s.push_str(&format!(
                    "<rect x=\"{px}\" y=\"{py}\" width=\"{}\" height=\"{ch}\" fill=\"{}\"/>\n",
                    span_cols as u32 * cw,
                    bg0.to_hex()
                ));
            }
            let weight = if cell.mods.bold {
                " font-weight=\"bold\""
            } else {
                ""
            };
            let style = if cell.mods.italic {
                " font-style=\"italic\""
            } else {
                ""
            };
            let attrs = format!("{weight}{style}");
            s.push_str(&format!(
                "<text x=\"{px}\" y=\"{}\" fill=\"{}\"{}>{}</text>\n",
                py + ch - 4,
                fg0.to_hex(),
                attrs,
                esc_xml(&run)
            ));
            x = nx;
        }
    }
    s.push_str("</svg>\n");
    s
}

/// Normalized ANSI dump (SGR runs from canonical state — for debugging, not
/// for replay; replay raw streams with [`crate::ansi::replay_raw`]).
pub fn ansi_dump(frame: &Frame) -> String {
    let mut out = String::new();
    for y in 0..frame.rows {
        let mut cur = String::new();
        for x in 0..frame.cols {
            let Some(c) = frame.get(x, y) else { continue };
            if c.continuation {
                continue;
            }
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

fn sgr_for(c: &crate::frame::Cell) -> String {
    let mut p: Vec<String> = Vec::new();
    if c.mods.bold {
        p.push("1".into());
    }
    if c.mods.dim {
        p.push("2".into());
    }
    if c.mods.italic {
        p.push("3".into());
    }
    if c.mods.underline {
        p.push("4".into());
    }
    if c.mods.strikethrough {
        p.push("9".into());
    }
    if c.mods.reverse {
        p.push("7".into());
    }
    let push_color = |p: &mut Vec<String>, code: u8, c: crate::frame::Color| match c {
        crate::frame::Color::Default => {}
        crate::frame::Color::Indexed(i) => p.push(format!("{code};5;{i}")),
        crate::frame::Color::Rgb(r) => {
            p.push(format!("{code};2;{};{};{}", r.r, r.g, r.b));
        }
    };
    push_color(&mut p, 38, c.fg);
    push_color(&mut p, 48, c.bg);
    p.join(";")
}
