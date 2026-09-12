//! Grouped multi-artifact snapshot store: one directory tree per suite,
//! nested scenario names, four committed artifacts per scenario.
//!
//! A scenario name is a slash-separated path like
//! `showcase/pages/overview_120x40_truecolor`. Each scenario commits exactly
//! these four artifacts under the approved root:
//!
//! ```text
//! <approved>/<name>.ansi   colored terminal text (normalized SGR dump)
//! <approved>/<name>.txt    plain black-and-white text
//! <approved>/<name>.png    colored image (authoritative pixel gate)
//! <approved>/<name>.html   standalone colored HTML render
//! ```
//!
//! The approved tree holds NOTHING else — no `.frame.json`, no `.cursor`
//! sidecars. Scratch state lives outside the approved root:
//!
//! ```text
//! <actual>/<name>.{ansi,txt,png,html}   latest capture (written BEFORE any assertion)
//! <actual>/<name>.frame.json            debug sidecar (report re-verification)
//! <actual>/<name>.png.fidelity.json     missing-glyph sidecar
//! <actual>/report.html                  portable expected/actual/diff report
//! <diff>/<name>.png                     red-overlay diff, on mismatch
//! ```
//!
//! Gates ([`GroupedStore::check_with`]):
//! - `.ansi` byte-compare — the cell-exact gate (symbol + fg + bg + mods per
//!   cell, deterministic);
//! - `.txt` byte-compare — content gate (a style-only change shows as
//!   ansi=false/txt=true);
//! - `.html` byte-compare — the render-level gate (identical cells with a
//!   changed renderer/font fail here; the embedded frame JSON normalizes the
//!   provenance timestamp, see [`crate::render::Renderer::render_html`]);
//! - `.png` decoded-pixel compare with the same threshold semantics as
//!   [`crate::snapshot::Store::check_with`] — dimensions must match exactly,
//!   `score >= pixel_threshold` to pass, diff PNG written on any
//!   sub-1.0 score.
//!
//! Statuses reuse [`Status`]: cell-gate failures read as
//! [`Status::CellsDiffer`], render-level/pixel failures as
//! [`Status::PixelsDiffer`], any missing approved artifact as
//! [`Status::MissingApproval`] (fail-closed, never silently). Approvals
//! change solely through explicit [`GroupedStore::accept`] /
//! [`GroupedStore::accept_all`] — there is no env-var auto-bless.
//!
//! Default roots for an approved root `snapshots/`: actual `snapshots.actual/`,
//! diff `snapshots.diff/`, report `snapshots.actual/report.html` — siblings,
//! so the committed tree stays clean. Override with
//! [`GroupedStore::with_actual_root`], [`GroupedStore::with_diff_root`] and
//! [`GroupedStore::with_report_path`] (e.g. under `target/`).

use crate::diff;
use crate::frame::Frame;
use crate::profile::Profile;
use crate::render::Renderer;
use crate::snapshot::{
    report_entry, write_atomic, write_report_at, CompareOutcome, SnapshotError, Status, StoreReport,
};
use std::path::{Path, PathBuf};

/// Invalid scenario name: rejected before any path is built from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidName(pub String);

impl std::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid snapshot name: {}", self.0)
    }
}

impl std::error::Error for InvalidName {}

impl From<InvalidName> for SnapshotError {
    fn from(e: InvalidName) -> Self {
        SnapshotError(e.to_string())
    }
}

/// Names are relative `/`-separated paths: no absolute paths, no `..` or
/// `.` segments, no empty segments, no backslashes. Anything else would
/// escape the store roots or fail to round-trip through recursive listing.
pub fn validate_name(name: &str) -> Result<(), InvalidName> {
    let bad = |m: &str| InvalidName(format!("{name:?}: {m}"));
    if name.is_empty() {
        return Err(bad("empty name"));
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return Err(bad("absolute paths are not allowed"));
    }
    if name.contains('\\') {
        return Err(bad("backslashes are not allowed (use `/` separators)"));
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err(bad("empty path segment"));
        }
        if seg == ".." {
            return Err(bad("`..` segments are not allowed"));
        }
        if seg == "." {
            return Err(bad("`.` segments are not allowed"));
        }
    }
    Ok(())
}

