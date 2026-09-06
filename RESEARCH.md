<question>
Research this tool:
https://github.com/donbeave/tui-snap

And all its competitors. All these reference projects. We want to understand if this project still makes sense or if it can't be replaced by something else. The main idea is to have a tool to verify that my TUI application isn't breaking. Verification can be done in two ways: one, we interact with the terminal application to ensure it works correctly and see the output. We'll probably use the view layer separately to check if we've broken the view. It's important to dump the view in files and then render them to compare. In this case, we'll write many unit tests for the view layer without using business logic or any other logic. We'll use a model and view approach, providing the model and seeing how the view is rendered. If the view renders accurately, it means we didn't break the view layer. If the view is different, we've broken it. The whole concept is to have almost the same experience as what I see in my terminal—the same output, the same look and feel—to verify by my eyes that nothing broke. If I change the UI, I can see the new UI visually verified much more easily than by text. Import/export as PNG or HTML is very important for me.
</question>

</research>
## Verdict

**Your testing approach makes sense. I would not continue developing `tui-snap` as a separate terminal-capture engine, terminal emulator, renderer, and snapshot-management system.**

I would keep it—potentially as a small library inside your existing projects—as an **integration layer for Ratatui view snapshots, reproducible rendering, and visual review**. Most of the underlying functionality already exists elsewhere.

The important qualification is that **the current `tui-snap` implementation does not yet satisfy your visual-verification requirement**. Its PNG exporter draws rectangular placeholders instead of letters and other glyphs. That is not a minor screenshot-quality issue: it prevents you from visually reviewing the actual interface.

My recommended direction is:

| Responsibility                           | What I would use                                                                  |
| ---------------------------------------- | --------------------------------------------------------------------------------- |
| Isolated model → view rendering          | Ratatui `TestBackend`                                                             |
| Structured snapshot storage and approval | `insta`                                                                           |
| Interactive testing of real applications | Terminal Control, or `termlens` for a Rust-test-focused alternative               |
| PNG rendering                            | Reuse an existing renderer, but validate and pin its font/rendering configuration |
| Expected/actual/diff HTML reports        | Existing image-review tooling such as `reg-cli`, or a thin Rust integration       |
| Project-specific glue                    | A substantially smaller, refocused `tui-snap`                                     |

The following assessment covers the references in both repository comparison documents and additional relevant alternatives. I inspected `tui-snap` at commit **`10dad93c3e26d4c8bb82ea050d4fd756189dc6ca`** and checked competitor source or official documentation as of **September 6, 2026**. This is a source-and-architecture review, **not a hands-on comparison of rendered screenshots or performance benchmarks**.

---

## 1. Your requirements are three related—but different—testing problems

Separating them makes the replacement decision much clearer.

### A. Isolated view regression tests

This is exactly the model → view approach you described:

```text
Known model + known view state + fixed terminal dimensions + fixed theme
                                  ↓
                    Your actual rendering function
                                  ↓
                         Terminal cell buffer
```

No database, network, application startup, or business operations are necessary. View state still includes things such as the selected row, scroll position, focused input, open dialog, and cursor position.

Ratatui already provides the essential mechanism through `TestBackend`; its official snapshot-testing recipe combines that backend with `insta`. `tui-snap`’s `widget_frame` and `draw_frame` functions are convenient wrappers around this existing pattern, rather than a fundamentally new testing capability. ([Ratatui][1])

### B. Interactive application tests

These answer different questions:

Does pressing Enter activate the selected item? Does resizing trigger a redraw? Does the application restore terminal state when exiting? Does the real executable produce the expected screen?

For this layer, a real PTY and a terminal-state emulator are useful. Terminal Control and `termlens` already provide substantial functionality here.

### C. Visual review of the rendered result

This is the part that plain text snapshots do not solve:

```text
Approved frame → approved image
Actual frame   → actual image
                        ↓
             Side-by-side + visual diff
```

