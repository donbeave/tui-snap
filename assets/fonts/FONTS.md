# Fonts: licensing and coverage

## Vendored assets (default family)

JetBrainsMono **Nerd Font Mono** — four faces, each ~2.5 MB:

- `JetBrainsMonoNerdFontMono-Regular.ttf` — geometry + hash authority
- `JetBrainsMonoNerdFontMono-Bold.ttf`
- `JetBrainsMonoNerdFontMono-Italic.ttf`
- `JetBrainsMonoNerdFontMono-BoldItalic.ttf`

- Original typeface: <https://www.jetbrains.com/lp/mono/>
- Patch project: <https://github.com/ryanoasis/nerd-fonts> (Mono variant:
  every glyph cell 1-wide)

`DejaVuSansMNerdFontMono-Regular.ttf` (2.7 MB) is kept in place for
reference/override but is no longer the default profile font.

## Vendored fallback faces (per-glyph chain)

Glyphs the primary family lacks are served per-glyph by pinned Noto subsets
(`VENDORED_FALLBACK_FACES` in `src/profile.rs`), tried in this order after
the primary styled + regular faces:

| File | Source | Subset covers | Size |
|---|---|---|---|
| `NotoSansSymbols2-subset.ttf` | Noto Sans Symbols 2 Regular | U+25A0–25FF Geometric Shapes (◐ ● ◆), U+2600–26FF Misc Symbols (★ ☕ ⚠), U+2700–27BF Dingbats (❤ ✔), U+2B00–2BFF Misc Symbols & Arrows (⬤ ⭐) | 87 KB |
| `NotoSansSymbols-subset.ttf` | Noto Sans Symbols (variable → wght 400 instance) | U+2600–26FF misc symbols unique to v1 (⚷ ⚙ ♻ ☰-class) — v1/v2 are disjoint in this block (probed) | 27 KB |
| `NotoSansCJKjp-subset.otf` | Noto Sans CJK JP Regular | U+3000–30FF (CJK punctuation, hiragana, katakana), U+FF01–FF65 (fullwidth forms), JIS X 0208 level-1 kanji (2965, incl. 東京日本語中文) | 679 KB |

Each face's SHA-256 is pinned in code (`VENDORED_*_FONT_SHA256`) and verified
when a `Renderer` loads the chain; a mismatch refuses to render. Fallback
faces draw single glyphs in their own weight, centered and clipped inside
the cell box the primary geometry pins — they never move the cell grid, and
primary-covered frames render byte-identical with or without the chain
(pinned by `tests/render.rs`).

Upstream sources are commit-pinned and sha256-verified in
`tools/subset_fonts.py`:

- `google/fonts@8b0a1d0f` `ofl/notosanssymbols/NotoSansSymbols[wght].ttf`
- `google/fonts@7b6724ac` `ofl/notosanssymbols2/NotoSansSymbols2-Regular.ttf`
- `notofonts/noto-cjk@165c01b4` `Sans/OTF/Japanese/NotoSansCJKjp-Regular.otf`

### Rebuilding / extending the subsets

Needs `fonttools` (`pip install --user fonttools`). The script downloads the
pinned upstreams, instantiates the Symbols variable font at wght=400,
subsets with pyftsubset, asserts required codepoints survived, and prints
the output hashes:

```text
python3 tools/subset_fonts.py           # rebuild assets/fonts/*-subset.*
python3 tools/subset_fonts.py --check   # verify committed subsets (offline)
```

The exact pyftsubset invocation (same flags for all three):

```text
pyftsubset <upstream> --output-file=<subset> --unicodes-file=<ranges> \
  --no-hinting --name-IDs=0,1,2,4,6,13,14 --name-legacy --name-languages=* \
  --no-layout-closure --drop-tables+=GSUB,GPOS
```

To extend coverage (JIS X 0208 level-2 kanji, Hangul, more symbol blocks):
widen the ranges in `tools/subset_fonts.py` (`SYMBOLS1_RANGES`,
`SYMBOLS2_RANGES`, `cjk_unicodes()`), rerun, then update the
`VENDORED_*_FONT_SHA256` pins in `src/profile.rs`, the literal pins in
`tests/render.rs`, and the coverage table below. Mind repo size: a full
Noto Sans CJK face is ~16 MB — keep subsets to a few MB max.

