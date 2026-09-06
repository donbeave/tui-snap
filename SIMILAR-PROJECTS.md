# Similar projects

GitHub repositories related to `tuisnap` (TUI snapshots: PTY capture +
headless dumps → txt/ansi/json/svg/html/png + digest baselines).
All links verified to resolve (Sept 2026). See `ALTERNATIVES-REVIEW.md`
for the full head-to-head comparison.

## PTY capture + TUI testing (closest rivals)

- https://github.com/anomalyco/terminal-control — cellshot (was `kitlangton/cellshot`): PTY sessions, PNG/SVG/JSON captures, recordings, MP4, TS client. Strongest rival.
- https://github.com/raibid-labs/ratatui-testlib — PTY integration-test harness (portable-pty + vt100), insta/expect-test hooks, Sixel/Bevy support.
- https://github.com/vyncint/termlens — minimal headless PTY test harness ("Playwright for terminal").
- https://github.com/a-kenji/tui-term — pseudoterminal widget for Ratatui (inverse direction: embed a terminal in your TUI).

## Snapshot testing foundations

- https://github.com/mitsuhiko/insta — generic snapshot testing for Rust (`cargo insta review`).
- https://github.com/rust-analyzer/expect-test — minimalist inline snapshots (`UPDATE_EXPECT=1`).
- https://github.com/slowli/term-transcript — CLI/REPL transcripts as SVG test oracles (static SVG, parse-back asserts).
- https://github.com/ratatui/ansi-to-tui — ANSI → Ratatui `Text` conversion library.
- https://github.com/ratatui/ratatui — the TUI framework itself (incl. `TestBackend` recipe).

## ANSI → image renderers (stills)

- https://github.com/charmbracelet/freeze — publication-quality PNG/SVG/WebP of code and terminal output (themes, fonts, window chrome).
- https://github.com/homeport/termshot — PNG screenshots from ANSI output.
- https://github.com/pamburus/termframe — Rust terminal-output → SVG screenshot tool.
- https://github.com/russmckendrick/terminal-svg — Rust terminal emulation + SVG with embedded WOFF2 font subsets and source metadata for re-rendering (distinct from termframe).
- https://github.com/reg-viz/reg-cli — image-comparison + HTML report workflow (3-dir contract mirrored natively by tuisnap; needs Node).

## Session record / demo (motion)

- https://github.com/charmbracelet/vhs — terminal GIFs/MP4 as code (`.tape` scripts).
- https://github.com/asciinema/asciinema — terminal session recorder (`.cast`).
- https://github.com/asciinema/agg — GIF generator for asciinema recordings.
- https://github.com/marionebl/svg-term-cli — asciicast → animated SVG.
- https://github.com/nbedos/termtosvg — record terminal sessions as SVG animations.
- https://github.com/faressoft/terminalizer — record terminal, GIF + web player.
- https://github.com/sassman/t-rec-rs — blazing-fast terminal → GIF recorder in Rust.
- https://github.com/icholy/ttygif — ttyrec → GIF converter.
- https://github.com/chjj/ttystudio — record terminal → GIF/APNG.
- https://github.com/orangekame3/awesome-terminal-recorder — curated list of terminal recorders.
