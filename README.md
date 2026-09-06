# tuisnap — Rust TUI visual-regression toolkit

Two capture paths share one canonical frame; both produce full approved
frames, readable PNGs, and portable HTML expected/actual/diff reports.

```text
Fixture model + view state + viewport + theme
        └─▶ actual production Ratatui view ──▶ frame        (no PTY, no subprocess)

Real executable ──▶ PTY + terminal-state engine ──▶ frame   (keyboard/mouse/resize)
```

A changed snapshot requires explicit review (`tuisnap accept`). Equality only
validates the fixtures covered — never every app state.

## Quick start: pure view tests

```rust
use tuisnap::{Profile, Provenance, VENDORED_FONT};
use tuisnap::snapshot::Store;

#[test]
fn home_screen() {
    let store = Store::new(std::path::Path::new("tests/visual"));
    let profile = Profile::default_profile();
    // Render the ACTUAL production view from fixture data:
    let frame = tuisnap::ratatui::draw_frame(120, 40, prov(), |f| {
        myapp::render_home(f, &fixture_model())
    });
    // Actual artifacts are written BEFORE the assertion, so a failure still
    // leaves reviewable evidence (actual/*.frame.json + *.png + report.html).
    let outcome = store.check("home", &frame, &profile, VENDORED_FONT, 1.0).unwrap();
    outcome.ensure_matched().unwrap();
}
```

First run fails with `missing-approval` (fail-closed). Inspect
`actual/*.png` + `report.html`, then accept explicitly:

```text
cargo run -q -- accept --store tests/visual --name home   # one snapshot
cargo run -q -- accept --store tests/visual --all         # everything reviewed
```

There is deliberately **no** `BLESS=1` / auto-accept: CI must never approve
snapshots by itself (see `docs/CI.md`).

## Interactive tests (feature `pty`, on by default)

```rust
let mut s = tuisnap::pty::Session::spawn(&["./my-tui".into()], &opts)?;
s.wait_for_text("Ready")?;                 // timeout fails WITH the screen
s.send_key("enter")?;
let frame = s.wait_stable(Duration::from_millis(300))?;  // style-aware settle
```

Pure view tests build without the PTY engine: `cargo test --no-default-features`.

## CLI

```text
tuisnap render --input shot.frame.json --format png --format svg --out shot
tuisnap check  --store tests/visual --name home --input actual.frame.json
tuisnap accept --store tests/visual --name home        # or --all
tuisnap report --store tests/visual                    # re-verify + rewrite report.html
tuisnap run --cols 120 --rows 40 --send enter --wait-for Ready \
  --store shots --name home -- ./my-tui                # capture + gate
```

`render` also accepts `--font-file` (hash recorded); all gates accept it too.
Offline `frame.json` re-renders byte-identical PNGs (proven by tests).

## Layout of a store

```text
<store>/approved/<name>.frame.json   # the only committed artifact (compact JSON)
<store>/actual/<name>.frame.json     # local evidence (gitignored)
<store>/actual/<name>.png
<store>/diff/<name>.png              # red-overlay diff, on mismatch
<store>/report.html                  # portable: embedded PNGs + frame JSON
```

Approved PNGs regenerate deterministically and are not committed.

## Fidelity contract

- Layout from frame widths (CJK keeps 2 cells even as tofu); real glyphs via
  `fontdue` from a pinned vendored font — never placeholder blocks.
- Profile pins font bytes (SHA-256), 10×19 cells at 16px, palette, scale ×2,
  cursor policy. `verify_geometry` fails loudly on drift.
- Covered: box drawing, blocks, Braille, Nerd icons, combining marks.
  CJK/emoji without font coverage render as deterministic tofu with correct
  advance (documented in `assets/fonts/FONTS.md`).
- Terminal-like, measured fidelity — NOT pixel-identity with any terminal
  emulator. Faux-bold (double-strike) and faux-italic (shear) are documented
  approximations; cell data stays authoritative for styles.

## Docs

- `docs/USAGE.md` — patterns, CLI reference, approval workflow
- `docs/MIGRATION.md` — v0.1 → v0.2 (breaking), BLESS removal
- `docs/CI.md` — CI wiring that cannot auto-accept
- `assets/fonts/FONTS.md` — font licensing and coverage
- `RESEARCH.md` — architecture analysis this implements
- `ALTERNATIVES-REVIEW.md`, `SIMILAR-PROJECTS.md` — competitor landscape

## License

Apache-2.0 — see [LICENSE](LICENSE).
