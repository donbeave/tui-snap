# Alternatives review: TUI snapshot / capture tools vs `tuisnap`

## v0.2 update (verified Sept 2026 — supersedes the tuisnap column below)

- `tuisnap` v0.2 reuses: `termlens` 0.9 as the PTY engine (sync, pure-cargo,
  `Drop` kills the child, style-aware `wait_stable`), `vt100` 0.16 for raw
  replay (it HAS `dim` — the old "no dim" claim was wrong), `fontdue` 0.9.4
  for real-glyph PNG, `image-compare` 0.5.0 for pixel gates.
- Rejected as dependencies (verified): `terminal-control` 1.2.1
  (`c1d4f95e`) needs Rust 1.93 + **Zig 0.15.2** + network at build time
  (unconditional `libghostty-vt`) — incompatible with pure-cargo CI; its
  public frame model informed our schema only. `resvg` defaults to system
  fonts (breaks pinning) and outweighs direct cell rasterization. `reg-cli`
  needs Node — its 3-dir + HTML contract is mirrored natively instead.
- Corrections to the matrix below: `terminal-svg` is
  `russmckendrick/terminal-svg` (Rust, WOFF2-subset + source-metadata
  embeds), NOT another name for `pamburus/termframe`. `cellshot` moved to
  `anomalyco/terminal-control`. `termlens` 0.9 is far more than minimal
  (waits, assertions, cursor/mode inspection, insta macros).
- v0.2 closes the old gaps: real glyphs (no blocks), full approved
  frames+images (no hash-only), explicit `accept` (no `BLESS`), portable
  HTML with embedded PNGs + frame JSON, style-aware waits that fail on
  timeout, `--font-file` on all gates.

Scope: alternatives to **this project** — `tuisnap` v0.1.0 in this workspace
(Rust CLI + crate: real-PTY black-box capture AND in-process Ratatui headless
dumps, exporting `txt/ansi/json/svg/html/png`, with FNV-1a digest baselines).
Lineage: `terminal-components-claude` (`tools/capture.sh` tmux harness +
`junie-tui-testing` Harness/Scene digest).

Method: live web research (Firecrawl search + scrapes of crates.io, docs.rs,
GitHub, official docs), Sept 2026. Per-tool claims below cite sources at the end.
Star counts are quoted only where the scrape showed them (freeze); everything
else is characterised qualitatively to avoid stale numbers.

## TL;DR positioning

No single alternative covers the exact `tuisnap` intersection
(headless Ratatui dumps + real-PTY capture + 6 static formats + digest
baselines in one Rust-native tool with no tmux/Python/Node/Go dependency).
Closest neighbours:

- For **Ratatui unit/widget pinning**: official `TestBackend + insta` (do that anyway).
- For **black-box PTY power + video + agent/TS clients**: `cellshot` (most capable rival).
- For **README-grade CLI transcripts as tests**: `term-transcript` (unique SVG-as-oracle idea).
- For **pixel-beautiful stills**: `freeze` (best renderer, not a test tool).
- For **scripted GIF/MP4 demos**: `VHS` (best demo language), `asciinema + agg` (best recording ecosystem).
- For **full-app PTY integration tests in Rust**: `ratatui-testlib`, `termlens`.

`tuisnap`'s defensible niche: the only Rust-native single binary+crate that does
both halves (headless dumps for refactor gates, PTY capture for real binaries)
with reviewable text-first artifacts and `BLESS=1` baselines.

## Category A — headless / in-process snapshot (Ratatui)

### A1. Ratatui `TestBackend` + `insta` (official recipe)
- What: render any `Widget`/app into an in-memory `TestBackend` buffer, snapshot with `insta`.
- Sources: <https://ratatui.rs/recipes/testing/snapshots/>, <https://docs.rs/insta>
- Capture: headless (no PTY, no subprocess). Formats: `insta` text/debug snapshots (`.snap`), review via `cargo insta review` / `cargo insta test --review`; `INSTA_UPDATE=no|always`.
- Interaction: none (drive state by calling code). Determinism: excellent.
- Strengths: official, fastest, zero flake, huge community, editor support for `.snap`, inline snapshots.
- Weaknesses: tests only what you render in-process; never exercises the real binary, PTY escape pipeline, resize/mouse/timing; snapshot text is buffer-debug, not styled ANSI/SVG/PNG.
- Best for: widget/layout/refactor gates. Every Ratatui project should have this layer.
- vs tuisnap: `tuisnap::ratatui_shot::widget_frame/draw_frame` is this pattern, plus styled exporters (ANSI/SVG/HTML/PNG/JSON) and its own lightweight baseline so you don't need `cargo-insta` to gate.

