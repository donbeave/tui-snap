# Fonts: licensing and coverage

## Vendored asset

`DejaVuSansMNerdFontMono-Regular.ttf` (2.7 MB) — DejaVu Sans Mono patched by
Nerd Fonts v3.5.1 (`DejaVuSansMono.tar.xz`).

- Upstream patch project: <https://github.com/ryanoasis/nerd-fonts>
- Original typeface: <https://github.com/dejavu-fonts/dejavu-fonts>

## License (why vendoring is allowed)

The Nerd patch **retains the upstream Bitstream Vera license** (it is NOT
SIL OFL for this family). Bitstream Vera permits reproduction and
redistribution in larger software packages provided the copyright and
license text ship alongside — hence `LICENSE-DejaVuSansMono.txt` in this
directory (Fonts are © Bitstream; DejaVu changes are public domain; Arev
glyphs © Tavmjong Bah). Do not sell the font standalone; do not use the
names "Bitstream" or "Vera" for modified versions.

The SHA-256 of the vendored bytes is pinned in code (`Profile::font_sha256`,
asserted by `tests/render.rs::font_hash_pinned_and_documented`). Any font
change fails gates loudly instead of shifting pixels silently.

## Coverage reality

| Class | Status |
|---|---|
| ASCII, Latin, punctuation | Full |
| Box drawing, blocks, Braille | Full |
| Nerd-Font icons (PUA) | Full (Mono variant: every cell 1-wide) |
| Combining marks | Rendered overlaid at the same origin (documented approximation) |
| CJK ideographs | NOT covered → deterministic tofu box, correct 2-cell advance via `unicode-width` |
| Color emoji | NOT covered → tofu; geometry per `unicode-width` |

Missing glyphs are detected via `lookup_glyph_index == 0` and drawn as an
outline box; wide codepoints keep their 2-cell span, so terminal geometry
stays correct even when the glyph is absent. `--font-file` overrides the
embedded font (its hash is recorded in reports instead).
