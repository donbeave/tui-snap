//! Approved store: full frames + images, never hash-only baselines.
//!
//! Layout under a store root:
//! ```text
//! approved/<name>.frame.json   approved/<name>.png
//! actual/<name>.frame.json     actual/<name>.png (+ .png.fidelity.json)
//! diff/<name>.png              report.html
//! ```
//!
//! Rules (failure handling is part of the design):
//! - actual artifacts are written BEFORE any assertion — a failing test
//!   still leaves reviewable evidence;
//! - approved artifacts are preserved untouched by `check` (only explicit
//!   [`Store::accept`] replaces them);
//! - a mismatch generates a visual diff PNG, cell diagnostics, and an HTML
//!   report; the gate then fails with artifact paths, not a bare hash;
//! - missing approval fails closed ("new snapshot requires review");
//! - corrupt approval files are explicit errors, never silent defaults;
//! - acceptance is an explicit local command. There is no env-var
//!   auto-bless: CI must never accept snapshots by itself;
//! - all writes are per-name files via atomic tmp+rename, so parallel test
//!   processes updating different names are safe (the index report is
//!   rewritten by whoever finalizes last — data files never clobber).

use crate::diff;
use crate::frame::{Frame, FrameError};
use crate::profile::Profile;
use crate::render;
use std::path::{Path, PathBuf};

/// Snapshot failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotError(pub String);

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "snapshot error: {}", self.0)
    }
}

impl std::error::Error for SnapshotError {}

impl From<FrameError> for SnapshotError {
    fn from(e: FrameError) -> Self {
        SnapshotError(e.to_string())
    }
}

impl From<crate::render::RenderError> for SnapshotError {
    fn from(e: crate::render::RenderError) -> Self {
        SnapshotError(e.to_string())
    }
}

impl From<crate::diff::DiffError> for SnapshotError {
    fn from(e: crate::diff::DiffError) -> Self {
        SnapshotError(e.to_string())
    }
}

/// Gate status for one named snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Matched,
    CellsDiffer,
    PixelsDiffer,
    DimensionMismatch,
    MissingApproval,
    CorruptApproval,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Matched => "matched",
            Status::CellsDiffer => "cells-differ",
            Status::PixelsDiffer => "pixels-differ",
            Status::DimensionMismatch => "dimension-mismatch",
            Status::MissingApproval => "missing-approval",
            Status::CorruptApproval => "corrupt-approval",
        }
    }

    #[must_use]
    pub fn matched(self) -> bool {
        matches!(self, Status::Matched)
    }
}

/// One differing cell, summarized for humans.
#[derive(Debug, Clone)]
pub struct CellDiff {
    pub x: u16,
    pub y: u16,
    pub expected: String,
    pub actual: String,
}

/// Cap stored per-cell diagnostics (the total is always counted).
pub const MAX_CELL_DIFFS: usize = 100;

/// Outcome of one `check`. Artifacts on disk even when unmatched.
#[derive(Debug, Clone)]
pub struct CompareOutcome {
    pub name: String,
    pub status: Status,
    pub cell_diffs: Vec<CellDiff>,
    pub cell_diff_total: usize,
    pub pixel_score: Option<f64>,
    pub approved_png_regenerated: bool,
    pub digest_expected: Option<String>,
    pub digest_actual: String,
    /// Extra context (e.g. why an approval file is corrupt).
    pub actual_frame: PathBuf,
    pub actual_png: PathBuf,
    pub expected_frame: PathBuf,
    /// The approved PNG path, but only when it actually exists on disk
    /// (when it was regenerated in memory this is `None` — see
    /// [`Self::expected_png_bytes`] for the image either way).
    pub expected_png: Option<PathBuf>,
    /// The exact expected image the pixel gate compared against — the
    /// approved PNG's bytes when on disk, the regenerated render otherwise.
    /// `None` only when there is no usable approved frame (missing/corrupt
    /// approval). Reports embed this so the expected panel always shows the
    /// gated image.
    pub expected_png_bytes: Option<Vec<u8>>,
    pub diff_png: Option<PathBuf>,
    pub note: String,
}