### A2. `expect-test` (rust-analyzer)
- What: minimalist inline snapshots; `expect![[""]]` rewritten in-source with `UPDATE_EXPECT=1`.
- Sources: <https://github.com/rust-analyzer/expect-test>, <https://docs.rs/expect-test/latest/expect_test/>, <https://www.rustprojectprimer.com/testing/snapshot.html>
- Strengths: zero new files (snapshots live in source), trivial update loop, tiny dependency, great for compiler-style golden tests.
- Weaknesses: generic (not terminal-aware); no PTY, no rendering, no image formats; inline strings get unwieldy for full screens.
- Best for: small golden strings alongside `TestBackend` dumps.
- vs tuisnap: complementary; tuisnap's file baselines scale better to 120×40 screens.

### A3. CLI-snapshot herd: `assert_cmd` / `insta-cmd` / `trycmd` (+ `trybuild` inspiration)
- What: snapshot stdout/stderr/exit of CLI invocations; `trycmd` enumerates `.toml`-defined cases ("cattle, not pets").
- Sources: <https://docs.rs/trycmd>, <https://users.rust-lang.org/t/ann-trycmd-snapshot-testing-for-a-herd-of-cli-tests/66915>
- Strengths: best for non-interactive CLI surface (flags, help, errors); scales to hundreds of cases.
- Weaknesses: pipe-based, not PTY: `isatty` checks, wrapping, colours, fullscreen/alternate-screen TUIs don't work; no interaction, no images.
- Best for: CLI argument/output matrix. Not a TUI screen tool.
- vs tuisnap: different layer; use trycmd for CLI surface, tuisnap for screens.

### A4. `tui-snapshot-tester` (Tessl/testland)
- What: registry package for deterministic SVG/text frame snapshots with per-run diffs.
- Source: <https://tessl.io/registry/testland/tui-snapshot-tester> (+ quality page)
- Strengths: purpose-built TUI snapshot framing (SVG+text determinism).
- Weaknesses: third-party registry dependency, smaller community than insta/expect-test; niche packaging.
- Best for: teams already on that registry wanting drop-in TUI snapshots.
- vs tuisnap: overlaps headless snapshot; tuisnap adds PTY path + PNG/HTML/JSON + no registry lock-in.

## Category B — real-PTY black-box harness (Rust)

