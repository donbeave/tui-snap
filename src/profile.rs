//! Pinned rendering profile + vendored font assets.
//!
//! A [`Profile`] fixes everything that affects pixels: font bytes (pinned by
//! SHA-256, not by filename), size, cell geometry, palette defaults, image
//! scale, padding, and cursor policy. Reports record the profile name and the
//! font hash, so a renderer change is distinguishable from an app regression.
//!
//! ## Font licensing
//!
//! The default family is **JetBrainsMono Nerd Font Mono** (Regular / Bold /
//! Italic / BoldItalic) under the **SIL Open Font License 1.1**:
//! redistribution in this repository is allowed provided the license text
//! ships alongside — see `assets/fonts/LICENSE-JetBrainsMono.txt` and
//! `assets/fonts/FONTS.md`. Upstream: <https://www.jetbrains.com/lp/mono/>,
//! patch project: <https://github.com/ryanoasis/nerd-fonts>.
//! `assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf` (Bitstream Vera
//! license, `LICENSE-DejaVuSansMono.txt`) is kept in place for reference but
//! is no longer the default.
//!
//! Coverage reality (documented, not hidden): the vendored family covers
//! ASCII, box drawing, block elements, Braille, and Nerd-Font icons. What it
//! does NOT cover is served by the vendored fallback chain
//! ([`VENDORED_FALLBACK_FACES`]): Noto Sans Symbols 2 / Noto Sans Symbols
//! subsets for symbol codepoints (★ ☕ ⚷ ◐ ❤ …) and a Noto Sans CJK JP subset
//! (kana, JIS X 0208 level-1 kanji incl. 東京, fullwidth forms). Glyphs no
//! face in the whole chain covers (color emoji, Hangul, JIS level-2 kanji)
//! still render as a deterministic tofu box AND are reported in the
//! `.png.fidelity.json` sidecar — never silently. Wide codepoints keep their
//! 2-cell advance via `unicode-width`, so geometry stays terminal-like even
//! when the glyph is absent. `--font-file` overrides the embedded family
//! (single face; faux styles; hash recorded; the fallback chain still
//! applies).

use sha2::{Digest, Sha256};

/// Vendored pinned font bytes (reproducible on any machine).
pub const VENDORED_FONT: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf");
pub const VENDORED_FONT_BOLD: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf");
pub const VENDORED_FONT_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf");
pub const VENDORED_FONT_BOLD_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf");

/// The four faces of one pinned monospace family, selected by `cell.mods`
/// (bold → Bold, italic → Italic, both → BoldItalic). The regular face pins
/// the geometry and the recorded hash; a non-regular face that fails to
/// parse falls back to regular with the faux double-strike / shear.
#[derive(Debug, Clone, Copy)]
pub struct FontFaces<'a> {
    pub regular: &'a [u8],
    pub bold: &'a [u8],
    pub italic: &'a [u8],
    pub bold_italic: &'a [u8],
}

impl<'a> FontFaces<'a> {
    /// Every slot = the same bytes (single-face override: faux styles).
    pub const fn single(bytes: &'a [u8]) -> Self {
        Self {
            regular: bytes,
            bold: bytes,
            italic: bytes,
            bold_italic: bytes,
        }
    }
}

/// The default vendored family (JetBrainsMono Nerd Font Mono).
pub const VENDORED_FACES: FontFaces<'static> = FontFaces {
    regular: VENDORED_FONT,
    bold: VENDORED_FONT_BOLD,
    italic: VENDORED_FONT_ITALIC,
    bold_italic: VENDORED_FONT_BOLD_ITALIC,
};