impl CompareOutcome {
    /// Fail with an actionable message (artifact paths + first diagnostics).
    pub fn ensure_matched(&self) -> Result<(), SnapshotError> {
        if self.status.matched() {
            return Ok(());
        }
        let mut msg = format!(
            "snapshot `{}` requires review ({}).",
            self.name,
            self.status.as_str()
        );
        msg.push_str(&format!(
            "\n  actual:   {} {}",
            self.actual_frame.display(),
            self.actual_png.display()
        ));
        if let Some(p) = &self.expected_png {
            msg.push_str(&format!("\n  expected: {}", p.display()));
        }
        if let Some(p) = &self.diff_png {
            msg.push_str(&format!("\n  diff:     {}", p.display()));
        }
        if self.cell_diff_total > 0 {
            msg.push_str(&format!(
                "\n  {} differing cell(s), first {}:",
                self.cell_diff_total,
                self.cell_diffs.len()
            ));
            for d in &self.cell_diffs {
                msg.push_str(&format!(
                    "\n    ({},{}): expected {} | actual {}",
                    d.x, d.y, d.expected, d.actual
                ));
            }
        }
        if let Some(s) = self.pixel_score {
            msg.push_str(&format!("\n  pixel similarity: {s:.6}"));
        }
        if !self.note.is_empty() {
            msg.push_str(&format!("\n  note: {}", self.note));
        }
        msg.push_str(&format!(
            "\n  review the report, then accept explicitly: tuisnap accept {}",
            self.name
        ));
        Err(SnapshotError(msg))
    }
}

fn summarize(cell: &crate::frame::Cell) -> String {
    if cell.continuation {
        return "…".to_string();
    }
    let (fg, bg) = Frame::resolve_cell(
        cell,
        crate::frame::Rgb::new(0xd0, 0xd0, 0xd0),
        crate::frame::Rgb::new(0, 0, 0),
    );
    let mut mods = String::new();
    if cell.mods.hidden {
        mods.push_str("+hidden");
    }
    if cell.mods.blink {
        mods.push_str("+blink");
    }
    if cell.mods.bold {
        mods.push_str("+bold");
    }
    if cell.mods.dim {
        mods.push_str("+dim");
    }
    if cell.mods.italic {
        mods.push_str("+italic");
    }
    if cell.mods.underline {
        mods.push_str("+ul");
    }
    if cell.mods.strikethrough {
        mods.push_str("+strike");
    }
    if cell.mods.reverse {
        mods.push_str("+rev");
    }
    format!(
        "{:?} fg={} bg={}{mods}",
        cell.symbol,
        fg.to_hex(),
        bg.to_hex()
    )
}

