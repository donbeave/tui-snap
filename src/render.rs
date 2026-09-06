//! Exporters: txt / ansi / json / svg / html / png from one [`Frame`].
//!
//! Ideas borrowed:
//! - `freeze`/`termshot`: SVG+PNG screenshots with real background blocks.
//! - tcc `ansi2html.py`/`ansi2png.py`: standalone HTML, JetBrains-Mono PNG.
//! - `cellshot`: explicit `--format` repeats, JSON versioned schema.
//!
//! PNG here is dependency-light (no system font): exact backgrounds +
//! legibility blocks for glyphs. Swap in `fontdue`/embedded TTF for
//! publication-quality PNGs (roadmap, same geometry).

use crate::frame::{Color, Frame};
use anyhow::Result;

const DEFAULT_FG: &str = "#d0d0d0";
const DEFAULT_BG: &str = "#000000";

fn fg_hex(c: &crate::frame::Cell) -> String {
    let bg = c.bg.map(|c| c.to_hex()).unwrap_or(DEFAULT_BG.into());
    let mut fg = c.fg.map(|c| c.to_hex()).unwrap_or(DEFAULT_FG.into());
    if c.reverse {
        fg = bg.clone();
    }
    if c.dim {
        // mix 60% fg over bg, like tcc ansi2png.py
        let f = hex_rgb(&fg);
        let b = hex_rgb(&bg);
        let m = [
            (f[0] as f32 * 0.6 + b[0] as f32 * 0.4) as u8,
            (f[1] as f32 * 0.6 + b[1] as f32 * 0.4) as u8,
            (f[2] as f32 * 0.6 + b[2] as f32 * 0.4) as u8,
        ];
        return format!("#{:02x}{:02x}{:02x}", m[0], m[1], m[2]);
    }
    fg
}

fn bg_hex(c: &crate::frame::Cell) -> String {
    if c.reverse {
        return c.fg.map(|c| c.to_hex()).unwrap_or(DEFAULT_FG.into());
    }
    c.bg.map(|c| c.to_hex()).unwrap_or(DEFAULT_BG.into())
}

fn hex_rgb(h: &str) -> [u8; 3] {
    let h = h.trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    [v(0), v(2), v(4)]
}

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn esc_html(s: &str) -> String {
    esc_xml(s)
}

/// SVG screenshot (freeze-style, editable text).
pub fn to_svg(frame: &Frame) -> String {
    const CW: u32 = 9;
    const CH: u32 = 18;
    const PAD: u32 = 12;
    let w = frame.cols as u32 * CW + PAD * 2;
    let h = frame.rows as u32 * CH + PAD * 2;
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" font-family=\"'JetBrainsMono Nerd Font Mono','JetBrains Mono',Menlo,monospace\" font-size=\"14\">\n<rect width=\"100%\" height=\"100%\" fill=\"{DEFAULT_BG}\"/>\n"
    );
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let c = frame.get(x, y).cloned().unwrap_or_default();
            let bg = bg_hex(&c);
            let px = PAD + x as u32 * CW;
            let py = PAD + y as u32 * CH;
            if bg != DEFAULT_BG {
                s.push_str(&format!(
                    "<rect x=\"{px}\" y=\"{py}\" width=\"{CW}\" height=\"{CH}\" fill=\"{bg}\"/>\n"
                ));
            }
            if c.symbol != " " {
                let fg = fg_hex(&c);
                let weight = if c.bold { " font-weight=\"bold\"" } else { "" };
                let style = if c.italic {
                    " font-style=\"italic\""
                } else {
                    ""
                };
                s.push_str(&format!(
                    "<text x=\"{px}\" y=\"{}\" fill=\"{fg}\"{weight}{style}>{}</text>\n",
                    py + 13,
                    esc_xml(&c.symbol)
                ));
            }
        }
    }
    s.push_str("</svg>\n");
    s
}

