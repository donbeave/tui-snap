//! Pinned-profile rendering: canonical [`Frame`] → PNG / SVG / ANSI / text.
//!
//! The PNG path rasterizes **real glyphs** with `fontdue` from pinned font
//! bytes — never placeholder blocks. Glyphs are rasterized at the FINAL scale
//! (`font_px * scale`) straight onto the output image, so HiDPI output keeps
//! real font hinting/coverage gradations instead of nearest-neighbor 2×2
//! blocks. [`verify_geometry`] fails loudly if the regular face's measured
//! advance/line-height drifts from the profile constants, so a font change
//! reads as a renderer change, not an app regression.
//!
//! Fidelity contract (measured, terminal-like — NOT pixel-identity with any
//! particular terminal emulator):
//! - layout from frame widths (wide = 2 cells, continuation = 0); CJK keeps
//!   2-cell geometry even when the glyph is missing (tofu fallback);
//! - bold / italic / bold-italic use the REAL faces of the pinned family
//!   (faux double-strike / shear survive only as fallback when a face fails
//!   to load or the family is a single-face override);
//! - per-glyph face chain: styled face → regular face → vendored fallback
//!   faces ([`crate::profile::VENDORED_FALLBACK_FACES`], sha256-pinned Noto
//!   subsets) → tofu; a face covers a codepoint only when it rasterizes a
//!   non-empty bitmap (cmap index is not enough); fallback glyphs are
//!   centered and clipped inside the primary cell box and drawn in the
//!   fallback face's own weight; default-ignorable codepoints (VS16, ZWJ)
//!   never tofu;
//! - underline / strikethrough drawn at fixed offsets from the baseline,
//!   including across whitespace cells (as real terminals do);
//! - blink frozen as visible; concealed glyphs omitted (see [`crate::frame`]);
//! - glyphs no face in the chain covers draw a deterministic tofu box AND are
//!   reported in the [`Fidelity`] record (the `.png.fidelity.json` sidecar),
//!   and glyphs served by a fallback face are recorded there too — exact
//!   reporting, never silent tofu.

use crate::frame::{Frame, Rgb};
use crate::profile::{FontFaces, Profile};
use fontdue::{Font, FontSettings};
use serde::Serialize;

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
    /// Human-readable face identity (fallback faces: the pinned description).
    pub desc: String,
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
        desc: String::new(),
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

/// The four faces of one family, loaded at one pixel size, plus the per-glyph
/// fallback chain. Faces share the regular face's baseline (cell grid
/// authority). A non-regular face that fails to parse falls back to the
/// regular face and is named in `fell_back` (the faux styles then return for
/// it). Fallback faces are coverage-only: they serve single glyphs the
/// primary family lacks, centered and clipped inside the primary cell box;
/// they never move the cell grid.
pub struct FontSet {
    pub regular: LoadedFont,
    pub bold: LoadedFont,
    pub italic: LoadedFont,
    pub bold_italic: LoadedFont,
    /// Non-regular faces that failed to parse and fell back to regular.
    pub fell_back: Vec<&'static str>,
    /// Per-glyph fallback chain, tried in order after the primary family.
    pub fallbacks: Vec<LoadedFont>,
}

impl FontSet {
    pub fn load(faces: &FontFaces<'_>, px: f32) -> Result<Self, RenderError> {
        Self::load_with_fallbacks(faces, px, &[])
    }

    /// Load the styled family plus a pinned fallback chain. Each fallback
    /// face's bytes are verified against its pinned SHA-256 before parsing;
    /// a hash mismatch or an unparsable face fails the load (explicit, never
    /// silent — a swapped/corrupt font must read as a renderer change).
    pub fn load_with_fallbacks(
        faces: &FontFaces<'_>,
        px: f32,
        fallbacks: &[crate::profile::FallbackFace<'_>],
    ) -> Result<Self, RenderError> {
        let regular = load_font(faces.regular, px)?;
        let mut fell_back = Vec::new();
        let mut face = |bytes: &[u8], name: &'static str| match load_font(bytes, px) {
            Ok(f) => f,
            Err(_) => {
                fell_back.push(name);
                load_font(faces.regular, px).expect("regular face parsed above")
            }
        };
        let mut loaded_fallbacks = Vec::with_capacity(fallbacks.len());
        if fallbacks.len() > u8::MAX as usize {
            return Err(RenderError(format!(
                "fallback chain too long: {} faces (max 255)",
                fallbacks.len()
            )));
        }
        for f in fallbacks {
            let actual = crate::profile::font_sha256(f.bytes);
            if actual != f.sha256 {
                return Err(RenderError(format!(
                    "fallback face '{}' sha256 mismatch: pinned {}, got {actual} — refusing to render",
                    f.desc, f.sha256
                )));
            }
            let mut lf = load_font(f.bytes, px)
                .map_err(|e| RenderError(format!("fallback face '{}': {e}", f.desc)))?;
            lf.desc = f.desc.to_string();
            loaded_fallbacks.push(lf);
        }
        Ok(Self {
            bold: face(faces.bold, "bold"),
            italic: face(faces.italic, "italic"),
            bold_italic: face(faces.bold_italic, "bold_italic"),
            regular,
            fell_back,
            fallbacks: loaded_fallbacks,
        })
    }