/// Atomic file write (tmp in same dir + rename). Tmp names carry pid, a
/// process-wide counter, and the thread id, so same-name writers from
/// different threads never share a tmp file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| SnapshotError(format!("cannot create {}: {e}", dir.display())))?;
        }
    }
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!(
        "tmp.{}.{n}.{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&tmp, bytes)
        .map_err(|e| SnapshotError(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| SnapshotError(format!("cannot publish {}: {e}", path.display())))?;
    Ok(())
}

/// Approved/actual/diff artifact store.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Store root (approved/actual/diff/report.html live beneath it).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn approved_frame(&self, name: &str) -> PathBuf {
        self.root
            .join("approved")
            .join(format!("{name}.frame.json"))
    }

    fn approved_png(&self, name: &str) -> PathBuf {
        self.root.join("approved").join(format!("{name}.png"))
    }

    fn actual_frame(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.frame.json"))
    }

    fn actual_png(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.png"))
    }

    /// Missing-glyph sidecar next to a PNG (`<name>.png.fidelity.json`).
    fn fidelity_sidecar(png: &Path) -> PathBuf {
        png.with_extension("png.fidelity.json")
    }

    fn diff_png(&self, name: &str) -> PathBuf {
        self.root.join("diff").join(format!("{name}.png"))
    }

    /// Names with actual frames (for `--all` acceptance).
    pub fn actual_names(&self) -> Result<Vec<String>, SnapshotError> {
        let dir = self.root.join("actual");
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(base) = name.strip_suffix(".frame") {
                        out.push(base.to_string());
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Check one actual frame against approval. Writes actual artifacts
    /// BEFORE comparing; on mismatch also writes the diff PNG. The actual
    /// PNG is paired with a `<name>.png.fidelity.json` sidecar listing any
    /// glyphs the font chain did not cover (never silent tofu).
    ///
    /// `pixel_threshold`: strict gates pass 1.0; review passes lower it
    /// explicitly. Dimensions must match exactly either way.
    ///
    /// This constructs a fresh [`render::Renderer`] per call; bulk gates
    /// should build one and call [`Self::check_with`] instead.
    pub fn check(
        &self,
        name: &str,
        actual: &Frame,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        let mut renderer = render::Renderer::new(profile, faces)?;
        self.check_with(&mut renderer, name, actual, pixel_threshold)
    }

    /// [`Self::check`] through a caller-owned [`render::Renderer`], so a
    /// suite reuses the parsed faces and the glyph cache across checks.
    pub fn check_with(
        &self,
        renderer: &mut render::Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        actual.validate().map_err(SnapshotError::from)?;
        let rendered = renderer.render(actual).map_err(SnapshotError::from)?;
        let actual_png_bytes = &rendered.png;
        let actual_frame_path = self.actual_frame(name);
        let actual_png_path = self.actual_png(name);
        write_atomic(&actual_frame_path, actual.to_json().as_bytes())?;
        write_atomic(&actual_png_path, actual_png_bytes)?;
        write_atomic(
            &Self::fidelity_sidecar(&actual_png_path),
            rendered.fidelity.to_json().as_bytes(),
        )?;

        let approved_frame_path = self.approved_frame(name);
        let approved_png_path = self.approved_png(name);
        let digest_actual = format!("{:016x}", actual.digest());
        let mut outcome = CompareOutcome {
            name: name.to_string(),
            status: Status::MissingApproval,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: None,
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual,
            actual_frame: actual_frame_path.clone(),
            actual_png: actual_png_path,
            expected_frame: approved_frame_path.clone(),
            expected_png: None,
            expected_png_bytes: None,
            diff_png: None,
            note: String::new(),
        };

        let approved_text = match std::fs::read_to_string(&approved_frame_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(outcome),
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_frame_path.display()
                )));
            }
        };
        let approved = match Frame::from_json(&approved_text) {
            Ok(f) => f,
            Err(e) => {
                outcome.status = Status::CorruptApproval;
                outcome.note = format!(
                    "approved file {} is corrupt: {e}",
                    approved_frame_path.display()
                );
                return Ok(outcome);
            }
        };
        outcome.digest_expected = Some(format!("{:016x}", approved.digest()));

        // Cell comparison (exact; dimension mismatch is a status, not a diff).
        match actual.diff_cells(&approved) {
            Err(_) => {
                outcome.status = Status::DimensionMismatch;
            }
            Ok(positions) => {
                outcome.cell_diff_total = positions.len();
                // Cursor-only change: every reported position holds equal
                // cells, so cell summaries would print identical text twice.
                // Say what actually changed instead.
                let cells_equal = positions.iter().all(|(x, y)| {
                    approved.get(*x, *y).map(summarize) == actual.get(*x, *y).map(summarize)
                });
                if cells_equal && outcome.cell_diff_total > 0 {
                    outcome.cell_diffs.push(CellDiff {
                        x: actual.cursor.x,
                        y: actual.cursor.y,
                        expected: Frame::summarize_cursor(&approved.cursor),
                        actual: Frame::summarize_cursor(&actual.cursor),
                    });
                } else {
                    for (x, y) in positions.into_iter().take(MAX_CELL_DIFFS) {
                        let e = approved
                            .get(x, y)
                            .map(summarize)
                            .unwrap_or_else(|| "∅".into());
                        let a = actual
                            .get(x, y)
                            .map(summarize)
                            .unwrap_or_else(|| "∅".into());
                        outcome.cell_diffs.push(CellDiff {
                            x,
                            y,
                            expected: e,
                            actual: a,
                        });
                    }
                }
                if outcome.cell_diff_total > 0 {
                    outcome.status = Status::CellsDiffer;
                }
            }
        }

        // Pixel comparison over decoded PNGs. A missing approved PNG is
        // rendered to MEMORY only: `check` never writes under `approved/`
        // (approvals change solely through explicit `accept`), so parallel
        // gates cannot race on approval files and renderer upgrades cannot
        // silently heal them. The gated bytes are kept on the outcome
        // (`expected_png_bytes`) so reports show the expected image even
        // though nothing exists at `expected_png`'s former disk path.
        let (approved_png_bytes, png_on_disk) = match std::fs::read(&approved_png_path) {
            Ok(b) => (b, true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let rendered = renderer.render(&approved)?;
                outcome.approved_png_regenerated = true;
                outcome.note = "approved PNG not on disk; expected image regenerated in memory \
                                from the approved frame"
                    .to_string();
                (rendered.png, false)
            }
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_png_path.display()
                )));
            }
        };
        if png_on_disk {
            outcome.expected_png = Some(approved_png_path);
        }
        let verdict = diff::compare_png(&approved_png_bytes, actual_png_bytes)?;
        outcome.expected_png_bytes = Some(approved_png_bytes);
        if !verdict.dims_equal {
            outcome.status = Status::DimensionMismatch;
        } else {
            outcome.pixel_score = Some(verdict.score);
            if verdict.score < pixel_threshold
                && !matches!(
                    outcome.status,
                    Status::CellsDiffer | Status::DimensionMismatch
                )
            {
                outcome.status = Status::PixelsDiffer;
            }
            if verdict.score < 1.0 {
                let path = self.diff_png(name);
                write_atomic(&path, &verdict.diff_png)?;
                outcome.diff_png = Some(path);
            }
        }

        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::Matched;
        }
        Ok(outcome)
    }

    /// Explicitly approve one snapshot: actual → approved (atomic).
    /// There is deliberately no environment-variable auto-accept.
    pub fn accept(&self, name: &str) -> Result<(), SnapshotError> {
        for (src, dst) in [
            (self.actual_frame(name), self.approved_frame(name)),
            (self.actual_png(name), self.approved_png(name)),
        ] {
            let bytes = std::fs::read(&src).map_err(|e| {
                SnapshotError(format!(
                    "nothing to accept for `{name}` ({}: {e})",
                    src.display()
                ))
            })?;
            if src.ends_with(".json") || src.extension().and_then(|s| s.to_str()) == Some("json") {
                let text = String::from_utf8(bytes).map_err(|e| {
                    SnapshotError(format!("actual frame for `{name}` is not UTF-8: {e}"))
                })?;
                Frame::from_json(&text).map_err(|e| {
                    SnapshotError(format!("actual frame for `{name}` invalid, refusing: {e}"))
                })?;
                write_atomic(&dst, text.as_bytes())?;
            } else {
                write_atomic(&dst, &bytes)?;
            }
        }
        // Pair the missing-glyph sidecar when the check produced one.
        let (src, dst) = (
            Self::fidelity_sidecar(&self.actual_png(name)),
            Self::fidelity_sidecar(&self.approved_png(name)),
        );
        if let Ok(bytes) = std::fs::read(&src) {
            write_atomic(&dst, &bytes)?;
        }
        Ok(())
    }

    /// Assemble one report row from a check outcome: reads the actual
    /// artifacts and base64-embeds the PNGs. The expected panel uses
    /// [`CompareOutcome::expected_png_bytes`] — the exact image the pixel
    /// gate compared against — so it renders even when the approved PNG was
    /// regenerated in memory rather than read from disk.
    pub fn report_entry(
        &self,
        outcome: &CompareOutcome,
        profile: &Profile,
    ) -> Result<ReportEntry, SnapshotError> {
        report_entry(outcome, profile)
    }

    /// Re-verify every actual frame in the store and rewrite `report.html` —
    /// the library form of the CLI `report` subcommand. Unmatched gates do
    /// not error here: inspect [`StoreReport::failed`] and the outcomes.
    ///
    /// This constructs a fresh [`render::Renderer`] per call; bulk callers
    /// should build one and use [`Self::report_with`].
    pub fn report(
        &self,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        let mut renderer = render::Renderer::new(profile, faces)?;
        self.report_with(&mut renderer, pixel_threshold, title)
    }

    /// [`Self::report`] through a caller-owned [`render::Renderer`].
    pub fn report_with(
        &self,
        renderer: &mut render::Renderer,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        // A store with no actuals yet yields an empty report (CLI parity),
        // while a genuinely unreadable directory stays an error.
        let names = match self.actual_names() {
            Ok(names) => names,
            Err(_) if !self.root.join("actual").exists() => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut entries = Vec::new();
        let mut outcomes = Vec::new();
        for name in names {
            let text = std::fs::read_to_string(self.actual_frame(&name)).map_err(|e| {
                SnapshotError(format!("cannot read actual frame for `{name}`: {e}"))
            })?;
            let frame = Frame::from_json(&text)?;
            let outcome = self.check_with(renderer, &name, &frame, pixel_threshold)?;
            entries.push(self.report_entry(&outcome, renderer.profile())?);
            outcomes.push(outcome);
        }
        let path = write_report(self, title, &entries)?;
        Ok(StoreReport { path, outcomes })
    }
}

