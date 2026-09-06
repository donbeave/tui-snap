# Migration: v0.1 → v0.2

v0.2 is a redesign implementing `RESEARCH.md`. Breaking changes are
intentional; the old APIs are gone, not deprecated.

## Removed (no shims)

| v0.1 | v0.2 |
|---|---|
| `tuisnap::ratatui_shot::{widget_frame, draw_frame}` | `tuisnap::ratatui::{widget_frame, draw_frame, capture}` — new signatures take `Provenance`; `widget_frame` hides the cursor, `draw_frame` preserves it |
| `tuisnap::Baseline` + `BLESS=1` / `UPDATE_SNAPSHOT=1` | `tuisnap::snapshot::Store` + explicit `tuisnap accept`. **Rationale:** ambient approval violates "CI must never auto-bless" (a test proves no env var accepts) |
| `digest` CLI subcommand | `check` / `report` subcommands |
| `render --input *.ansi` | `render` reads canonical `frame.json` only; raw streams replay via `ansi::replay_raw` (feature `pty`) |
| `tuisnap::render::write_format`, `to_png_bytes`, block-glyph PNG | `render_png` (real fontdue glyphs), `render_svg`, `ansi_dump`; formats via CLI `--format` |
| Hand-rolled SGR replay parser | Deleted; `vt100` is the established emulator |
| `PtySession` / `run_once(argv, opts, sends)` | `pty::Session` (termlens engine) / `run_once(argv, opts, sends, settle)`; waits now fail on timeout instead of returning `false` |

## Schema

- `Frame` is now versioned (`version: 2`), with positioned cells, explicit
  widths/continuations, `Default|Indexed|Rgb` colors, six modifiers,
  cursor state, and provenance. v0.1 JSON does not import — re-capture.
- `Frame::digest` now mixes the schema version and the cursor. Old digests
  are incomparable by design.

## Workflow

```text
# v0.1
UPDATE_BASELINE=1 cargo test   # ambient bless

# v0.2
cargo test                     # writes actuals, fails missing/changed
cargo run -q -- accept --store <dir> --all   # explicit, after review
cargo test                     # green
```

## Dependencies

- Added (all latest, pure-cargo): `fontdue`, `image-compare`, `base64`,
  `sha2`, `termlens` (optional via `pty` feature), `vt100` (optional).
- Removed direct use of `portable-pty` (termlens owns PTY lifetime now).
- Pure view tests: `cargo build/test --no-default-features` excludes
  `termlens` + `vt100` entirely.