You need to see colors, borders, spacing, glyphs, emphasis, selection states, and clipping—not merely a changed hash or text string.

One refinement to your premise: **a changed snapshot means “review required,” not necessarily “the view is broken.”** Conversely, an unchanged snapshot means that the tested fixture still matches its approved reference; it does not prove that all possible application states are correct.

I would build your workflow around all three layers, rather than expecting one kind of snapshot to prove everything.

---

## 2. What is good—and currently wrong—in `tui-snap`

The good architectural idea is **two capture paths converging on one frame model**, followed by offline artifact generation. That is worth preserving. The current implementation of that idea is incomplete in several important ways.

### The most important findings

| Finding                                                                                                                                                                                              | Why it matters for your use case                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| **PNG output does not render glyphs.** Nonblank characters become the same rectangular mark, with a different size for bold.                                                                         | An `A` changing to a `B` can produce identical PNG pixels. The digest can detect the text change, but the image cannot show you what changed. |
| **The exporters do not share a faithful rendering profile.** PNG uses 9×20 cells; SVG uses 9×18 cells. HTML/SVG rely on font-family fallbacks.                                                       | Different exported formats can present different geometry or typography for the same frame.                                                   |
| **The offline ANSI parser is not a terminal emulator.** It discards cursor-moving CSI sequences and treats each Unicode scalar as one cell.                                                          | Replaying a raw fullscreen terminal stream can produce the wrong screen. Wide characters and combining sequences are especially problematic.  |
| **Capture loses relevant state.** The Ratatui adapter does not propagate the terminal cursor; PTY conversion sets `dim` to false; the frame schema lacks explicit cell-width information.            | The two capture paths are not equivalent representations of everything the user sees.                                                         |
| **Wait failures can be ignored.** `wait_for_text` returns `false` on timeout, but `run_once` discards that result. “Idle” checks text rather than complete screen state.                             | A scenario can continue after its expected screen never appeared, or capture while only colors/cursor state are changing.                     |
| **Baselines contain hashes, not the approved frames.** The digest also omits cursor position and visibility.                                                                                         | You cannot reconstruct an expected image from the baseline. Cursor-only regressions can escape that comparison.                               |
| **Import/export is asymmetric.** The `render` command always parses input as ANSI text. JSON is accepted by `digest`, but not as a structured frame by `render`. PNG/HTML import is not implemented. | “Six output formats” does not yet provide the reusable artifact round-trip you want.                                                          |

There is also a workflow issue in the README example: it asserts the baseline **before** writing the PNG. A failing assertion therefore prevents the visual artifact from being written—the moment when you most need it.

The inspected exporter test checks that outputs are nonempty. That verifies file generation, not that the files faithfully render text, styles, Unicode, or terminal geometry.

**Consequently, I would not replace a working capture pipeline with the current `tui-snap` solely because it offers more formats or fewer dependencies.**

---

## 3. The serious replacement candidates

### Ratatui `TestBackend` + `insta`: the foundation I would adopt regardless

This is the strongest starting point for your large collection of isolated view tests.

Use the real production view function with fixture data, then snapshot a representation that includes **symbols, positions, colors, modifiers, and relevant cursor state**. A plain string snapshot alone will miss style-only changes.

Importantly, `insta` is not limited to text: it supports serialized snapshots and also has experimental binary snapshots. However, binary snapshots are compared **byte-for-byte**. That is not the same as comparing decoded PNG pixels, and it does not automatically provide the visual-review interface you described. ([Insta Snapshots][2])

**Replacement verdict:** use it instead of inventing another general baseline/approval system. Add rendering and image review alongside it.

### Terminal Control, formerly cellshot: the strongest broad capture replacement

The existing comparison in your repository is outdated here.

Current Terminal Control source uses **Ghostty’s terminal core through `libghostty-vt`**, rather than the earlier `vt100` architecture. The inspected manifest declares version `1.2.1`; source builds require Rust and Zig. This is a statically linked terminal core—not a requirement to launch the Ghostty desktop application.