### B1. `cellshot` (kitlangton) — strongest rival
- What: Rust CLI + library + external driver + TypeScript client for controlling, inspecting, testing, capturing real terminal apps; pure-Rust `vt100` renderer; exports PNG/SVG/JSON/text/raw-ANSI; MP4 via ffmpeg; `.cellshot` JSONL recordings.
- Sources: <https://crates.io/crates/cellshot> (full feature scrape), `@cellshot/test` npm client.
- Commands: `show/save/start/status/wait/send/resize/logs/restart/stop/video/driver` — named sessions over Unix sockets, `wait-for-text/idle`, settle-before-capture, `--format` repeats, `--pipe`, `--input -` for offline render, `transcript.ansi`/recording opt-in for secrets, versioned JSON schemas (`frame-v1`, `recording-entry-v1`).
- Strengths: deepest session lifecycle, agent-oriented (driver protocol, TS/Vitest matchers, on-failure artifacts), recordings + video, secret-aware artifact policy, cross-platform named sessions.
- Weaknesses: larger surface to learn/operate; video needs ffmpeg; full power (TS client, video) pulls Node/ffmpeg; no headless Ratatui-dump story (it's black-box only).
- Best for: agent-driven TUI testing, failure-evidence pipelines, demo videos of real apps.
- vs tuisnap: tuisnap v0 copies its architecture (portable-pty+vt100, wait/idle/settle, `--format` repeats, offline `--input`) but stays minimal (one-shot `run`, no daemon/sockets/video/TS). tuisnap's exclusive edge is the headless Ratatui half + digest baselines in the same tool. If you need sessions/video/TS, use cellshot; if you want one small Rust tool for dumps+captures+gates, tuisnap.

### B2. `ratatui-testlib` / `terminal-testlib` (raibid-labs)
- What: PTY integration-test library (portable-pty + vt100) with `TuiTestHarness` (spawn, wait_for, send, screen_contents), `insta`/`expect-test` snapshot integration, Bevy ECS + Sixel/Kitty/iTerm2 graphics support.
- Sources: <https://crates.io/crates/ratatui-testlib>, <https://docs.rs/ratatui-testlib>, <https://github.com/raibid-labs/ratatui-testlib> (+ `ARCHITECTURE.md`, `RESEARCH.md`, `EXISTING_SOLUTIONS.md` docs)
- Strengths: Rust-test-native (no CLI to shell out to), graphics-protocol coverage (Sixel) nobody else has, headless Bevy runner, explicit TestBackend-comparison docs.
- Weaknesses: library-only (no CLI/screenshot exporter); younger/smaller community; graphics breadth you may never need.
- Best for: Rust integration tests incl. graphics protocols, Bevy-based TUIs.
- vs tuisnap: tuisnap's PTY core is the same stack; tuisnap adds CLI + 6 exporters + baselines, but lacks Sixel/Bevy.

### B3. `termlens` (vyncint)
- What: headless PTY test harness — spawn real binary in real PTY, VT-emulate to in-memory grid, assert/snapshot rendered screen (web-testing style for terminals).
- Sources: <https://lib.rs/crates/termlens>, <https://docs.rs/termlens>, <https://github.com/vyncint/termlens>
- Strengths: minimal, focused "Playwright for terminal" idea; real-PTY fidelity without a big platform.
- Weaknesses: new (v0.1.0, Aug 2026 per repo), thin ecosystem, no exporter/video story.
- Best for: teams wanting the smallest real-PTY assertion lib.
- vs tuisnap: philosophy overlap; tuisnap additionally ships CLI, 6 formats, baselines, headless dumps.

### B4. `rexpect` / `expectrl`
- What: script interactive sessions (send input, match output) — Expect for Rust.
- Source: <https://www.rustadventure.dev/building-a-digital-garden-cli/clap-v4/testing-interactive-clis-with-rexpect> (+ trycmd thread context)
- Strengths: interaction scripting incl. passwords/prompts; Unix-proven pattern.
- Weaknesses: matching on byte streams, not rendered screens; brittle for fullscreen TUIs; Unix-only for rexpect; no snapshots/images.
- Best for: interactive prompt/REPL flows, not screen pinning.
- vs tuisnap: use for flows, tuisnap for frames.

### B5. `tui-term` (a-kenji)
- What: inverse direction — a pseudoterminal *widget* inside Ratatui (embed a terminal in your TUI); uses vt100 + insta for its own snapshots.
- Source: raibid-labs `EXISTING_SOLUTIONS.md` survey.
- Note: not a snapshot tool, but relevant when your TUI embeds terminals (then PTY-path testing is mandatory).
- vs tuisnap: if you embed terminals, pair tui-term with tuisnap's PTY path.

## Category C — ANSI-to-image renderers (stills, not tests)

### C1. `freeze` (charmbracelet) — best stills renderer
- What: Go CLI generating PNG/SVG/WebP of code (Chroma highlighting) and terminal output (`--execute`), themes, fonts (+embedded TTF/WOFF), window chrome, padding/margin/shadow/border, line numbers, config files, interactive TUI configurator; TUIs via `tmux capture-pane | freeze`.
- Source: <https://github.com/charmbracelet/freeze> — 4.8k stars, 105 forks, MIT, 417 commits (scraped).
- Strengths: publication-quality output, huge theme/font/window controls, multi-format incl. WebP, proven at scale.
- Weaknesses: renderer, not a test harness (no assertions/baselines/review workflow, no drive/wait/settle, TUI path depends on external tmux).
- Best for: docs/blog/README screenshots.
- vs tuisnap: tuisnap's SVG/HTML/PNG cover the same formats dependency-free, but v0 PNG is geometry blocks (no TTF) — freeze wins beauty; tuisnap wins testing (digest gates) + single-Rust-toolchain.

### C2. `termshot` (homeport)
- What: PNG screenshots from ANSI "rich text" piped from real command output (unlike Carbon-style highlighters).
- Source: <https://github.com/homeport/termshot>
- Strengths: faithful ANSI-to-PNG for command output; simple `-- file` UX.
- Weaknesses: stills only, no interaction/testing/formats breadth.
- Best for: quick terminal-output PNGs.
- vs tuisnap: `render` subsumes this for ANSI→PNG plus 5 more formats.

### C3. `termframe` / `terminal-svg` (pamburus et al., Rust)
- What: non-interactive terminal emulator executing one command, rendering output to SVG (full ANSI support, editable).
- Sources: <https://github.com/pamburus/termframe>, <https://terminaltrove.com/termframe/>, <https://www.russ.cloud/2026/07/12/a-catch-up-terminal-svg-and-token-use-v1/>, <https://www.reddit.com/r/commandline/comments/1i8vyny/terminalsvgscreenshot_create_beautiful_editable/>
- Strengths: Rust-native, crisp editable SVGs, good docs use-case.
- Weaknesses: single-command stills; no interaction/snapshots/video.
- Best for: Rust shops wanting SVG stills without Go/Node.
- vs tuisnap: closest renderer kin; tuisnap adds PTY interaction + testing + more formats.

### C4. Conversion libs: `anstyle-svg`, `ansi-to-tui` (ratatui), `ansi4tui`
- What: building blocks — ANSI→SVG, ANSI→Ratatui `Text`, ANSI→TUI styles.
- Sources: <https://github.com/ratatui/ansi-to-tui> (99 stars, 28 forks, 1.3k dependents per scrape), lib.rs CLI listings.
- Note: tuisnap hand-rolls its SGR layer (tcc parity); swapping to these libs is a viable refactor, not a rival.

### C5. `term-transcript` (+ `term-transcript-cli`) — unique test-oracle idea
- What (v0.5.0, docs.rs): capture CLI/REPL transcripts (pipe by default, `portable-pty` feature), save as **static SVG**, parse transcripts **back from SVG**, assert terminal output (text or text+colour) against the SVG. Static-by-design (animated SVGs = noise; SVG stays copyable). Handlebars SVG templates, font subsetting/embedding, `test`/`svg` features.
- Sources: <https://docs.rs/term-transcript>, <https://crates.io/crates/term-transcript-cli>
- Limitations (self-declared): SGR-only (other CSI/OSC dropped or error); pipe capture breaks `isatty`/sizing programs; PTY feature still poor for cursor-moving fullscreen output → **not a fullscreen-TUI tool**.
- Best for: README-embedded CLI transcripts that double as regression tests.
- vs tuisnap: borrow its "snapshot IS the docs image" philosophy; tuisnap covers what it explicitly excludes (fullscreen cursor-moving TUIs via vt100 emulation) + PNG/HTML/JSON/digest.

## Category D — session record / replay / demo (motion, not gates)

### D1. `VHS` (charmbracelet)
- What: `.tape` scripts → GIF/MP4 (+publish/hosting); `Type/Enter/Sleep/Screenshot/Hide/Show`, theming, `vhs record` to generate tapes.
- Sources: <https://github.com/charmbracelet/vhs> (+ docs/mirror coverage)
- Strengths: demos-as-code, best GIF/MP4 story, tutorial ecosystem.
- Weaknesses: motion media, not assertions; GIF weight; not frame-exact gates.
- Best for: marketing/docs demos, release GIFs.
- vs tuisnap: roadmap (`record` tape file + `video`) is VHS-inspired; today tuisnap does stills+gates, VHS does motion.

### D2. `asciinema` + `agg` + `svg-term-cli`/`termtosvg` (+ terminalizer, t-rec, ttygif)
- What: `asciinema` records `.cast`; `agg` renders GIF (gifski quality, heavy files); `svg-term-cli` renders animated SVG (crisp, small); `termtosvg` records straight to animated SVG.
- Sources: <https://docs.asciinema.org/manual/agg/>, <https://sadman.ca/blog/software-showcase-01-asciinema/>, <https://github.com/orangekame3/awesome-terminal-recorder>, <https://news.ycombinator.com/item?id=17449810>
- Strengths: biggest recording ecosystem, sharing (asciinema.org), format choice GIF-vs-animated-SVG.
- Weaknesses: dynamic by nature (bad for assertions), animated SVGs uncopyable/unrewindable, GIF size, multi-tool chain.
- Best for: session sharing + web embeds.
- vs tuisnap: complementary; tuisnap deliberately static (like term-transcript's argument).

### D3. tmux `capture-pane` scripts (tcc `tools/capture.sh` pattern)
- What: drive tmux panes, `capture-pane -e -p` dumps, keys/mouse/resize scripting, external rasterise.
- Strengths: works with ANY binary today, scriptable, proven by tcc's `shots/` corpus + provenance manifest.
- Weaknesses: tmux + Python + system-font dependencies, shell fragility, macOS/Linux-centric, no library story.
- vs tuisnap: tuisnap `run` is the portable-pty replacement for exactly this; `render` replaces the Python scripts.

## Comparison matrix

| Tool | Type | Real PTY | Headless | Drives input | Wait/settle | txt | ansi | json | svg | html | png | gif/mp4 | Baseline/digest | Review UX | Rust lib | CLI | Video |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| tuisnap (this project) | capture+test | ✅ | ✅ Ratatui | ✅ keys/type/sleep/wait | ✅ text+idle | ✅ | ✅ | ✅ v1 | ✅ | ✅ | ✅ blocks* | ❌ roadmap | ✅ FNV+BLESS | CLI | ✅ | ✅ | ❌ |
| TestBackend+insta | test | ❌ | ✅ | ❌ (call code) | n/a | snap | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ .snap | ✅ cargo-insta | ✅ | ❌ | ❌ |
| expect-test | test | ❌ | via backend | ❌ | n/a | inline | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ inline | UPDATE_EXPECT | ✅ | ❌ | ❌ |
| trycmd herd | test | ❌ pipes | ❌ | args only | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ files | ✅ | ✅ | ❌ | ❌ |
| term-transcript | test+docs | ⚠️ opt PTY | ❌ | ✅ commands | ❌ | ❌ | ❌ | ❌ | ✅ static | ❌ | ❌ | ❌ | ✅ SVG-oracle | cargo test | ✅ | ✅ | ❌ |
| cellshot | capture+test+video | ✅ vt100 | ❌ | ✅ rich+stdin | ✅ text+idle | ✅ | ✅ opt-in | ✅ v1 | ✅ | ❌ | ✅ | ✅ mp4 | ✅ snapshots | TS/Vitest | ✅ | ✅ | ✅ |
| ratatui-testlib | test lib | ✅ vt100 | ✅ Bevy | ✅ keys/text/mouse | ✅ cond | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | via insta | insta | ✅ | ❌ | ❌ |
| termlens | test lib | ✅ | ❌ | ✅ typed keys | ✅ | grid | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | asserts | ✅ | ❌ | ❌ |
| freeze | stills | ⚠️ via tmux | ❌ | ❌ | ❌ | ❌ | in | ❌ | ✅ | ❌ | ✅ | ✅ webp | ❌ | ❌ | ❌ Go | ✅ | ❌ |
| termshot | stills | pipe ANSI | ❌ | ❌ | ❌ | ❌ | in | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ Go | ✅ | ❌ |
| termframe/terminal-svg | stills | ✅ emu | ❌ | single cmd | ❌ | ❌ | in | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ | ❌ |
| VHS | demo | ✅ | ❌ | ✅ tape | ✅ sleep | ❌ | ❌ | ❌ | ❌ | ❌ | stills | ✅ | ❌ | ❌ | ❌ Go | ✅ | ✅ |
| asciinema+agg/svg-term | record | ✅ | ❌ | ✅ record | n/a | ❌ | ❌ | cast | ✅ anim | ❌ | ❌ | ✅ gif | ❌ | ❌ | ❌ | ✅ | ✅ |
| rexpect/expectrl | interact | ✅ | ❌ | ✅ | ✅ match | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |
| tcc capture.sh+Harness | harness | ✅ tmux | ✅ | ✅+mouse | ✅ render | ✅ | ✅ | ❌ | ❌ | ✅ | ✅ MPL | ❌ | ✅ digest+BLESS | BLESS | ✅ dev-only | shell | ❌ |

\* tuisnap v0 PNG is geometry-exact backgrounds + legibility blocks (no TTF) — fine for diff review, not publication beauty; font-accurate rendering is roadmap (fontdue + embedded TTF, freeze-style).

## Recommendations

1. Keep `TestBackend + insta` for widget gates regardless of anything else (fastest, official).
2. For CLI docs-that-test-themselves, add `term-transcript` SVG snapshots of representative commands.
3. For agent/failure-evidence pipelines or MP4 needs, adopt `cellshot` (sessions, driver, TS client, recordings).
4. For beautiful release stills, render with `freeze` (or `termframe` to stay Rust-only).
5. For GIF/MP4 demos, script `VHS` tapes (or `asciinema+agg` for shared recordings).
6. For fullscreen-TUI regression gates with reviewable artifacts and no external toolchain, use `tuisnap` (headless dumps per screen + PTY captures of the real binary + `BLESS=1`).
7. Retire tmux+Python capture scripts in favour of `tuisnap run/render` (same bundle, fewer dependencies); keep the provenance-manifest idea for a future `--provenance` flag.

## What tuisnap should still borrow (gaps)

- cellshot: named sessions, `logs` vs `screen` split, secret-aware defaults (ANSI/transcript opt-in), versioned schemas, on-failure artifact hooks, driver protocol.
- term-transcript: SVG-as-single-source-of-truth docs pattern; Handlebars SVG templates; font embedding.
- freeze: TTF embedding, themes, window chrome, WebP.
- VHS: tape-file DSL for multi-shot scripted runs.
- tcc: provenance manifest (argv, binary hash, git dirty, env, tool versions), atomic publish.
- ratatui-testlib: Sixel graphics coverage (only if users need it).

## Sources

- tuisnap workspace + README in this repo; `terminal-components-claude` (`tools/capture.sh`, `ansi2png.py`, `ansi2html.py`, `crates/tui-testing` Harness/Scene/Baseline).
- <https://crates.io/crates/cellshot> (cellshot CLI/lib/driver/TS client/recording docs)
- <https://crates.io/crates/ratatui-testlib>, <https://docs.rs/ratatui-testlib>, <https://github.com/raibid-labs/ratatui-testlib> (incl. ARCHITECTURE/RESEARCH/EXISTING_SOLUTIONS/TESTING_APPROACHES docs)
- <https://ratatui.rs/recipes/testing/snapshots/>, <https://docs.rs/insta>, <https://insta.rs/docs/quickstart/>, <https://github.com/mitsuhiko/insta>
- <https://github.com/rust-analyzer/expect-test>, <https://docs.rs/expect-test/latest/expect_test/>, <https://www.rustprojectprimer.com/testing/snapshot.html>
- <https://docs.rs/trycmd>, <https://docs.rs/term-transcript> (v0.5.0 incl. Alternatives/Limitations), <https://crates.io/crates/term-transcript-cli>
- <https://github.com/charmbracelet/freeze> (4.8k★/105 forks/MIT/Go per scrape), <https://github.com/charmbracelet/vhs>
- <https://github.com/homeport/termshot>, <https://github.com/pamburus/termframe>, <https://terminaltrove.com/termframe/>, <https://www.russ.cloud/2026/07/12/a-catch-up-terminal-svg-and-token-use-v1/>, <https://github.com/ratatui/ansi-to-tui>
- <https://lib.rs/crates/termlens>, <https://docs.rs/termlens>, <https://github.com/vyncint/termlens>, <https://tessl.io/registry/testland/tui-snapshot-tester>
- <https://docs.asciinema.org/manual/agg/>, <https://sadman.ca/blog/software-showcase-01-asciinema/>, <https://github.com/orangekame3/awesome-terminal-recorder>, <https://news.ycombinator.com/item?id=17449810>
- <https://www.rustadventure.dev/building-a-digital-garden-cli/clap-v4/testing-interactive-clis-with-rexpect>, <https://users.rust-lang.org/t/ann-trycmd-snapshot-testing-for-a-herd-of-cli-tests/66915>
