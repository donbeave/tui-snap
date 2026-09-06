# tuisnap — TUI snapshots for humans and refactors

One artifact model, two capture paths, six formats.

```text
cargo run -q -- run --cols 120 --rows 40 --format txt --format png --format svg \
  --out shots/home -- ./my-tui --flag
cargo run -q -- render --input shots/home.ansi --cols 120 --rows 40 \
  --format png --format html --out shots/home
cargo run -q -- digest --input shots/home.ansi --name home --baseline baselines/tui.txt
BLESS=1 cargo run -q -- digest --input shots/home.ansi --name home --baseline baselines/tui.txt
```

## Library (Ratatui refactoring guard)

```rust
use ratatui::widgets::Paragraph;
use tuisnap::{Baseline, ratatui_shot::widget_frame};

#[test]
fn screens_pinned() {
    let base = Baseline::new("tests/baselines/tui.txt");
    for (name, text) in [("home", "hello"), ("empty", "nothing")] {
        let frame = widget_frame(Paragraph::new(text), 120, 40);
        base.assert_frame(name, &frame).unwrap();
        tuisnap::render::write_format(&frame, "png", &format!("shots/{name}")).unwrap();
    }
}
```

Headless `TestBackend` dumps: deterministic, no PTY flake. Drive app state
machine via method calls, dump each screen, pin with `Baseline`.
`BLESS=1` / `UPDATE_SNAPSHOT=1` / `UPDATE_BASELINE=1` regenerate.

## CLI

- `run`: spawn any binary in real PTY (no tmux), `--send` steps
  (`type:<..>`, `enter|escape|tab|up|down|left|right|space|ctrl-x|text:..`,
  `sleep:<ms>`, `wait:<needle>`), `--wait-for`, settle on idle, write
  `--format` repeats to `--out.<ext>`.
- `render`: offline `.ansi` → `txt|ansi|json|svg|html|png` (replaces
  `ansi2png.py` / `ansi2html.py`, no Python/fonts needed).
- `digest`: `.ansi|.txt|.json` → FNV-1a vs `name cols rows hash` baseline.

## Lineage (borrowed best)

- `terminal-components-claude` `tools/capture.sh`: fixed geometry, key/mouse
  scripts, `ansi/txt/cursor/html/png` bundle, provenance, atomic publish,
  digest baselines (`Harness` + `Scene` + `BLESS=1`).
- `cellshot`: `portable-pty` + `vt100`, `show/save/start/send/wait/resize/stop`,
  `wait-for-text/idle`, settle-before-snapshot, repeat `--format`, versioned
  JSON, `.cellshot` recordings, driver protocol.
- `ratatui-testlib`: PTY harness + `insta`/`expect-test` snapshots.
- `TestBackend` + `insta`: in-process golden tests (this crate's `ratatui_shot`).
- `term-transcript`: SVG transcripts as test oracles.
- `freeze` / `termshot` / `anstyle-svg`: pretty SVG/PNG from ANSI.
- `VHS`: tape-script demos → GIF/MP4 (roadmap: `record` script file + `video`).

## Roadmap to best-in-class

1. Named sessions (`start/send/wait/show/save/stop`, Unix socket).
2. Tape file (`record` script → reproducible multi-shot runs).
3. Font-accurate PNG (embedded TTF via `fontdue`) + GIF/MP4 via `ffmpeg`.
4. `insta` adapter macro + cursor/mouse SGR + scrollback/logs split.

## License

Apache-2.0 — see [LICENSE](LICENSE).