It provides real PTY sessions, keyboard and mouse input, resizing, waits, screen inspection, PNG/SVG/JSON/text/ANSI artifacts, and optional recording/video workflows.

More importantly for you, **it is a Rust library, not just a CLI**. Its session, frame, and rendering modules are exposed to callers. The frame structure has public fields, including cell positions and widths, attributes, and cursor information. This makes a direct Ratatui-buffer adapter feasible without launching a PTY for every unit test. Such an adapter is integration work—not a documented built-in Ratatui feature.

But there is a crucial limitation:

> **Terminal Control uses Ghostty to interpret terminal state; it does not use Ghostty’s desktop renderer to produce PNG pixels.**

Its renderer constructs SVG, then rasterizes it through `resvg`/`tiny-skia`. It loads system fonts and draws some terminal graphics using custom geometry. Therefore, “Ghostty-powered” does **not** establish pixel identity with your actual terminal.

**Replacement verdict:** my first candidate for replacing `tui-snap`’s PTY/session infrastructure and possibly its rendering layer. Validate the renderer against your fixtures before standardizing on it.

### `termlens`: particularly relevant for Rust integration tests

`termlens` is considerably more capable than the “minimal PTY harness” description in your repository suggests.

Its documented interface includes screen and style assertions, typed keyboard/mouse interaction, resize, paste and focus handling, cursor/mode inspection, and multiple waiting strategies. It distinguishes content waits, quiet-output waits, stable-screen waits, and synchronized-update frame boundaries. Timeout errors include the screen. It also integrates with `insta`.

It does **not** provide your complete PNG/HTML rendering-and-review workflow. Its optional image decoding concerns terminal graphics payloads; that is not equivalent to rendering an entire terminal screenshot.

**Replacement verdict:** a strong alternative to Terminal Control when the priority is ergonomic, precise Rust integration tests rather than a broad CLI/agent capture platform.

### Other substantial candidates

| Project                                 | Relevant strengths                                                                                                                         | Why it is not a complete replacement                                                                                                                                                                                       |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`ratatui-testlib`**                   | Rust PTY integration testing, snapshot hooks, and specialized Bevy/Sixel-related functionality.                                            | Primarily a test harness, not a complete faithful PNG/HTML review system. ([Docs.rs][3])                                                                                                                                   |
| **`testty`**                            | Rust terminal tests, scenarios, and paired semantic/visual snapshot workflows.                                                             | Its documented visual screenshot path uses VHS, introducing an external rendering pipeline rather than a purely in-process Ratatui renderer. ([Docs.rs][4])                                                                |
| **TermProof**                           | Scenario verification, visual baselines, diffs, and evidence/report generation.                                                            | Its dependency-free native PNG path also paints blocks instead of glyphs. Readable screenshot conversion requires an external path such as `rsvg-convert`. This is a major limitation for your requirement. ([Docs.rs][5]) |
| **Textual + `pytest-textual-snapshot`** | A particularly relevant example of integrated application testing, SVG snapshots, visual failure reports, and explicit updates.            | It belongs to the Python/Textual framework. Borrow its workflow; do not rewrite a Ratatui application merely to obtain it. ([Textual Documentation][6])                                                                    |
| **`reg-cli` / `reg-suit`**              | Compare existing image collections and produce visual HTML reports. They do not care whether the images came from a terminal or a browser. | They need another component to capture/render the terminal. The packaged `reg-cli` workflow requires Node.js.  ([GitHub][7])                                                                                               |

The last row is important: **you do not necessarily need to build the visual review interface either**.

For example, the existing `reg-cli` workflow is:

```bash
reg-cli artifacts/actual artifacts/expected artifacts/diff \
  --report artifacts/report.html \
  --extendedErrors \
  --diffFormat png
```

Its current implementation has a Rust-based engine compiled to WebAssembly. The repository also contains a public native `reg_core::run` interface. Native reuse is worth evaluating for a Rust-only tool, but I have not verified its packaging/API suitability by building an integration. Also, a caller must inspect the comparison report; receiving a successfully generated report is not itself proof that images matched.