/// The on-disk artifact paths of one scenario under one root.
#[derive(Debug, Clone)]
pub struct ArtifactPaths {
    pub ansi: PathBuf,
    pub txt: PathBuf,
    pub png: PathBuf,
    pub html: PathBuf,
    /// Canonical frame sidecar (scratch roots only — never approved state).
    pub frame_json: PathBuf,
}

/// `root/<name>.<ext>` for the four artifacts plus the frame sidecar.
/// `name` must be pre-validated ([`validate_name`]).
fn artifact_paths(root: &Path, name: &str) -> ArtifactPaths {
    ArtifactPaths {
        ansi: root.join(format!("{name}.ansi")),
        txt: root.join(format!("{name}.txt")),
        png: root.join(format!("{name}.png")),
        html: root.join(format!("{name}.html")),
        frame_json: root.join(format!("{name}.frame.json")),
    }
}

/// Missing-glyph sidecar next to a PNG (`<name>.png.fidelity.json`).
fn fidelity_sidecar(png: &Path) -> PathBuf {
    png.with_extension("png.fidelity.json")
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, SnapshotError> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SnapshotError(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

/// Locate the first differing byte of two blobs, for human diagnostics.
fn first_difference(approved: &[u8], actual: &[u8]) -> String {
    let n = approved.len().min(actual.len());
    let mut i = 0;
    while i < n && approved[i] == actual[i] {
        i += 1;
    }
    let line = approved[..i].iter().filter(|&&c| c == b'\n').count() + 1;
    format!(
        "first difference at byte {i} (approved line {line}); approved {} bytes, actual {} bytes",
        approved.len(),
        actual.len()
    )
}

/// Outcome of one grouped [`GroupedStore::check_with`]. The shared
/// [`CompareOutcome`] carries status, pixel score, diff path and the paths
/// the report machinery reads; the per-artifact booleans record each byte
/// gate individually (`None` = the approved artifact is missing).
#[derive(Debug, Clone)]
pub struct GroupedOutcome {
    pub outcome: CompareOutcome,
    /// `.ansi` byte gate (the cell-exact gate).
    pub ansi_match: Option<bool>,
    /// `.txt` byte gate.
    pub txt_match: Option<bool>,
    /// `.html` byte gate (the render-level gate).
    pub html_match: Option<bool>,
    /// Where the actual artifacts were written.
    pub actual: ArtifactPaths,
    /// Where the approved artifacts live (some may not exist yet).
    pub approved: ArtifactPaths,
}

impl GroupedOutcome {
    #[must_use]
    pub fn status(&self) -> Status {
        self.outcome.status
    }

    #[must_use]
    pub fn matched(&self) -> bool {
        self.outcome.status.matched()
    }

    /// Fail with an actionable message (artifact paths + which gates fell).
    pub fn ensure_matched(&self) -> Result<(), SnapshotError> {
        self.outcome.ensure_matched()
    }
}

/// Grouped approved/actual/diff artifact store. See the module docs for the
/// layout and gate semantics.
#[derive(Debug, Clone)]
pub struct GroupedStore {
    approved_root: PathBuf,
    actual_root: PathBuf,
    diff_root: PathBuf,
    report_path: Option<PathBuf>,
}

/// `root` with `suffix` appended to its last component
/// (`snapshots` + `.actual` → `snapshots.actual`).
fn sibling(root: &Path, suffix: &str) -> PathBuf {
    match root.file_name() {
        Some(f) => root.with_file_name(format!("{}{suffix}", f.to_string_lossy())),
        None => PathBuf::from(format!("{}{suffix}", root.display())),
    }
}

impl GroupedStore {
    /// Approved root `snapshots/` defaults to actual `snapshots.actual/`,
    /// diff `snapshots.diff/`, report `snapshots.actual/report.html`.
    #[must_use]
    pub fn new(approved_root: &Path) -> Self {
        Self {
            approved_root: approved_root.to_path_buf(),
            actual_root: sibling(approved_root, ".actual"),
            diff_root: sibling(approved_root, ".diff"),
            report_path: None,
        }
    }

    #[must_use]
    pub fn with_actual_root(mut self, root: &Path) -> Self {
        self.actual_root = root.to_path_buf();
        self
    }

    #[must_use]
    pub fn with_diff_root(mut self, root: &Path) -> Self {
        self.diff_root = root.to_path_buf();
        self
    }

    /// Explicit report path. Consumers typically point this under `target/`
    /// or the store scratch area — never inside the approved tree.
    #[must_use]
    pub fn with_report_path(mut self, path: &Path) -> Self {
        self.report_path = Some(path.to_path_buf());
        self
    }

    #[must_use]
    pub fn approved_root(&self) -> &Path {
        &self.approved_root
    }

    #[must_use]
    pub fn actual_root(&self) -> &Path {
        &self.actual_root
    }

    #[must_use]
    pub fn diff_root(&self) -> &Path {
        &self.diff_root
    }

    /// The configured report path, defaulting to `<actual_root>/report.html`.
    #[must_use]
    pub fn report_path(&self) -> PathBuf {
        self.report_path
            .clone()
            .unwrap_or_else(|| self.actual_root.join("report.html"))
    }

    /// Scenario names with actual artifacts, recursively, sorted. An absent
    /// actual root lists nothing (a fresh store is empty, not an error).
    pub fn actual_names(&self) -> Result<Vec<String>, SnapshotError> {
        list_names(&self.actual_root)
    }

    /// Scenario names with approved artifacts, recursively, sorted.
    pub fn approved_names(&self) -> Result<Vec<String>, SnapshotError> {
        list_names(&self.approved_root)
    }

    /// Check one frame against the approved artifacts. Writes the four
    /// actual artifacts (plus debug sidecars) under the actual root BEFORE
    /// comparing; on pixel mismatch also writes the diff PNG under the diff
    /// root. Gate semantics are in the module docs.
    ///
    /// This constructs a fresh [`Renderer`] per call; bulk gates should
    /// build one and call [`Self::check_with`] instead.
    pub fn check(
        &self,
        name: &str,
        actual: &Frame,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
    ) -> Result<GroupedOutcome, SnapshotError> {
        let mut renderer = Renderer::new(profile, faces)?;
        self.check_with(&mut renderer, name, actual, pixel_threshold)
    }

    /// [`Self::check`] through a caller-owned [`Renderer`], so a suite
    /// reuses the parsed faces and the glyph cache across checks.
    pub fn check_with(
        &self,
        renderer: &mut Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<GroupedOutcome, SnapshotError> {
        validate_name(name)?;
        actual.validate().map_err(SnapshotError::from)?;
        let artifacts = renderer
            .render_artifacts(actual, name)
            .map_err(SnapshotError::from)?;

        let actual_paths = artifact_paths(&self.actual_root, name);
        let approved_paths = artifact_paths(&self.approved_root, name);
        // Actuals first: a failing gate still leaves reviewable evidence.
        write_atomic(&actual_paths.ansi, artifacts.ansi.as_bytes())?;
        write_atomic(&actual_paths.txt, artifacts.txt.as_bytes())?;
        write_atomic(&actual_paths.png, &artifacts.png)?;
        write_atomic(&actual_paths.html, artifacts.html.as_bytes())?;
        write_atomic(&actual_paths.frame_json, actual.to_json().as_bytes())?;
        write_atomic(
            &fidelity_sidecar(&actual_paths.png),
            artifacts.fidelity.to_json().as_bytes(),
        )?;

        let outcome = CompareOutcome {
            name: name.to_string(),
            status: Status::MissingApproval,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: None,
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual: format!("{:016x}", actual.digest()),
            actual_frame: actual_paths.frame_json.clone(),
            actual_png: actual_paths.png.clone(),
            // Never exists in a conforming approved tree (four artifacts
            // only); reports simply omit the expected-frame panel.
            expected_frame: approved_paths.frame_json.clone(),
            expected_png: None,
            expected_png_bytes: None,
            diff_png: None,
            note: String::new(),
        };
        let mut grouped = GroupedOutcome {
            outcome,
            ansi_match: None,
            txt_match: None,
            html_match: None,
            actual: actual_paths,
            approved: approved_paths,
        };
        let outcome = &mut grouped.outcome;

        // Missing ANY approved artifact fails closed as MissingApproval.
        let approved_ansi = read_optional(&grouped.approved.ansi)?;
        let approved_txt = read_optional(&grouped.approved.txt)?;
        let approved_html = read_optional(&grouped.approved.html)?;
        let approved_png = read_optional(&grouped.approved.png)?;
        let mut missing = Vec::new();
        if approved_ansi.is_none() {
            missing.push(".ansi");
        }
        if approved_txt.is_none() {
            missing.push(".txt");
        }
        if approved_html.is_none() {
            missing.push(".html");
        }
        if approved_png.is_none() {
            missing.push(".png");
        }
        if !missing.is_empty() {
            outcome.note = format!(
                "missing approved artifact(s) for `{name}`: {}",
                missing.join(" ")
            );
            return Ok(grouped);
        }
        let (approved_ansi, approved_txt, approved_html, approved_png) = (
            approved_ansi.expect("checked above"),
            approved_txt.expect("checked above"),
            approved_html.expect("checked above"),
            approved_png.expect("checked above"),
        );
        outcome.expected_png = Some(grouped.approved.png.clone());
        outcome.expected_png_bytes = Some(approved_png.clone());

        let mut notes: Vec<String> = Vec::new();

        // ANSI byte gate: the cell-exact comparison (symbol+fg+bg+mods).
        let ansi_equal = approved_ansi == artifacts.ansi.as_bytes();
        grouped.ansi_match = Some(ansi_equal);
        if !ansi_equal {
            outcome.status = Status::CellsDiffer;
            notes.push(format!(
                "ansi differs (cell-exact gate): {}",
                first_difference(&approved_ansi, artifacts.ansi.as_bytes())
            ));
        }

        // TXT byte gate: content only (style-only changes keep txt equal).
        let txt_equal = approved_txt == artifacts.txt.as_bytes();
        grouped.txt_match = Some(txt_equal);
        if !txt_equal {
            if matches!(outcome.status, Status::MissingApproval) {
                outcome.status = Status::CellsDiffer;
            }
            notes.push(format!(
                "txt differs: {}",
                first_difference(&approved_txt, artifacts.txt.as_bytes())
            ));
        }

        // HTML byte gate: identical cells with a changed renderer/font fail
        // here — a render-level event, reported as PixelsDiffer.
        let html_equal = approved_html == artifacts.html.as_bytes();
        grouped.html_match = Some(html_equal);
        if !html_equal {
            if matches!(outcome.status, Status::MissingApproval) {
                outcome.status = Status::PixelsDiffer;
            }
            notes.push(format!(
                "html differs (render-level gate): {}",
                first_difference(&approved_html, artifacts.html.as_bytes())
            ));
        }

        // PNG pixel gate: decoded pixels, same threshold semantics as the
        // classic store. A corrupt approved PNG is an explicit error.
        let verdict = diff::compare_png(&approved_png, &artifacts.png)?;
        if !verdict.dims_equal {
            outcome.status = Status::DimensionMismatch;
            notes.push(format!(
                "png dimensions differ: approved {:?}, actual {:?}",
                verdict.expected_dims, verdict.actual_dims
            ));
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
                let path = sibling_diff(&self.diff_root, name);
                write_atomic(&path, &verdict.diff_png)?;
                outcome.diff_png = Some(path);
            }
        }

        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::Matched;
        }
        outcome.note = notes.join("; ");
        Ok(grouped)
    }

    /// Explicitly approve one scenario: the four actual artifacts replace
    /// the approved ones (atomic per file). Sidecars stay in the scratch
    /// area — the approved tree holds the four artifacts and nothing else.
    /// There is deliberately no environment-variable auto-accept.
    pub fn accept(&self, name: &str) -> Result<(), SnapshotError> {
        validate_name(name)?;
        let actual = artifact_paths(&self.actual_root, name);
        let approved = artifact_paths(&self.approved_root, name);
        for (src, dst, label) in [
            (&actual.ansi, &approved.ansi, ".ansi"),
            (&actual.txt, &approved.txt, ".txt"),
            (&actual.png, &approved.png, ".png"),
            (&actual.html, &approved.html, ".html"),
        ] {
            let bytes = std::fs::read(src).map_err(|e| {
                SnapshotError(format!(
                    "nothing to accept for `{name}` ({label} {}: {e})",
                    src.display()
                ))
            })?;
            write_atomic(dst, &bytes)?;
        }
        Ok(())
    }

    /// Accept every scenario with actual artifacts, recursively. Returns the
    /// accepted names (sorted).
    pub fn accept_all(&self) -> Result<Vec<String>, SnapshotError> {
        let names = self.actual_names()?;
        for name in &names {
            self.accept(name)?;
        }
        Ok(names)
    }

    /// Re-verify every actual scenario and rewrite the HTML report — the
    /// grouped form of [`crate::snapshot::Store::report`]. Unmatched gates
    /// do not error here: inspect [`StoreReport::failed`] and the outcomes.
    ///
    /// This constructs a fresh [`Renderer`] per call; bulk callers should
    /// build one and use [`Self::report_with`].
    pub fn report(
        &self,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        let mut renderer = Renderer::new(profile, faces)?;
        self.report_with(&mut renderer, pixel_threshold, title)
    }

    /// [`Self::report`] through a caller-owned [`Renderer`].
    pub fn report_with(
        &self,
        renderer: &mut Renderer,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        let names = self.actual_names()?;
        let mut entries = Vec::new();
        let mut outcomes = Vec::new();
        for name in names {
            let frame_path = artifact_paths(&self.actual_root, &name).frame_json;
            let text = std::fs::read_to_string(&frame_path).map_err(|e| {
                SnapshotError(format!("cannot read actual frame for `{name}`: {e}"))
            })?;
            let frame = Frame::from_json(&text)?;
            let grouped = self.check_with(renderer, &name, &frame, pixel_threshold)?;
            entries.push(report_entry(&grouped.outcome, renderer.profile())?);
            outcomes.push(grouped.outcome);
        }
        let path = write_report_at(&self.report_path(), title, &entries)?;
        Ok(StoreReport { path, outcomes })
    }
}

/// Diff PNG path of one scenario under the diff root (`<name>.png`).
fn sibling_diff(diff_root: &Path, name: &str) -> PathBuf {
    diff_root.join(format!("{name}.png"))
}

/// Recursively collect scenario names below `root`: every `*.ansi` file,
/// relative to `root`, suffix stripped, segments re-joined with `/`.
fn list_names(root: &Path) -> Result<Vec<String>, SnapshotError> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    walk_names(root, root, &mut out)?;
    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_names(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), SnapshotError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
        let path = entry.path();
        if path.is_dir() {
            walk_names(root, &path, out)?;
            continue;
        }
        let Some(file) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(_) = file.strip_suffix(".ansi") else {
            continue;
        };
        let rel = path.strip_prefix(root).map_err(|e| {
            SnapshotError(format!(
                "cannot relativize {} below {}: {e}",
                path.display(),
                root.display()
            ))
        })?;
        let mut segments: Vec<&str> = rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        if let Some(last) = segments.last_mut() {
            *last = last.strip_suffix(".ansi").unwrap_or(last);
        }
        let name = segments.join("/");
        validate_name(&name).map_err(SnapshotError::from)?;
        out.push(name);
    }
    Ok(())
}