    /// The face `cell.mods` selects, before per-glyph coverage fallback.
    fn styled(&self, bold: bool, italic: bool) -> &LoadedFont {
        match (bold, italic) {
            (true, true) => &self.bold_italic,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (false, false) => &self.regular,
        }
    }
}

/// One cell whose glyph(s) no face in the chain covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingGlyph {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    /// Uncovered codepoints, formatted `U+26B7`.
    pub codepoints: Vec<String>,
}

/// One cell whose glyph(s) the primary family did not cover but a fallback
/// face rendered as real ink (see [`Fidelity::fallback_glyphs`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FallbackGlyph {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    /// Fallback-served codepoints, formatted `U+26B7`.
    pub codepoints: Vec<String>,
    /// Fallback face(s) that rendered them, in chain order.
    pub faces: Vec<String>,
}

/// Exact coverage accounting for one rendered frame — written next to PNG
/// outputs as `<name>.png.fidelity.json`. `approximate` is true when any
/// glyph is missing or any styled face fell back (mirroring the legacy
/// sidecar's purpose: a PNG is labelled approximate when it must be).
#[derive(Debug, Clone, Serialize)]
pub struct Fidelity {
    pub profile: String,
    pub font_sha256: String,
    pub font_desc: String,
    pub scale: u32,
    pub approximate: bool,
    /// Styled faces that failed to parse and fell back to regular.
    pub faces_fell_back: Vec<String>,
    pub missing: Vec<MissingGlyph>,
    /// Cells a fallback face rendered (omitted from the JSON when empty, so
    /// sidecars of primary-covered frames stay byte-stable).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallback_glyphs: Vec<FallbackGlyph>,
}

impl Fidelity {
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Fidelity is plain serializable data")
    }
}

/// A rendered PNG plus its fidelity record.
pub struct Rendered {
    pub png: Vec<u8>,
    pub fidelity: Fidelity,
}

/// The four snapshot artifacts of one frame, generated in one render pass:
/// the normalized SGR dump ([`ansi_dump`]), plain text ([`Frame::text`]), the
/// standalone HTML view ([`Renderer::render_html`]) and the authoritative
/// PNG. Bytes are deterministic for identical frames (the HTML embed
/// normalizes the provenance timestamp — see [`Renderer::render_html`]).
pub struct Artifacts {
    /// Colored terminal text (normalized SGR dump).
    pub ansi: String,
    /// Plain black-and-white text.
    pub txt: String,
    /// Standalone colored HTML render.
    pub html: String,
    /// Colored image (authoritative pixel-gate evidence).
    pub png: Vec<u8>,
    /// Coverage accounting of the PNG render.
    pub fidelity: Fidelity,
}

/// Glyph rasters keyed by character and face, negative results included
/// (`None` = rasterized once to an empty bitmap — never re-rasterized).
type GlyphCache = std::collections::HashMap<GlyphKey, Option<(fontdue::Metrics, Vec<u8>)>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    face: FaceIdx,
}

/// The face a glyph was actually rasterized from. Rasterize size is fixed
/// per [`Renderer`] (`font_px * scale`), so it is not part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FaceIdx {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    /// Index into `FontSet::fallbacks` (chains are short: < 256 faces).
    Fallback(u8),
}

fn face_idx(bold: bool, italic: bool) -> FaceIdx {
    match (bold, italic) {
        (true, true) => FaceIdx::BoldItalic,
        (true, false) => FaceIdx::Bold,
        (false, true) => FaceIdx::Italic,
        (false, false) => FaceIdx::Regular,
    }
}