---

## 4. The remaining reference projects: what they actually replace

These projects are relevant, but many are **components or adjacent tools**, not substitutes for the entire workflow.

### Still-image rendering and reusable visual artifacts

| Project                           | What it provides                                                                                        | Assessment for your use case                                                                                                                          |
| --------------------------------- | ------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`freeze`**                      | PNG/SVG/WebP generation from code and terminal output, with typography and presentation controls.       | A rendering option, not an application test harness or snapshot-approval system. ([GitHub][8])                                                        |
| **`termshot`**                    | PNG screenshots of command output; uses a PTY and supports saved-output workflows.                      | More than a simple pipe formatter, but its documentation notes cursor-reset limitations. Not a full view-testing system. ([GitHub][9])                |
| **`pamburus/termframe`**          | A Rust noninteractive terminal emulator that runs a command and produces SVG.                           | Useful renderer; does not supply the complete model-fixture, interaction, and approval workflow. ([GitHub][10])                                       |
| **`russmckendrick/terminal-svg`** | Rust terminal emulation and SVG output with embedded font subsets and source metadata for re-rendering. | Particularly relevant to portable artifacts and import/export. It is a separate project from `termframe`, not another name for it. ([GitHub][11])     |
| **`term-transcript`**             | CLI/REPL transcripts exported as SVG and parsed back for testing.                                       | Excellent “visual artifact as test oracle” idea, but its terminal handling is not intended for general cursor-moving fullscreen TUIs. ([Docs.rs][12]) |
| **`anstyle-svg`**                 | Styled-output-to-SVG building block.                                                                    | Possible renderer component, not a complete test framework. ([Docs.rs][13])                                                                           |

`terminal-svg` deserves special attention for your import/export requirement. Its SVGs can retain the original recording/ANSI data and rendering options, allowing later extraction and re-rendering. That is a useful design to borrow. It still does not prove identical rendering everywhere: for example, its documented emoji handling can depend on the viewer’s fonts. ([GitHub][11])

### Interaction and snapshot foundations

| Projects                                | Their role                                                                                                                                                                               |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`expect-test`**                       | Minimal inline/file snapshot assertions. Useful for small expectations, but no terminal renderer or visual report. ([Docs.rs][14])                                                       |
| **`assert_cmd`, `insta-cmd`, `trycmd`** | Command execution and stdout/stderr/exit-result testing. Keep them for CLI behavior; they do not replace fullscreen terminal-state capture. ([Docs.rs][15])                              |
| **`trybuild`**                          | Rust compiler/diagnostic tests. Its “UI tests” are not visual terminal-interface tests. ([Docs.rs][16])                                                                                  |
| **`rexpect`, `expectrl`**               | Script interactive processes by matching their output streams. Useful for prompts and REPLs, but stream matching is not the same as comparing the final rendered screen. ([Docs.rs][17]) |
| **`tui-term`**                          | Embeds a pseudoterminal as a Ratatui widget—the opposite direction from screenshot testing. ([GitHub][18])                                                                               |
| **`ansi-to-tui`, `ansi4tui`**           | Conversion/adaptation of ANSI-styled content into TUI representations. Building blocks rather than capture-and-review systems. ([GitHub][19])                                            |

### Recorders and scripted demonstrations