/// Standalone HTML page (tcc `ansi2html.py` equivalent, no Python).
pub fn to_html(frame: &Frame) -> String {
    let mut body = String::new();
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let c = frame.get(x, y).cloned().unwrap_or_default();
            let fg = fg_hex(&c);
            let bg = bg_hex(&c);
            let mut css = format!("color:{fg};background:{bg}");
            if c.bold {
                css.push_str(";font-weight:700");
            }
            if c.dim {
                css.push_str(";opacity:.6");
            }
            if c.italic {
                css.push_str(";font-style:italic");
            }
            if c.underline {
                css.push_str(";text-decoration:underline");
            }
            body.push_str(&format!(
                "<span style=\"{css}\">{}</span>",
                esc_html(&c.symbol)
            ));
        }
        body.push('\n');
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>tuisnap</title>\n<style>\nhtml,body{{margin:0;background:#1a1a1a}}\npre{{margin:16px;display:inline-block;font-family:\"JetBrainsMono Nerd Font Mono\",\"JetBrains Mono\",Menlo,monospace;font-size:14px;line-height:18px;white-space:pre;background:{DEFAULT_BG}}}\n</style></head><body><pre>{body}</pre></body></html>"
    )
}

/// PNG via `image` crate. Geometry matches tcc `ansi2png.py`
/// (9x20 cell at 15px + 12px pad); glyphs render as fg blocks sized by
/// symbol weight so layout/diff review works without a font dependency.
pub fn to_png_bytes(frame: &Frame) -> Result<Vec<u8>> {
    const CW: u32 = 9;
    const CH: u32 = 20;
    const PAD: u32 = 12;
    let w = frame.cols as u32 * CW + PAD * 2;
    let h = frame.rows as u32 * CH + PAD * 2;
    let bg0 = hex_rgb(DEFAULT_BG);
    let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([bg0[0], bg0[1], bg0[2]]));
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let c = frame.get(x, y).cloned().unwrap_or_default();
            let bg = hex_rgb(&bg_hex(&c));
            let fg = hex_rgb(&fg_hex(&c));
            let px = PAD + x as u32 * CW;
            let py = PAD + y as u32 * CH;
            for dy in 0..CH {
                for dx in 0..CW {
                    img.put_pixel(px + dx, py + dy, image::Rgb([bg[0], bg[1], bg[2]]));
                }
            }
            if c.symbol != " " {
                // legibility block: centered bar, bold = taller, underline = bottom row
                let (bw, bh) = if c.symbol.trim().is_empty() {
                    (0, 0)
                } else if c.bold {
                    (6, 12)
                } else {
                    (5, 9)
                };
                let ox = px + (CW - bw) / 2;
                let oy = py + (CH - bh) / 2;
                for dy in 0..bh {
                    for dx in 0..bw {
                        img.put_pixel(ox + dx, oy + dy, image::Rgb([fg[0], fg[1], fg[2]]));
                    }
                }
                if c.underline {
                    for dx in 0..CW {
                        img.put_pixel(px + dx, py + CH - 3, image::Rgb([fg[0], fg[1], fg[2]]));
                    }
                }
            }
            // cursor: white outline cell
            if frame.cursor_visible && frame.cursor == Some((x, y)) {
                for dx in 0..CW {
                    img.put_pixel(px + dx, py, image::Rgb([255, 255, 255]));
                    img.put_pixel(px + dx, py + CH - 1, image::Rgb([255, 255, 255]));
                }
                for dy in 0..CH {
                    img.put_pixel(px, py + dy, image::Rgb([255, 255, 255]));
                    img.put_pixel(px + CW - 1, py + dy, image::Rgb([255, 255, 255]));
                }
            }
        }
    }
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)?;
    Ok(buf)
}

/// Write one `--format` artifact next to `out_prefix` (mirrors cellshot:
/// `--out captures/home` + `--format png` -> `captures/home.png`).
pub fn write_format(frame: &Frame, format: &str, out_prefix: &str) -> Result<String> {
    let path = format!("{out_prefix}.{format}");
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    match format {
        "txt" => std::fs::write(&path, frame.text())?,
        "ansi" => std::fs::write(&path, crate::ansi::to_ansi(frame))?,
        "json" => std::fs::write(&path, serde_json::to_string_pretty(frame)?)?,
        "svg" => std::fs::write(&path, to_svg(frame))?,
        "html" => std::fs::write(&path, to_html(frame))?,
        "png" => std::fs::write(&path, to_png_bytes(frame)?)?,
        _ => anyhow::bail!("unknown format: {format} (txt|ansi|json|svg|html|png)"),
    }
    Ok(path)
}

#[allow(dead_code)]
fn _color_use(c: Color) -> String {
    c.to_hex()
}