/// Assemble one report row from a check outcome: reads the actual artifacts
/// and base64-embeds the PNGs. Free-function form of [`Store::report_entry`]
/// so non-classic stores (e.g. [`crate::grouped::GroupedStore`]) can build
/// rows for [`write_report_at`] without a [`Store`].
pub fn report_entry(
    outcome: &CompareOutcome,
    profile: &Profile,
) -> Result<ReportEntry, SnapshotError> {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;
    let actual_png = std::fs::read(&outcome.actual_png).map_err(|e| {
        SnapshotError(format!(
            "cannot read actual PNG {}: {e}",
            outcome.actual_png.display()
        ))
    })?;
    let actual_compact = std::fs::read_to_string(&outcome.actual_frame).map_err(|e| {
        SnapshotError(format!(
            "cannot read actual frame {}: {e}",
            outcome.actual_frame.display()
        ))
    })?;
    let actual_frame = Frame::from_json(&actual_compact)?;
    Ok(ReportEntry {
        outcome: outcome.clone(),
        expected_png_b64: outcome.expected_png_bytes.as_ref().map(|b| b64.encode(b)),
        actual_png_b64: b64.encode(&actual_png),
        diff_png_b64: outcome
            .diff_png
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| b64.encode(&b)),
        expected_frame_json: std::fs::read_to_string(&outcome.expected_frame).ok(),
        actual_frame_json: actual_frame.to_json_pretty(),
        actual_frame_compact: actual_compact,
        profile_desc: profile.name.clone(),
        font_sha256: profile.font_sha256.clone(),
    })
}