| Projects                        | Assessment                                                                                                                                                                                                                                                     |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **VHS**                         | More capable than the repository comparison acknowledges: scripted interaction, condition-based waits, PNG screenshots, and ASCII output for golden testing, in addition to video/GIF generation. Still not an in-process Ratatui view library. ([GitHub][20]) |
| **asciinema + agg**             | Session recording/replay plus GIF generation. Valuable evidence and demonstrations; not a model-fixture snapshot-approval system. ([GitHub][21])                                                                                                               |
| **`svg-term-cli`, `termtosvg`** | SVG representations of recorded sessions. `termtosvg` is archived. They do not supply the desired Rust view-test workflow. ([GitHub][22])                                                                                                                      |
| **Terminalizer**                | Recording, GIF generation, and a web player. Primarily a recording/presentation tool. ([GitHub][23])                                                                                                                                                           |
| **`t-rec-rs`**                  | Captures actual visible terminal windows for recordings. Relevant when real desktop appearance matters, but a different approach from headless in-process view testing. ([GitHub][24])                                                                         |
| **`ttygif`, `ttystudio`**       | Recording-to-animation tools, not structured view-snapshot frameworks. ([GitHub][25])                                                                                                                                                                          |
| **`awesome-terminal-recorder`** | A discovery catalog, not an implementation you could adopt. ([GitHub][26])                                                                                                                                                                                     |

Two other entries should not be mistaken for complete competing implementations: the Tessl `testland/tui-snapshot-tester` entry is a skill/recipe using existing framework tools, while the inspected `ratatui-snapshots` package is a namespace reservation without a public API. ([Tessl][27])

Your repository’s `terminal-components-claude` capture scripts and Harness/Scene machinery are **internal lineage**, not another independently packaged competitor. I would treat their useful conventions—fixtures, provenance, artifact bundles—as migration requirements.

---

## 5. What “looks like my terminal” must mean

This is the most important acceptance criterion for whichever renderer you choose.

There are two different goals:

**Deterministic, terminal-like screenshots.**
The same frame always renders using the same font files, palette, cell geometry, cursor policy, and renderer. This is suitable for reliable visual regression tests.

**Verified correspondence with a particular terminal.**
The screenshot is additionally checked against the actual terminal you use, with its font fallback, shaping, spacing, and special-glyph behavior.

I would begin with the first, then validate the second against a representative fixture corpus. Do not accept a renderer’s “pixel-perfect” description as evidence of correspondence with your terminal.

Terminal Control illustrates why: its terminal-state interpretation and pixel generation are separate implementations, and its PNG renderer currently loads system fonts. `tui-snap` likewise uses independently implemented exporters with different geometry.

For your test suite, I would pin a **rendering profile** containing:

| Profile component | What should be fixed                                                          |
| ----------------- | ----------------------------------------------------------------------------- |
| Typography        | Exact font files, fallback order, size, weight, and relevant shaping settings |
| Geometry          | Cell width/height, padding, image scale                                       |
| Colors            | Default foreground/background and indexed palette                             |
| Temporal effects  | Frozen animation state and explicit cursor/blink policy                       |
| Implementation    | Renderer version and a controlled rendering environment                       |

This should be separate from the application’s model fixture.

### HTML should not introduce another rendering discrepancy

My recommendation is that your HTML report display the **authoritative PNGs** for approved, actual, and diff views.

A selectable SVG or styled-cell view can be available alongside them, but the primary visual evidence should not be independently re-rendered by each reviewer’s browser and local fonts.

That gives you portable visual review without requiring every machine to reproduce the exact rasterization environment.

---

## 6. The architecture I would build

```text
ISOLATED VIEW TESTS                         INTERACTIVE TESTS

Fixture model + view state                 Real application executable
             ↓                                          ↓
Actual production view                     PTY + terminal-state engine
             ↓                                          ↓
Ratatui TestBackend                         Captured frame
             └──────────────────┬───────────────────────┘
                                ↓
                      Canonical frame artifact
                                ↓
                ┌───────────────┼────────────────┐
                ↓               ↓                ↓
           Frame JSON       Pinned renderer   ANSI/text
                ↓               ↓             debugging
          Cell comparison       PNG
                                ↓
                       Decoded-pixel comparison
                                ↓
                    HTML expected/actual/diff report
                                ↓
                         Explicit approval
```

### Preserve the full approved state, not just a digest

A useful artifact layout would be:

```text
tests/visual/approved/
  table-selected-dark-120x40.frame.json
  table-selected-dark-120x40.png

artifacts/visual/actual/
  table-selected-dark-120x40.frame.json
  table-selected-dark-120x40.png

artifacts/visual/diff/
  table-selected-dark-120x40.png

artifacts/visual/report.html
```

The canonical frame should preserve grapheme content, cell positions and widths, colors, relevant modifiers, and cursor state. Terminal-protocol details that do not affect pixels—such as hyperlink targets—can remain additional assertions rather than pretending a screenshot verifies them.

Terminal Control’s public frame model is a useful starting point, but it is not automatically a complete universal schema. For example, its inspected underline enumeration contains only a single-underline variant. Adapt or extend the contract according to what your applications actually use.

### Keep cell comparison and pixel comparison separate

I would require both:

**Cell comparison** catches changes in text, placement, colors, styles, and cursor state—even when a deficient renderer happens to draw two states identically.

**Pixel comparison** catches changes in the final image produced by the pinned rendering profile.

Compare decoded image dimensions and pixels for the strict visual gate. Do not make compressed PNG byte identity your only definition of visual equality: `insta`’s binary snapshot comparison is explicitly byte-based. ([Insta Snapshots][2])

This separation also helps classify failures. A renderer upgrade can change pixels without changing the application’s cell output; that should be distinguishable from an application layout regression.

### Define import/export explicitly

“Import PNG or HTML” needs a clear contract:

| Artifact                        | What import should mean                                                                |
| ------------------------------- | -------------------------------------------------------------------------------------- |
| **Canonical JSON**              | Restore the structured frame and render it again offline                               |
| **PNG**                         | Load an approved visual baseline and compare pixels                                    |
| **HTML generated by your tool** | Optionally recover embedded canonical JSON and rendering metadata                      |
| **Arbitrary external HTML**     | Not assumed to be a lossless terminal-frame representation                             |
| **Raw ANSI stream**             | Replay through a real terminal-state engine with explicit initial state and dimensions |

A PNG cannot reliably restore everything that generated it: invisible attributes, hyperlink destinations, original palette references, and underlying grapheme/cell structure are not all recoverable from pixels.

The reusable artifact should therefore be **a structured frame plus its visual rendering**, rather than a screenshot that you later try to reverse-engineer.

### Make failure handling part of the design

Write actual artifacts before asserting. Preserve the approved artifacts. Generate a report on mismatch. Fail when a scenario’s required condition times out. Never silently approve changes in CI.

For interactive tests, prefer an application-specific ready condition or a known complete-frame boundary. Quiet output can be a useful fallback, but it should not be treated as proof that the application reached the intended state. `termlens`’s documented distinctions between these waiting strategies are worth borrowing.

### Test the renderer before trusting it to test your application

I would make this a prerequisite, using fixtures that include:

* Ordinary letters and punctuation; style-only changes; selection and cursor-only changes.
* Box drawing, block characters, Braille, and the Nerd Font icons your applications actually use.
* Wide characters, combining accents, emoji, clipping, and wrapping.
* Multiple themes and viewports, including 80×24, 120×40, and 160×50.

The simplest initial regression test is already revealing: **changing `A` to `B` must change the rendered image**. The current `tui-snap` PNG implementation fails that requirement by construction.

---

## 7. What I would keep, replace, and stop building

| Decision    | Recommendation                                                                                                                                 |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Keep**    | Model/view fixtures, a convenient Ratatui adapter, a shared artifact contract, deterministic rendering profiles, and visual-review integration |
| **Replace** | The hand-written ANSI replay parser, placeholder PNG renderer, hash-only baseline system, and basic PTY orchestration                          |
| **Reuse**   | `TestBackend`, `insta`, Terminal Control or `termlens`, and existing image-diff/report components                                              |
| **Defer**   | Named-session daemons, custom recording formats, video export, another tape language, and other features already implemented by competitors    |

There are two sensible implementation choices.

**For the least custom work:** combine `TestBackend`, a small frame adapter, Terminal Control, `insta`, and an image reporter such as `reg-cli`.