## License (why vendoring is allowed)

JetBrains Mono is licensed under the **SIL Open Font License 1.1**, and the
Nerd Fonts patch re-releases under the same OFL. OFL permits redistribution
with software provided the license text ships alongside — hence
`LICENSE-JetBrainsMono.txt` in this directory (the canonical OFL 1.1 text,
<https://openfontlicense.org>). Do not sell the font standalone; modified
versions must not use a Reserved Font Name.

The Noto faces (Symbols, Symbols 2, CJK JP) are likewise **SIL OFL 1.1** —
see `LICENSE-Noto.txt` (Copyright The Noto Project Authors). Subsetting an
OFL font for embedding is permitted; the subsets keep the upstream copyright
and license name entries.

The DejaVu file remains under the Bitstream Vera license
(`LICENSE-DejaVuSansMono.txt`): redistribution allowed with the license
text; do not use the names "Bitstream" or "Vera" for modified versions.

The SHA-256 of the vendored regular face is pinned in code
(`Profile::font_sha256`, asserted by
`tests/render.rs::font_hash_pinned_and_documented`), and each fallback
face's SHA-256 is pinned in `src/profile.rs` (asserted by
`tests/render.rs::vendored_fallback_hashes_pinned_and_documented`). Any font
change fails gates loudly instead of shifting pixels silently.

## Cell metrics (measured with fontdue, pinned in `Profile`)

At `font_px = 16`: advance(`M`) = 9.60 px → `cell_w = 10`; line height
(ascent 16.32 + descent 4.80) = 21.12 px → `cell_h = 21`. All four primary
faces measure identically. Glyphs rasterize at `16 × scale` directly onto
the final image (HiDPI, no post upscale). JetBrains Mono's line box is
taller than DejaVu's (1.32 em vs 1.19 em), matching what real terminals give
this family — the pins moved 10×19 → 10×21 with the family switch. Fallback
faces are rasterized at the same pixel size but do NOT share these metrics:
their glyphs are centered horizontally in the cell span and clipped to the
cell rect.

## Face selection

`cell.mods` selects the primary face: bold → Bold, italic → Italic, both →
BoldItalic. Per-glyph chain: styled face → regular face → fallback faces in
chain order → tofu. The faux double-strike / shear survive only when a
non-regular face fails to parse (recorded as `faces_fell_back` in the
fidelity sidecar) or `--font-file` overrides with a single face. Fallback
faces render in their own regular weight regardless of `cell.mods`.

## Coverage reality

| Class | Status |
|---|---|
| ASCII, Latin, punctuation | Full (primary family) |
| Box drawing, blocks, Braille | Full (primary family) |
| Powerline symbols, Nerd-Font icons (PUA) | Full (primary family) |
| Combining marks | Rendered overlaid at the same origin (documented approximation) |
| Geometric Shapes ◐, Misc Symbols ★ ☕, Dingbats ❤ ✔ | Fallback: Noto Sans Symbols 2 subset (recorded in `fallback_glyphs`) |
| Misc symbols unique to v1: ⚷ (U+26B7), ⚙, ♻ | Fallback: Noto Sans Symbols subset |
| Kana, JIS X 0208 level-1 kanji (東京…), fullwidth forms | Fallback: Noto Sans CJK JP subset, correct 2-cell advance via `unicode-width` |
| Hangul, JIS X 0208 level-2 kanji | NOT covered → deterministic tofu box + fidelity report (subset extensible, see above) |
| Color emoji (e.g. U+1F980 🦀) | NOT covered → tofu + fidelity report |

Missing glyphs are detected via `lookup_glyph_index == 0` across the whole
face chain and drawn as an outline box; every miss is listed with position
and `U+XXXX` codepoints in `<name>.png.fidelity.json` next to each PNG
output (`render --format png` and store `actual/`/`approved/` pairs) —
exact reporting, never silent tofu. Cells served by a fallback face are
listed in the same sidecar under `fallback_glyphs` (the field is omitted
when empty). `approximate: true` marks any render with missing glyphs or
fell-back faces. `--font-file` overrides the embedded family (single face;
its hash is recorded in reports instead; the fallback chain still applies).