/// Result of [`Store::report`]/[`Store::report_with`]: the rewritten report
/// plus every outcome it embeds.
#[derive(Debug)]
pub struct StoreReport {
    /// Path of the rewritten `report.html`.
    pub path: PathBuf,
    /// One outcome per re-verified actual, in name order.
    pub outcomes: Vec<CompareOutcome>,
}

impl StoreReport {
    /// Outcomes that did not match (the CLI turns this into a non-zero exit).
    #[must_use]
    pub fn failed(&self) -> usize {
        self.outcomes.iter().filter(|o| !o.status.matched()).count()
    }
}

/// One row of the portable HTML report.
pub struct ReportEntry {
    pub outcome: CompareOutcome,
    pub expected_png_b64: Option<String>,
    pub actual_png_b64: String,
    pub diff_png_b64: Option<String>,
    pub expected_frame_json: Option<String>,
    /// Human-readable pretty JSON for the report `<pre>`.
    pub actual_frame_json: String,
    /// Compact canonical JSON embedded for lossless re-import.
    pub actual_frame_compact: String,
    pub profile_desc: String,
    pub font_sha256: String,
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JSON embedded in `<script type="application/json">`: escape `<` so a cell
/// symbol like `</script>` cannot terminate the element (still valid JSON —
/// `\u003c` re-parses to `<`, keeping lossless re-import).
pub fn json_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// Write a portable single-file report: authoritative PNGs embedded as
/// base64 (no per-viewer font dependence), frame JSON embedded for lossless
/// re-import, cell diagnostics as a table.
pub fn write_report(
    store: &Store,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    write_report_at(&store.root.join("report.html"), title, entries)
}

/// [`write_report`] with an explicit output path, for stores whose report
/// does not live at a fixed location (e.g. [`crate::grouped::GroupedStore`],
/// which keeps its report out of the approved tree).
pub fn write_report_at(
    path: &Path,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    let mut body = String::new();
    for e in entries {
        let o = &e.outcome;
        body.push_str(&format!(
            "<section><h2>{} — {}</h2>\n",
            esc_html(&o.name),
            o.status.as_str()
        ));
        body.push_str("<div class=\"imgs\">");
        if let Some(b) = &e.expected_png_b64 {
            body.push_str(&format!(
                "<figure><figcaption>expected</figcaption><img src=\"data:image/png;base64,{b}\" alt=\"expected\"></figure>"
            ));
        } else {
            body.push_str(
                "<figure><figcaption>expected</figcaption><p>missing approval</p></figure>",
            );
        }
        body.push_str(&format!(
            "<figure><figcaption>actual</figcaption><img src=\"data:image/png;base64,{}\" alt=\"actual\"></figure>",
            e.actual_png_b64
        ));
        if let Some(b) = &e.diff_png_b64 {
            body.push_str(&format!(
                "<figure><figcaption>diff</figcaption><img src=\"data:image/png;base64,{b}\" alt=\"diff\"></figure>"
            ));
        }
        body.push_str("</div>");
        if o.cell_diff_total > 0 {
            body.push_str(&format!(
                "<p>{} differing cell(s):</p><table><tr><th>x</th><th>y</th><th>expected</th><th>actual</th></tr>",
                o.cell_diff_total
            ));
            for d in &o.cell_diffs {
                body.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    d.x,
                    d.y,
                    esc_html(&d.expected),
                    esc_html(&d.actual)
                ));
            }
            body.push_str("</table>");
        }
        if let Some(s) = o.pixel_score {
            body.push_str(&format!("<p>pixel similarity: {s:.6}</p>"));
        }
        body.push_str(&format!(
            "<details><summary>actual frame.json</summary><script type=\"application/json\" id=\"actual-{}\">{}</script><pre>{}</pre></details>",
            esc_html(&o.name),
            json_for_script(&e.actual_frame_compact),
            esc_html(&e.actual_frame_json)
        ));
        if let Some(j) = &e.expected_frame_json {
            body.push_str(&format!(
                "<details><summary>expected frame.json</summary><pre>{}</pre></details>",
                esc_html(j)
            ));
        }
        body.push_str("</section>");
    }
    let profile_line = entries
        .first()
        .map(|e| {
            format!(
                "<p>profile: {} · font sha256: {}</p>",
                esc_html(&e.profile_desc),
                esc_html(&e.font_sha256)
            )
        })
        .unwrap_or_default();
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:system-ui,sans-serif;background:#141414;color:#eee;margin:24px}}section{{border:1px solid #444;margin:16px 0;padding:16px}}img{{max-width:100%;image-rendering:pixelated}}table{{border-collapse:collapse}}td,th{{border:1px solid #555;padding:2px 8px;font-family:monospace}}pre{{white-space:pre-wrap}}</style></head><body><h1>{}</h1>{profile_line}{body}</body></html>",
        esc_html(title),
        esc_html(title)
    );
    write_atomic(path, html.as_bytes())?;
    Ok(path.to_path_buf())
}