**For a predominantly Rust library workflow:** retain a thin `tui-snap` crate around those interfaces, with a controlled renderer and a small HTML-report integration. Evaluate reusable Rust comparison/report code before implementing another complete review system.

In either case, put renderer fidelity and failure evidence ahead of additional capture commands.

## Bottom line

**I did not find a single verified drop-in tool that already combines every part of your desired Rust/Ratatui workflow. But that does not justify implementing every part again.**

The defensible role for `tui-snap` is:

> **Render the real view from fixture data, preserve its complete frame, generate a trustworthy image, and make every change easy to review and explicitly approve.**

That is worth building where the integration is missing.

A new PTY framework, approximate ANSI parser, placeholder renderer, and custom hash-baseline system are not the parts I would invest in.

[1]: https://ratatui.rs/recipes/testing/snapshots/ "https://ratatui.rs/recipes/testing/snapshots/"
[2]: https://insta.rs/docs/snapshot-types/ "https://insta.rs/docs/snapshot-types/"
[3]: https://docs.rs/ratatui-testlib/latest/ratatui_testlib/ "https://docs.rs/ratatui-testlib/latest/ratatui_testlib/"
[4]: https://docs.rs/testty/0.13.5/testty/ "https://docs.rs/testty/0.13.5/testty/"
[5]: https://docs.rs/termproof/latest/termproof/ "https://docs.rs/termproof/latest/termproof/"
[6]: https://textual.textualize.io/guide/testing/ "https://textual.textualize.io/guide/testing/"
[7]: https://github.com/reg-viz/reg-suit "https://github.com/reg-viz/reg-suit"
[8]: https://github.com/charmbracelet/freeze "https://github.com/charmbracelet/freeze"
[9]: https://github.com/homeport/termshot "https://github.com/homeport/termshot"
[10]: https://github.com/pamburus/termframe "https://github.com/pamburus/termframe"
[11]: https://github.com/russmckendrick/terminal-svg "https://github.com/russmckendrick/terminal-svg"
[12]: https://docs.rs/term-transcript/latest/term_transcript/ "https://docs.rs/term-transcript/latest/term_transcript/"
[13]: https://docs.rs/anstyle-svg/latest/anstyle_svg/ "https://docs.rs/anstyle-svg/latest/anstyle_svg/"
[14]: https://docs.rs/expect-test/latest/expect_test/ "https://docs.rs/expect-test/latest/expect_test/"
[15]: https://docs.rs/assert_cmd/latest/assert_cmd/ "https://docs.rs/assert_cmd/latest/assert_cmd/"
[16]: https://docs.rs/trybuild/latest/trybuild/ "https://docs.rs/trybuild/latest/trybuild/"
[17]: https://docs.rs/rexpect/latest/rexpect/ "https://docs.rs/rexpect/latest/rexpect/"
[18]: https://github.com/a-kenji/tui-term "https://github.com/a-kenji/tui-term"
[19]: https://github.com/ratatui/ansi-to-tui "https://github.com/ratatui/ansi-to-tui"
[20]: https://github.com/charmbracelet/vhs "https://github.com/charmbracelet/vhs"
[21]: https://github.com/asciinema/asciinema "https://github.com/asciinema/asciinema"
[22]: https://github.com/marionebl/svg-term-cli "https://github.com/marionebl/svg-term-cli"
[23]: https://github.com/faressoft/terminalizer "https://github.com/faressoft/terminalizer"
[24]: https://github.com/sassman/t-rec-rs "https://github.com/sassman/t-rec-rs"
[25]: https://github.com/icholy/ttygif "https://github.com/icholy/ttygif"
[26]: https://github.com/orangekame3/awesome-terminal-recorder "https://github.com/orangekame3/awesome-terminal-recorder"
[27]: https://tessl.io/registry/testland/tui-snapshot-tester "https://tessl.io/registry/testland/tui-snapshot-tester"
</research>
