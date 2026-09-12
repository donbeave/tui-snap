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

## License (why vendoring is allowed)

JetBrains Mono is licensed under the **SIL Open Font License 1.1**, and the
Nerd Fonts patch re-releases under the same OFL. OFL permits redistribution
with software provided the license text ships alongside — hence
`LICENSE-JetBrainsMono.txt` in this directory (the canonical OFL 1.1 text,
<https://openfontlicense.org>). Do not sell the font standalone; modified
versions must not use a Reserved Font Name.

The DejaVu file remains under the Bitstream Vera license
(`LICENSE-DejaVuSansMono.txt`): redistribution allowed with the license
text; do not use the names "Bitstream" or "Vera" for modified versions.

The SHA-256 of the vendored regular face is pinned in code
(`Profile::font_sha256`, asserted by
`tests/render.rs::font_hash_pinned_and_documented`). Any font change fails
gates loudly instead of shifting pixels silently.

## Cell metrics (measured with fontdue, pinned in `Profile`)

At `font_px = 16`: advance(`M`) = 9.60 px → `cell_w = 10`; line height
(ascent 16.32 + descent 4.80) = 21.12 px → `cell_h = 21`. All four faces
measure identically. Glyphs rasterize at `16 × scale` directly onto the
final image (HiDPI, no post upscale). JetBrains Mono's line box is taller
than DejaVu's (1.32 em vs 1.19 em), matching what real terminals give this
family — the pins moved 10×19 → 10×21 with the family switch.

## Face selection

`cell.mods` selects the face: bold → Bold, italic → Italic, both →
BoldItalic. Per-glyph chain: styled face → regular face → tofu. The faux
double-strike / shear survive only when a non-regular face fails to parse
(recorded as `faces_fell_back` in the fidelity sidecar) or `--font-file`
overrides with a single face.

## Coverage reality

| Class | Status |
|---|---|
| ASCII, Latin, punctuation | Full |
| Box drawing, blocks, Braille | Full |
| Powerline symbols, Nerd-Font icons (PUA) | Full |
| Combining marks | Rendered overlaid at the same origin (documented approximation) |
| CJK ideographs | NOT covered → deterministic tofu box + fidelity report, correct 2-cell advance via `unicode-width` |
| Color emoji (e.g. U+1F980 🦀) | NOT covered → tofu + fidelity report |
| **U+26B7 (⚷)** | **NOT covered** (probed: `lookup_glyph_index == 0` in all four faces) → tofu + fidelity report |

Missing glyphs are detected via `lookup_glyph_index == 0` across the face
chain and drawn as an outline box; every miss is listed with position and
`U+XXXX` codepoints in `<name>.png.fidelity.json` next to each PNG output
(`render --format png` and store `actual/`/`approved/` pairs) — exact
reporting, never silent tofu. `approximate: true` marks any render with
missing glyphs or fell-back faces. `--font-file` overrides the embedded
family (single face; its hash is recorded in reports instead).
