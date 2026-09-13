# Usage

## The two paths

### 1. Pure view tests (default, no PTY)

Render the actual production view from fixture data. No business logic,
network, database, subprocesses, or PTY:

```rust
let frame = tuisnap::ratatui::draw_frame(120, 40, prov(), |f| {
    myapp::render_home(f, &fixture_model())
});
```

- `widget_frame(widget, cols, rows, prov)` — one widget; hardware cursor
  forced hidden so the gate never depends on backend defaults.
- `draw_frame(cols, rows, prov, draw)` — full screens, layouts, stateful
  widgets; cursor captured post-draw (cursor-only changes are gated).
- `capture(&mut terminal, prov)` — capture an existing `TestBackend`
  terminal you drove yourself.

Build and run view tests without the PTY engine:

```text
cargo test --no-default-features
```

### 2. Interactive tests (feature `pty`)

```rust
use std::time::Duration;
use tuisnap::pty::{PtyOptions, Session};

let opts = PtyOptions { cols: 120, rows: 40, ..Default::default() }
    .without_env("NO_COLOR")                    // strip inherited ambient vars
    .with_env("HOLLA_NO_HISTORY", "1");         // set app-specific ones
let mut s = Session::spawn(&["./my-tui".into(), "--flag".into()], &opts)?;
s.wait_for_text("Ready")?;                       // timeout FAILS, screen embedded
s.wait_until(|sc| sc.cursor() == (0, 4, true))?; // any predicate on the live screen
s.send_key("enter")?;                            // enter escape tab up down ...
s.send_key("ctrl-up")?;                          // ctrl/alt/shift chords over special keys
s.type_text("hello")?;
s.click(10, 5)?; s.click_with(tuisnap::pty::MouseButton::Right, 10, 5)?;
s.scroll(10, 5, tuisnap::pty::Scroll::Down)?;
s.drag(1, 1, 5, 5)?; s.resize(100, 30)?;         // axes bounded to 2..=1000
let frame = s.wait_stable(Duration::from_millis(300))?;  // content AND styles stable
```

`PtyOptions.env_remove` (or `without_env`) removes inherited variables from
the child (`std::process::Command::env_remove` semantics); `env` (or
`with_env`) sets entries after the TERM/COLORTERM/LINES/COLUMNS presets.
`MouseButton`/`MouseChord`/`Scroll`/`ScrollChord` are re-exported from
`tuisnap::pty`.

Scripted one-shot equivalent for the CLI (`type:`, `sleep:<ms>`,
`wait:<needle>`, key names):

```text
tuisnap run --cols 120 --rows 40 --send enter --send wait:Ready \
  --store shots --name home -- ./my-tui
```

Dropping a `Session` kills and reaps the child — cleanup is guaranteed even
when a test panics.

## The gate

```rust
let outcome = store.check("home", &frame, &profile, &VENDORED_FACES, 1.0)?;
outcome.ensure_matched()?;
```

Bulk gates should reuse one renderer per thread instead of paying 8 font
parses (4 primary + 3 fallback + the geometry probe) per check:

```rust
let mut renderer = profile.renderer(&VENDORED_FACES)?;  // faces parsed once
let outcome = store.check_with(&mut renderer, "home", &frame, 1.0)?;
```

(`tuisnap::render::Renderer` caches glyph rasters keyed by `(char, face)`,
negatives included. All methods take `&mut self`, so parallel cargo-test
threads each need their own — `thread_local!` is the convenient carrier.)

1. `actual/` artifacts are written FIRST (`<name>.frame.json` + `.png` +
   `.png.fidelity.json` missing-glyph sidecar).