/// Vendored per-glyph fallback faces: Noto subsets covering what the primary
/// family lacks (see `assets/fonts/FONTS.md`; subsets reproducible via
/// `tools/subset_fonts.py`, SIL OFL 1.1, `LICENSE-Noto.txt`).
pub const VENDORED_SYMBOLS2_FONT: &[u8] =
    include_bytes!("../assets/fonts/NotoSansSymbols2-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS2_FONT`], pinned at load.
pub const VENDORED_SYMBOLS2_FONT_SHA256: &str =
    "e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5";
pub const VENDORED_SYMBOLS_FONT: &[u8] =
    include_bytes!("../assets/fonts/NotoSansSymbols-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS_FONT`], pinned at load.
pub const VENDORED_SYMBOLS_FONT_SHA256: &str =
    "6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a";
pub const VENDORED_CJK_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSansCJKjp-subset.otf");
/// SHA-256 of [`VENDORED_CJK_FONT`], pinned at load.
pub const VENDORED_CJK_FONT_SHA256: &str =
    "777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897";

/// One pinned fallback face: font bytes + expected SHA-256 + description.
/// The hash is verified when a [`crate::render::Renderer`] loads the chain; a
/// mismatch refuses to render (explicit, never silent). Fallback faces draw
/// in their own regular weight regardless of `cell.mods`, centered and
/// clipped inside the cell box the primary geometry pins.
#[derive(Debug, Clone, Copy)]
pub struct FallbackFace<'a> {
    pub bytes: &'a [u8],
    pub sha256: &'a str,
    pub desc: &'a str,
}

/// The default per-glyph fallback chain, tried in order after the primary
/// family: Noto Sans Symbols 2 (Geometric Shapes, Miscellaneous Symbols,
/// Dingbats, Miscellaneous Symbols and Arrows), Noto Sans Symbols (misc
/// symbols unique to v1, e.g. U+26B7 ⚷), Noto Sans CJK JP (kana, JIS X 0208
/// level-1 kanji, fullwidth forms). [`crate::render::Renderer::new`] loads
/// this chain; [`crate::render::Renderer::with_fallbacks`] replaces it.
pub const VENDORED_FALLBACK_FACES: &[FallbackFace<'static>] = &[
    FallbackFace {
        bytes: VENDORED_SYMBOLS2_FONT,
        sha256: VENDORED_SYMBOLS2_FONT_SHA256,
        desc: "vendored NotoSansSymbols2 subset (SIL OFL 1.1)",
    },
    FallbackFace {
        bytes: VENDORED_SYMBOLS_FONT,
        sha256: VENDORED_SYMBOLS_FONT_SHA256,
        desc: "vendored NotoSansSymbols subset (SIL OFL 1.1)",
    },
    FallbackFace {
        bytes: VENDORED_CJK_FONT,
        sha256: VENDORED_CJK_FONT_SHA256,
        desc: "vendored NotoSansCJKjp subset: kana, JIS X 0208 level-1 kanji, fullwidth forms (SIL OFL 1.1)",
    },
];

/// Rendering profile. [`Profile::default_profile`] is the reproducible gate.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    /// Pixels per Em for glyph rasterization (before `scale`).
    pub font_px: f32,
    /// Cell geometry in output pixels (before `scale`).
    pub cell_w: u32,
    pub cell_h: u32,
    pub pad: u32,
    /// Integer rasterization scale: glyphs are rasterized at
    /// `font_px * scale` straight onto the final image (HiDPI crispness, no
    /// post upscale).
    pub scale: u32,
    /// Terminal defaults.
    pub default_fg: crate::frame::Rgb,
    pub default_bg: crate::frame::Rgb,
    /// Font identity actually used (vendored or override).
    pub font_sha256: String,
    pub font_desc: String,
    /// Cursor policy: frozen-visible block cursor. Blink phase is ignored by
    /// design so reruns are deterministic.
    pub cursor_visible: bool,
}

impl Profile {
    /// The reproducible gate profile. Geometry is measured from the vendored
    /// font at init (see [`crate::render::measure`]) and then pinned here as
    /// constants so a font change fails loudly instead of shifting pixels.
    #[must_use]
    pub fn default_profile() -> Self {
        Self {
            name: "tuisnap-default".to_string(),
            font_px: 16.0,
            cell_w: 10,
            cell_h: 21,
            pad: 12,
            scale: 2,
            default_fg: crate::frame::Rgb::new(0xd0, 0xd0, 0xd0),
            default_bg: crate::frame::Rgb::new(0x00, 0x00, 0x00),
            font_sha256: font_sha256(VENDORED_FONT),
            font_desc: "vendored JetBrainsMonoNerdFontMono-Regular (SIL OFL 1.1)".to_string(),
            cursor_visible: true,
        }
    }

    #[must_use]
    pub fn with_font_file(mut self, desc: String, bytes: &[u8]) -> Self {
        self.font_sha256 = font_sha256(bytes);
        self.font_desc = desc;
        self
    }

    /// A reusable [`crate::render::Renderer`] pinned to this profile: faces
    /// parsed once, glyph rasters cached across frames. Bulk gates
    /// (`Store::check_with`/`Store::report_with`) should go through one of
    /// these per thread instead of the one-shot free functions.
    pub fn renderer(
        &self,
        faces: &FontFaces<'_>,
    ) -> Result<crate::render::Renderer, crate::render::RenderError> {
        crate::render::Renderer::new(self, faces)
    }

    /// Image dimensions for a `cols`×`rows` frame.
    #[must_use]
    pub fn image_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        (
            (cols as u32 * self.cell_w + self.pad * 2) * self.scale,
            (rows as u32 * self.cell_h + self.pad * 2) * self.scale,
        )
    }
}

#[must_use]
pub fn font_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