/// A reusable renderer: parses the pinned faces ONCE at construction (the
/// geometry pin is verified there too) and caches glyph rasters across
/// frames, so a bulk gate costs O(distinct glyphs) rasterizations instead of
/// 8 font parses plus a full re-rasterization per frame.
///
/// Threading: every method takes `&mut self`, so the borrow checker enforces
/// exclusive use — give each parallel test thread its own instance
/// (`thread_local!` is the convenient carrier), matching the per-thread
/// session confinement of the PTY layer.
pub struct Renderer {
    profile: Profile,
    set: FontSet,
    glyphs: GlyphCache,
}

impl Renderer {
    /// Load the faces and verify the geometry pin (once for all renders).
    /// The per-glyph fallback chain is the vendored default
    /// ([`crate::profile::VENDORED_FALLBACK_FACES`]); use
    /// [`Self::with_fallbacks`] to replace it.
    pub fn new(profile: &Profile, faces: &FontFaces<'_>) -> Result<Self, RenderError> {
        Self::with_fallbacks(profile, faces, crate::profile::VENDORED_FALLBACK_FACES)
    }

    /// Like [`Self::new`] but with an explicit per-glyph fallback chain,
    /// tried in order after the primary family (pass `&[]` for
    /// primary-family-only rendering). Each face's bytes are verified
    /// against its pinned SHA-256 before parsing; a mismatch refuses to
    /// render. Primary geometry stays pinned to `faces.regular` regardless —
    /// fallback faces only fill coverage holes inside the pinned cell box.
    pub fn with_fallbacks(
        profile: &Profile,
        faces: &FontFaces<'_>,
        fallbacks: &[crate::profile::FallbackFace<'_>],
    ) -> Result<Self, RenderError> {
        // Geometry pins are UNSCALED metrics; the gate compares against the
        // profile constants directly.
        let unscaled = load_font(faces.regular, profile.font_px)?;
        verify_geometry(&unscaled, profile)?;
        // HiDPI: rasterize glyphs at the final scale, no post upscale.
        let set =
            FontSet::load_with_fallbacks(faces, profile.font_px * profile.scale as f32, fallbacks)?;
        Ok(Self {
            profile: profile.clone(),
            set,
            glyphs: GlyphCache::new(),
        })
    }

    /// The profile this renderer is pinned to.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Distinct `(char, face)` rasters currently cached (diagnostics).
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }

    /// Render a validated frame to PNG bytes.
    pub fn render_png(&mut self, frame: &Frame) -> Result<Vec<u8>, RenderError> {
        Ok(self.render(frame)?.png)
    }

    /// Standalone colored HTML render of a frame: the authoritative PNG as
    /// the primary `<img>` (real glyphs, including CJK/symbols the viewer
    /// font would tofu), a selectable SVG overlay with transparent fills
    /// (viewer fonts, copy/select only), and the canonical frame JSON
    /// embedded in a `<script type="application/json">` for lossless re-import.
    ///
    /// The embedded JSON carries `provenance.created_unix = 0`: the timestamp
    /// is informational only (excluded from every gate by design), and
    /// zeroing it keeps the document byte-deterministic for identical
    /// screens. Every other field is preserved.
    pub fn render_html(&mut self, frame: &Frame, title: &str) -> Result<String, RenderError> {
        let rendered = self.render(frame)?;
        Ok(html_document(frame, &self.profile, title, &rendered.png))
    }

    /// Generate all four snapshot artifacts in one render pass (the PNG is
    /// rasterized once and shared by the HTML embed and the PNG artifact).
    pub fn render_artifacts(
        &mut self,
        frame: &Frame,
        title: &str,
    ) -> Result<Artifacts, RenderError> {
        let rendered = self.render(frame)?;
        Ok(Artifacts {
            ansi: ansi_dump(frame),
            txt: frame.text(),
            html: html_document(frame, &self.profile, title, &rendered.png),
            png: rendered.png,
            fidelity: rendered.fidelity,
        })
    }