2. Missing approval → `MissingApproval` (fail-closed: "new snapshot
   requires review"). Corrupt approval → `CorruptApproval` naming the file.
3. Cell comparison is exact (symbol, width, colors, all modifiers, cursor).
   Dimension mismatch is a status, never a silent diff.
4. Pixel comparison runs on **decoded** pixels (`image-compare` hybrid
   metric). Strict gates pass `1.0`; pass a lower `pixel_threshold`
   explicitly for review passes. Byte-identity of PNG files is never the
   gate (re-encoding must not fail it). An approved PNG missing from disk
   is regenerated in memory and STILL gates pixels — the outcome carries
   the image as `expected_png_bytes` so reports always show it.
5. On mismatch: `diff/<name>.png` (red overlay) + cell diagnostics (first
   100 of N) + `report.html`, then the assertion fails with paths and the
   exact accept command.

## Reports from Rust (no CLI)

`tuisnap report --store DIR` is a thin wrapper over library calls a Rust
suite can make directly:

```rust
// Re-verify every actual/*.frame.json, rewrite report.html:
let report = store.report(&profile, &VENDORED_FACES, 1.0, "my suite")?;
assert_eq!(report.failed(), 0, "{:?}", report.outcomes);
// …or through a cached renderer: store.report_with(&mut renderer, 1.0, "t")?;
// Single outcome → one report row (base64-embedded PNGs):
let entry = store.report_entry(&outcome, &profile)?;
tuisnap::snapshot::write_report(&store, "title", &[entry])?;
```

## Approval workflow (local only)

```text
cargo test                                  # writes actuals, reports mismatch
# open tests/visual/report.html, review actual vs expected vs diff
cargo run -q -- accept --store tests/visual --name home
cargo test                                  # green
```

`accept --all` approves every actual in the store after you reviewed the
report. Acceptance is an explicit command — there is no environment
variable that approves anything (a test asserts exactly this).

## Grouped multi-artifact store

`tuisnap::grouped::GroupedStore` is an alternative store for suites that
want nested scenario names and a committed tree of human-reviewable
artifacts instead of frame JSON. A scenario name is a slash-separated path
(`showcase/pages/overview_120x40_truecolor`); each scenario commits EXACTLY
four artifacts under the approved root — no `.frame.json`, no sidecars:

```text
snapshots/showcase/pages/overview_120x40_truecolor.ansi   # normalized SGR dump (cell-exact gate)
snapshots/showcase/pages/overview_120x40_truecolor.txt    # plain black-and-white text
snapshots/showcase/pages/overview_120x40_truecolor.png    # colored image (pixel gate)
snapshots/showcase/pages/overview_120x40_truecolor.html   # standalone colored HTML render
```

```rust
use tuisnap::grouped::GroupedStore;

let store = GroupedStore::new(std::path::Path::new("tests/snapshots"));
// scratch defaults: tests/snapshots.actual/, tests/snapshots.diff/,
// report at tests/snapshots.actual/report.html — override with
// .with_actual_root(...) / .with_diff_root(...) / .with_report_path(...)
// (e.g. under target/; never inside the approved tree).
let mut renderer = profile.renderer(&VENDORED_FACES)?;
let outcome = store.check_with(&mut renderer, "pages/overview", &frame, 1.0)?;
outcome.ensure_matched()?;
```

Gate semantics per check (actuals + debug `<name>.frame.json` /
`.png.fidelity.json` sidecars are written to the actual root FIRST):

1. `.ansi` byte-compare — the cell-exact gate (symbol + fg + bg + mods per
   cell, deterministic). Failure reads as `CellsDiffer`.
2. `.txt` byte-compare — content gate: a style-only change shows as
   `ansi_match: Some(false)` with `txt_match: Some(true)`.
3. `.html` byte-compare — the render-level gate: identical cells through a
   changed renderer/font fail here as `PixelsDiffer`. The HTML embeds the
   frame JSON with the provenance timestamp zeroed, so identical screens
   serialize byte-identically (`Renderer::render_html` documents this).
4. `.png` decoded-pixel compare with the same threshold semantics as the
   classic store; the diff PNG lands under the diff root.

Any missing approved artifact is `MissingApproval` (fail-closed). Names are
validated everywhere: absolute paths, `..`/`.` segments, empty segments and
backslashes are rejected with `InvalidName`. Blessing is recursive:

```text
tuisnap accept --grouped --store snapshots --all        # or --name pages/overview
tuisnap report --grouped --store snapshots [--report-path target/report.html]
tuisnap check  --grouped --store snapshots --name pages/overview --input f.frame.json
```

The classic `Store` is untouched; both stores share `Status`,
`CompareOutcome`, the report machinery and the renderer.

## CLI reference

```text
tuisnap render --input F.frame.json --format png --format svg --format html \
  --format ansi --format txt --format json --out PREFIX [--font-file TTF]
tuisnap check   --store DIR --name N --input F.frame.json [--pixel-threshold X] [--font-file TTF]
tuisnap accept  --store DIR (--name N | --all)
tuisnap report  --store DIR [--title T] [--pixel-threshold X] [--font-file TTF]
tuisnap run     --cols C --rows R [--send STEP]... [--wait-for T] [--timeout-ms N]
  [--settle-ms N] [--format F]... --out PREFIX [--store DIR --name N] [--font-file TTF]
  -- CMD [ARGS...]
```

`check`, `accept` and `report` take `--grouped` for the grouped
multi-artifact store (`report` then also takes `--report-path`); `accept
--grouped --all` walks nested scenario names recursively. Without
`--grouped` the CLI drives the classic store exactly as before.

`report` re-verifies every `actual/*.frame.json` and rewrites the index —
use it in CI to publish one HTML artifact per run.

## Import/export contracts

| Artifact | Meaning |
|---|---|
| Canonical `frame.json` (compact) | Restores the structured frame; re-renders byte-identical PNGs |
| PNG | Approved visual baseline for pixel comparison — NOT state recovery |
| HTML generated by tuisnap | Embeds frame JSON + profile: lossless re-import via the `<script>` payload |
| Arbitrary external HTML | Not a frame representation; never parsed as one |
| Raw ANSI stream | Replay through the `termpane` emulator with explicit size (`ansi::replay_raw`) |
| Normalized ANSI dump | Debugging view generated FROM a frame; never re-parsed |
