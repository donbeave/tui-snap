# Qualification fork: schema 3

Canonical schema 3 adds `Mods.hidden` and `Mods.blink`; version-2 imports fail
instead of guessing lost attributes. Blink presence survives, but blink phase
is frozen visible and slow/rapid rates remain combined. HIDDEN suppresses
PNG/SVG glyph paint, while canonical JSON retains the original symbol: hidden
text is not redacted. Never capture real secrets.

`python3 tools/migrate_fixture_v3.py --out DIR` exports only the audited
schema-2 approvals read directly from Git revision
`5036cf87e621e6beb66deffe3224abdbefc955cb`, tied to the exact fixture source hash.
Run from a checkout containing that revision; current schema-3 approvals and
actual captures are never read. `--continuation-styles` additionally repairs old continuation
colors/modifiers from each original wide lead; its ledger lists these changes
separately. Outputs require review before replacing approved files. This is
not a general migration of arbitrary schema-2 snapshots: unknown hidden/blink
state cannot be recovered from that format.

The vendored vt100 0.16.2 source fixes independent bold/DIM flags, DECAWM,
wide continuation attributes, and hidden/blink/strike support. Archive and
original per-file hashes are in `vendor/UPSTREAM.json`; full licensed source
is included so a clean checkout builds without a developer-machine patch.
The unmodified termlens 0.9 library is also vendored, with its vt100 dependency
wired to that same local engine. Both are direct path dependencies: Git/path
consumers need no `[patch.crates-io]` workaround. Original source hashes and
licenses are retained in `vendor/TERMLENS-UPSTREAM.json` and `vendor/termlens`.
Code using `frame_from_screen` should construct its screen through the
`tuisnap::termlens` re-export so the engine's Rust type identity matches.
Qualify ordinary consumption with `cargo run --locked --manifest-path
tests/fixtures/consumer/Cargo.toml` and inspect `cargo tree -i vt100` in that
consumer. A future crates.io release must publish the forked engine packages
under distinct names before replacing these Git/path dependencies; this PR
does not silently fall back to the defective registry engine.

`Session::paste` retains termlens's simulated terminal behavior (LF→CR plus
paste-marker sanitization). `Session::paste_literal` preserves literal line
breaks, requires bracketed-paste mode, and rejects embedded delimiters so
payload text cannot escape into ordinary input. This separates deterministic
payload testing from simulated terminal behavior.

Raw-ANSI replay still cannot observe cursor appearance; its block/steady
cursor is a limitation, not equivalence to the PTY path. Use PTY captures for
cursor shape/blinking assertions. Raster output remains a pinned approximation
of terminal font rendering, not pixel identity with a terminal emulator.

---

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