    /// Render plus exact coverage accounting (see [`Fidelity`]).
    pub fn render(&mut self, frame: &Frame) -> Result<Rendered, RenderError> {
        frame
            .validate()
            .map_err(|e| RenderError(format!("refusing to render: {e}")))?;
        let profile = &self.profile;
        let u = profile.scale;
        let cell_w = profile.cell_w * u;
        let cell_h = profile.cell_h * u;
        let pad = profile.pad * u;
        let u_i = u as i32;

        let w = frame.cols as u32 * cell_w + pad * 2;
        let h = frame.rows as u32 * cell_h + pad * 2;
        let bg = profile.default_bg;
        let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([bg.r, bg.g, bg.b]));
        let mut missing: Vec<MissingGlyph> = Vec::new();
        let mut fallback_glyphs: Vec<FallbackGlyph> = Vec::new();

        for y in 0..frame.rows {
            for x in 0..frame.cols {
                let Some(cell) = frame.get(x, y) else {
                    continue;
                };
                if cell.continuation {
                    continue;
                }
                let (fg, cbg) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
                let span = u32::from(cell.width.max(1)) * cell_w;
                let cx = pad + x as u32 * cell_w;
                let cy = pad + y as u32 * cell_h;
                if cbg != profile.default_bg {
                    fill_rect(&mut img, cx, cy, span, cell_h, cbg);
                }
                if cell.mods.hidden {
                    continue;
                }
                let baseline = cy as i32 + self.set.regular.ascent.round() as i32;
                // Whitespace cells carry no glyph, but real terminals still
                // draw underline/strikethrough across them (the background is
                // already painted above) — decorations are not part of the
                // skipped glyph draw.
                if !cell.symbol.trim().is_empty() {
                    draw_symbol(
                        &mut img,
                        &self.set,
                        &mut self.glyphs,
                        &cell.symbol,
                        cx as i32,
                        baseline,
                        span,
                        cy as i32,
                        cell_h,
                        fg,
                        cell.mods.bold,
                        cell.mods.italic,
                        u_i,
                        Some(CellSinks {
                            x,
                            y,
                            missing: &mut missing,
                            fallback: &mut fallback_glyphs,
                        }),
                    );
                }
                if cell.mods.underline {
                    let uy = (baseline + 2 * u_i).min((cy + cell_h - 1) as i32);
                    let th = if cell.mods.bold { 2 * u } else { u };
                    for t in 0..th {
                        for dx in 0..span {
                            blend(&mut img, cx + dx, (uy + t as i32) as u32, fg, 255);
                        }
                    }
                }
                if cell.mods.strikethrough {
                    let sy = baseline - (self.set.regular.ascent * 0.35) as i32;
                    for t in 0..u {
                        for dx in 0..span {
                            blend(&mut img, cx + dx, (sy + t as i32).max(0) as u32, fg, 255);
                        }
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
                let span = u32::from(cell.width.max(1)) * cell_w;
                let px = pad + cx as u32 * cell_w;
                let py = pad + frame.cursor.y as u32 * cell_h;
                let style = frame.cursor.style;
                match style {
                    crate::frame::CursorStyle::Block => {
                        fill_rect(&mut img, px, py, span, cell_h, fg);
                        if !cell.mods.hidden && !cell.symbol.trim().is_empty() {
                            let baseline = py as i32 + self.set.regular.ascent.round() as i32;
                            draw_symbol(
                                &mut img,
                                &self.set,
                                &mut self.glyphs,
                                &cell.symbol,
                                px as i32,
                                baseline,
                                span,
                                py as i32,
                                cell_h,
                                cbg,
                                false,
                                false,
                                u_i,
                                None,
                            );
                        }
                    }
                    crate::frame::CursorStyle::Underline => {
                        let uy = (py + cell_h - 2 * u) as i32;
                        for t in 0..2 * u {
                            for dx in 0..span {
                                blend(&mut img, px + dx, (uy + t as i32) as u32, fg, 255);
                            }
                        }
                    }
                    crate::frame::CursorStyle::Bar => {
                        for dx in 0..2 * u {
                            for dy in 0..cell_h {
                                blend(&mut img, px + dx, py + dy, fg, 255);
                            }
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| RenderError(format!("PNG encode: {e}")))?;
        let fidelity = Fidelity {
            profile: profile.name.clone(),
            font_sha256: profile.font_sha256.clone(),
            font_desc: profile.font_desc.clone(),
            scale: u,
            approximate: !missing.is_empty() || !self.set.fell_back.is_empty(),
            faces_fell_back: self.set.fell_back.iter().map(|s| s.to_string()).collect(),
            missing,
            fallback_glyphs,
        };
        Ok(Rendered { png: out, fidelity })
    }
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

/// Deterministic tofu box for missing glyphs (spans `span_px` wide). `u` is
/// the scale unit: insets are one unscaled pixel.
fn draw_tofu(dst: &mut image::RgbImage, x0: i32, top: i32, span_px: u32, h: u32, fg: Rgb, u: i32) {
    let w = (span_px as i32 - 2 * u).max(3 * u);
    for dx in 0..w {
        blend(dst, (x0 + u + dx).max(0) as u32, top.max(0) as u32, fg, 255);
        blend(
            dst,
            (x0 + u + dx).max(0) as u32,
            (top + h as i32 - u).max(0) as u32,
            fg,
            255,
        );
    }
    for dy in 0..h as i32 {
        blend(
            dst,
            (x0 + u).max(0) as u32,
            (top + dy).max(0) as u32,
            fg,
            255,
        );
        blend(
            dst,
            (x0 + u + w - u).max(0) as u32,
            (top + dy).max(0) as u32,
            fg,
            255,
        );
    }
}

/// Per-cell fidelity sinks threaded through [`draw_symbol`] (the cursor
/// redraw passes `None`: it re-renders a cell already accounted for).
struct CellSinks<'a> {
    x: u16,
    y: u16,
    missing: &'a mut Vec<MissingGlyph>,
    fallback: &'a mut Vec<FallbackGlyph>,
}

/// Default_Ignorable codepoints (variation selectors, ZWJ, …) carry no ink
/// of their own. They must not count as uncovered: a cell like `☕`+U+FE0F
/// would otherwise draw tofu on top of a real glyph (cmap-only coverage
/// treated the selector as a miss). Combining marks are NOT ignorable and
/// still overlay at the same origin.
fn is_default_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Rasterize `c` from `face` once (negatives cached). Coverage is **ink**,
/// not cmap index: an empty outline (Nerd-Font placeholder, failed CFF,
/// zero bitmap) does not cover, so the chain can try the next face.
fn cached_raster<'a>(
    cache: &'a mut GlyphCache,
    face: &LoadedFont,
    idx: FaceIdx,
    c: char,
) -> Option<&'a (fontdue::Metrics, Vec<u8>)> {
    cache
        .entry(GlyphKey { ch: c, face: idx })
        .or_insert_with(|| {
            if face.font.lookup_glyph_index(c) == 0 {
                return None;
            }
            let (m, bmp) = face.font.rasterize(c, face.px);
            if m.width == 0 || m.height == 0 || bmp.iter().all(|&p| p == 0) {
                None
            } else {
                Some((m, bmp))
            }
        })
        .as_ref()
}

/// Draw one lead-cell symbol at pen origin. Combining scalars overlay at the
/// same origin (documented approximation of terminal combining behavior).
/// Face chain: styled face → regular face → fallback faces in chain order →
/// tofu (recorded in `sinks.missing`). A face covers a codepoint only when
/// it produces a non-empty bitmap — cmap-only hits with empty outlines fall
/// through (otherwise Nerd-Font placeholders / un-rasterizable CFF would
/// block Noto). Faux styles apply only when the regular face serves a cell
/// whose mods asked for a styled face. Fallback glyphs draw in their face's
/// own weight, centered horizontally in the cell span and clipped to the
/// cell rect (fallback faces have their own metrics; the primary cell grid
/// never moves). Rasters come from `cache` (per `(char, face)`, negatives
/// included) instead of re-rasterizing per cell.
#[allow(clippy::too_many_arguments)]
fn draw_symbol(
    dst: &mut image::RgbImage,
    set: &FontSet,
    cache: &mut GlyphCache,
    symbol: &str,
    pen_x: i32,
    baseline: i32,
    span_px: u32,
    cell_top: i32,
    cell_h: u32,
    fg: Rgb,
    bold: bool,
    italic: bool,
    u: i32,
    mut sinks: Option<CellSinks<'_>>,
) {
    let styled = set.styled(bold, italic);
    let styled_idx = face_idx(bold, italic);
    let mut uncovered: Vec<char> = Vec::new();
    let mut served: Vec<(char, u8)> = Vec::new();
    for c in symbol.chars() {
        if is_default_ignorable(c) {
            continue;
        }
        // Face chain: styled → regular → fallbacks in order → missing.
        // Coverage = non-empty raster, not lookup_glyph_index != 0.
        enum Pick {
            Styled,
            Regular,
            Fallback(usize),
            Missing,
        }
        let pick = if cached_raster(cache, styled, styled_idx, c).is_some() {
            Pick::Styled
        } else if cached_raster(cache, &set.regular, FaceIdx::Regular, c).is_some() {
            Pick::Regular
        } else if let Some(fi) = (0..set.fallbacks.len()).find(|&fi| {
            cached_raster(cache, &set.fallbacks[fi], FaceIdx::Fallback(fi as u8), c).is_some()
        }) {
            Pick::Fallback(fi)
        } else {
            Pick::Missing
        };
        match pick {
            Pick::Styled => {
                draw_primary(
                    cache, dst, styled, styled_idx, c, pen_x, baseline, fg, false, false, u,
                );
            }
            Pick::Regular => {
                draw_primary(
                    cache,
                    dst,
                    &set.regular,
                    FaceIdx::Regular,
                    c,
                    pen_x,
                    baseline,
                    fg,
                    bold,
                    italic,
                    u,
                );
            }
            Pick::Missing => {
                uncovered.push(c);
            }
            Pick::Fallback(fi) => {
                let idx = FaceIdx::Fallback(fi as u8);
                let Some((m, bmp)) = cached_raster(cache, &set.fallbacks[fi], idx, c).cloned()
                else {
                    uncovered.push(c);
                    continue;
                };
                // Center the glyph's advance box in the cell span; clip ink
                // to the cell rect so fallback metrics never bleed into
                // neighboring cells.
                let origin_x = pen_x + ((span_px as f32 - m.advance_width) / 2.0).round() as i32;
                let top = baseline - (m.ymin + m.height as i32);
                let mut inked = false;
                for (i, &cov) in bmp.iter().enumerate() {
                    if cov == 0 {
                        continue;
                    }
                    let bx = (i % m.width) as i32;
                    let by = (i / m.width) as i32;
                    let dx = origin_x + m.xmin + bx;
                    let dy = top + by;
                    if dx < pen_x
                        || dx >= pen_x + span_px as i32
                        || dy < cell_top
                        || dy >= cell_top + cell_h as i32
                    {
                        continue;
                    }
                    inked = true;
                    blend(dst, dx as u32, dy as u32, fg, cov);
                }
                if inked {
                    served.push((c, fi as u8));
                } else {
                    // Raster existed but every pixel sat outside the cell
                    // box: still a miss, not a silent blank.
                    uncovered.push(c);
                }
            }
        }
    }
    if !served.is_empty() {
        if let Some(s) = sinks.as_mut() {
            let mut faces: Vec<String> = Vec::new();
            for (_, fi) in &served {
                let desc = set.fallbacks[usize::from(*fi)].desc.clone();
                if !faces.contains(&desc) {
                    faces.push(desc);
                }
            }
            s.fallback.push(FallbackGlyph {
                x: s.x,
                y: s.y,
                symbol: symbol.to_string(),
                codepoints: served
                    .iter()
                    .map(|(c, _)| format!("U+{:04X}", *c as u32))
                    .collect(),
                faces,
            });
        }
    }
    if uncovered.is_empty() {
        return;
    }
    draw_tofu(
        dst,
        pen_x,
        cell_top + 2 * u,
        span_px,
        cell_h.saturating_sub(4 * u as u32),
        fg,
        u,
    );
    if let Some(s) = sinks {
        s.missing.push(MissingGlyph {
            x: s.x,
            y: s.y,
            symbol: symbol.to_string(),
            codepoints: uncovered
                .iter()
                .map(|c| format!("U+{:04X}", *c as u32))
                .collect(),
        });
    }
}

/// Draw one glyph from the primary family (styled or regular face, with the
/// faux double-strike / shear when the regular face serves a styled cell).
/// This path is byte-stable: fallback-chain changes never touch it.
#[allow(clippy::too_many_arguments)]
fn draw_primary(
    cache: &mut GlyphCache,
    dst: &mut image::RgbImage,
    face: &LoadedFont,
    idx: FaceIdx,
    c: char,
    pen_x: i32,
    baseline: i32,
    fg: Rgb,
    faux_bold: bool,
    faux_italic: bool,
    u: i32,
) {
    let Some((m, bmp)) = cached_raster(cache, face, idx, c) else {
        return;
    };
    // ymin = offset of the bitmap's BOTTOM edge from the baseline, so the
    // top edge sits at baseline - (ymin + height).
    let top = baseline - (m.ymin + m.height as i32);
    for (i, &cov) in bmp.iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let bx = (i % m.width) as i32;
        let by = (i / m.width) as i32;
        // Faux italic: shear top rows right (fallback only).
        let shear = if faux_italic {
            ((m.height as i32 - 1 - by) as f32 * 0.15) as i32
        } else {
            0
        };
        let dx = pen_x + m.xmin + bx + shear;
        let dy = top + by;
        if dx >= 0 && dy >= 0 {
            blend(dst, dx as u32, dy as u32, fg, cov);
            // Faux bold: double-strike one unscaled pixel right.
            if faux_bold {
                blend(dst, (dx + u) as u32, dy as u32, fg, cov);
            }
        }
    }
}

/// Render a validated frame to PNG bytes under `profile`.
///
/// One-shot convenience: constructs a fresh [`Renderer`] per call (8 font
/// parses, cold glyph cache). Bulk gates should keep a `Renderer` instead.
pub fn render_png(
    frame: &Frame,
    profile: &Profile,
    faces: &FontFaces<'_>,
) -> Result<Vec<u8>, RenderError> {
    Renderer::new(profile, faces)?.render_png(frame)
}

/// Render plus exact coverage accounting (see [`Fidelity`]).
///
/// One-shot convenience: constructs a fresh [`Renderer`] per call (8 font
/// parses, cold glyph cache). Bulk gates should keep a `Renderer` instead.
pub fn render_png_report(
    frame: &Frame,
    profile: &Profile,
    faces: &FontFaces<'_>,
) -> Result<Rendered, RenderError> {
    Renderer::new(profile, faces)?.render(frame)
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
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" font-family=\"'JetBrainsMono Nerd Font Mono','JetBrains Mono',monospace\" font-size=\"{}\">\n<rect width=\"100%\" height=\"100%\" fill=\"{bg}\"/>\n",
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
                if c.mods.hidden {
                    run.push_str(&" ".repeat(usize::from(c.width.max(1))));
                } else {
                    run.push_str(&c.symbol);
                }
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
            // text-decoration paints across the whole run, spaces included —
            // the same contract the PNG path follows for whitespace cells.
            let mut deco = Vec::new();
            if cell.mods.underline {
                deco.push("underline");
            }
            if cell.mods.strikethrough {
                deco.push("line-through");
            }
            let decoration = if deco.is_empty() {
                String::new()
            } else {
                format!(" text-decoration=\"{}\"", deco.join(" "))
            };
            let attrs = format!("{weight}{style}{decoration}");
            s.push_str(&format!(
                "<text xml:space=\"preserve\" x=\"{px}\" y=\"{}\" fill=\"{}\"{}>{}</text>\n",
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

/// Build the standalone HTML document for a frame from an already-rendered
/// PNG (shared by [`Renderer::render_html`] and [`Renderer::render_artifacts`]
/// so the PNG is rasterized once). See [`Renderer::render_html`] for the
/// determinism contract of the embedded frame JSON.
fn html_document(frame: &Frame, profile: &Profile, title: &str, png: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let (png_w, png_h) = profile.image_size(frame.cols, frame.rows);
    // SVG is a selectable overlay only: its fills are forced transparent so
    // viewer fonts cannot tofu-over the PNG. Copy/select still works.
    let svg = render_svg(frame, profile);
    let mut embedded = frame.clone();
    embedded.provenance.created_unix = 0;
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{background:#141414;margin:24px}}.shot{{position:relative;display:inline-block;line-height:0}}.shot>img{{display:block;image-rendering:pixelated}}.shot>svg{{position:absolute;inset:0;width:100%;height:100%}}.shot>svg rect,.shot>svg text{{fill:transparent!important}}</style></head><body><div class=\"shot\"><img src=\"data:image/png;base64,{b64}\" alt=\"{}\" width=\"{png_w}\" height=\"{png_h}\">{svg}</div><script type=\"application/json\">{}</script></body></html>",
        esc_xml(title),
        esc_xml(title),
        crate::snapshot::json_for_script(&embedded.to_json())
    )
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
    if c.mods.hidden {
        p.push("8".into());
    }
    if c.mods.blink {
        p.push("5".into());
    }
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
