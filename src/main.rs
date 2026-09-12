//! `tuisnap`: visual-regression toolkit for Ratatui TUIs.
//!
//! ```text
//! # offline: canonical JSON -> artifacts
//! tuisnap render --input shot.frame.json --format png --format svg --out shot
//! # gate one frame against the approved store
//! tuisnap check --store tests/visual --name home --input actual.frame.json
//! # explicit local acceptance (never automatic, never in CI)
//! tuisnap accept --store tests/visual --name home
//! # black-box capture of the real binary (feature `pty`)
//! tuisnap run --cols 120 --rows 40 --send enter --wait-for Ready \
//!   --format png --out shots/home -- ./my-tui
//! ```

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "tuisnap",
    version,
    about = "TUI visual regression: frames, PNGs, HTML reports"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Render a canonical frame.json to offline artifacts.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "format", default_values_t = Vec::<String>::new())]
        formats: Vec<String>,
        #[arg(long, default_value = "shot")]
        out: String,
        #[arg(long)]
        font_file: Option<PathBuf>,
    },
    /// Check one frame against the approved store (writes actuals first).
    Check {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        pixel_threshold: f64,
        /// Grouped multi-artifact store (see docs/USAGE.md): --store is the
        /// approved root holding <name>.{ansi,txt,png,html} only.
        #[arg(long, default_value_t = false)]
        grouped: bool,
        /// Override the pinned profile font (hash recorded in the report).
        #[arg(long)]
        font_file: Option<PathBuf>,
    },
    /// Explicitly approve actual snapshots (local review only).
    Accept {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Grouped multi-artifact store: --all walks nested names
        /// recursively (e.g. showcase/pages/overview_120x40_truecolor).
        #[arg(long, default_value_t = false)]
        grouped: bool,
    },
    /// Re-verify every actual frame in the store and rewrite the report.
    Report {
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value = "tuisnap visual report")]
        title: String,
        #[arg(long, default_value_t = 1.0)]
        pixel_threshold: f64,
        /// Grouped multi-artifact store: re-verifies nested actuals and
        /// writes the report under the store's scratch area (not approved/).
        #[arg(long, default_value_t = false)]
        grouped: bool,
        /// Grouped stores only: explicit report output path.
        #[arg(long)]
        report_path: Option<PathBuf>,
        /// Override the pinned profile font (hash recorded in the report).
        #[arg(long)]
        font_file: Option<PathBuf>,
    },
    /// Capture the real binary in a PTY, optionally gating into a store.
    Run {
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 40)]
        rows: u16,
        #[arg(long = "send")]
        sends: Vec<String>,
        #[arg(long)]
        wait_for: Option<String>,
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 300)]
        settle_ms: u64,
        #[arg(long = "format", default_values_t = vec!["txt".to_string(), "png".to_string()])]
        formats: Vec<String>,
        #[arg(long, default_value = "shot")]
        out: String,
        #[arg(long)]
        store: Option<PathBuf>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        pixel_threshold: f64,
        /// Override the pinned profile font (hash recorded in the report).
        #[arg(long)]
        font_file: Option<PathBuf>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
}

fn load_font_bytes(path: Option<&PathBuf>) -> Result<(OwnedFaces, tuisnap::Profile)> {
    let profile = tuisnap::Profile::default_profile();
    match path {
        None => Ok((
            OwnedFaces {
                regular: tuisnap::VENDORED_FONT.to_vec(),
                bold: tuisnap::VENDORED_FONT_BOLD.to_vec(),
                italic: tuisnap::VENDORED_FONT_ITALIC.to_vec(),
                bold_italic: tuisnap::VENDORED_FONT_BOLD_ITALIC.to_vec(),
            },
            profile,
        )),
        Some(p) => {
            let bytes = std::fs::read(p).with_context(|| format!("read font {}", p.display()))?;
            let profile = profile.with_font_file(format!("{}", p.display()), &bytes);
            // Single-face override: faux bold/italic, as documented.
            Ok((
                OwnedFaces {
                    regular: bytes.clone(),
                    bold: bytes.clone(),
                    italic: bytes.clone(),
                    bold_italic: bytes,
                },
                profile,
            ))
        }
    }
}

/// Owned face bytes so `--font-file` overrides can outlive their read.
struct OwnedFaces {
    regular: Vec<u8>,
    bold: Vec<u8>,
    italic: Vec<u8>,
    bold_italic: Vec<u8>,
}

impl OwnedFaces {
    fn faces(&self) -> tuisnap::FontFaces<'_> {
        tuisnap::FontFaces {
            regular: &self.regular,
            bold: &self.bold,
            italic: &self.italic,
            bold_italic: &self.bold_italic,
        }
    }
}

/// The cached [`tuisnap::render::Renderer`] shared across formats, so
/// `--format png --format html` parses the faces once.
fn renderer_cached<'a>(
    slot: &'a mut Option<tuisnap::Renderer>,
    profile: &tuisnap::Profile,
    faces: &tuisnap::FontFaces<'_>,
) -> Result<&'a mut tuisnap::Renderer> {
    if slot.is_none() {
        *slot = Some(tuisnap::Renderer::new(profile, faces).map_err(|e| anyhow::anyhow!("{e}"))?);
    }
    Ok(slot.as_mut().expect("constructed above"))
}

/// Render `frame` through [`renderer_cached`].
fn render_cached(
    slot: &mut Option<tuisnap::Renderer>,
    frame: &tuisnap::Frame,
    profile: &tuisnap::Profile,
    faces: &tuisnap::FontFaces<'_>,
) -> Result<tuisnap::render::Rendered> {
    renderer_cached(slot, profile, faces)?
        .render(frame)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn render_formats(
    frame: &tuisnap::Frame,
    profile: &tuisnap::Profile,
    faces: &tuisnap::FontFaces<'_>,
    formats: &[String],
    out: &str,
    name: &str,
) -> Result<()> {
    if formats.is_empty() {
        anyhow::bail!("no --format given");
    }
    let mut renderer: Option<tuisnap::Renderer> = None;
    for f in formats {
        let path = format!("{out}.{f}");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        match f.as_str() {
            "txt" => std::fs::write(&path, frame.text())?,
            "ansi" => std::fs::write(&path, tuisnap::render::ansi_dump(frame))?,
            "json" => std::fs::write(&path, frame.to_json())?,
            "svg" => std::fs::write(&path, tuisnap::render::render_svg(frame, profile))?,
            "html" => {
                // Standalone colored render, built by the library
                // (`Renderer::render_html`): selectable SVG primary visual,
                // authoritative PNG under <details>, frame JSON embedded.
                let html = renderer_cached(&mut renderer, profile, faces)?
                    .render_html(frame, name)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                std::fs::write(&path, html)?;
            }
            "png" => {
                let rendered = render_cached(&mut renderer, frame, profile, faces)?;
                std::fs::write(&path, &rendered.png)?;
                let sidecar = format!("{path}.fidelity.json");
                std::fs::write(&sidecar, rendered.fidelity.to_json())?;
                if rendered.fidelity.approximate {
                    eprintln!(
                        "note: {} uncovered glyph(s) (see {sidecar})",
                        rendered.fidelity.missing.len()
                    );
                }
            }
            _ => anyhow::bail!("unknown format: {f} (txt|ansi|json|svg|html|png)"),
        }
        eprintln!("wrote {path}");
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Render {
            input,
            formats,
            out,
            font_file,
        } => {
            let (owned, profile) = load_font_bytes(font_file.as_ref())?;
            let text = std::fs::read_to_string(&input)
                .with_context(|| format!("read {}", input.display()))?;
            let frame = tuisnap::Frame::from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
            // Frame name for the HTML <title>: input stem minus `.frame`.
            let name = input
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.strip_suffix(".frame").unwrap_or(s))
                .unwrap_or("frame");
            render_formats(&frame, &profile, &owned.faces(), &formats, &out, name)?;
            println!("{}", frame.text());
        }
        Cmd::Check {
            store,
            name,
            input,
            pixel_threshold,
            grouped,
            font_file,
        } => {
            let (owned, profile) = load_font_bytes(font_file.as_ref())?;
            let text = std::fs::read_to_string(&input)
                .with_context(|| format!("read {}", input.display()))?;
            let frame = tuisnap::Frame::from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
            if grouped {
                let st = tuisnap::grouped::GroupedStore::new(&store);
                let outcome =
                    st.check(&name, &frame, &profile, &owned.faces(), pixel_threshold)?;
                let entry = tuisnap::snapshot::report_entry(&outcome.outcome, &profile)?;
                let report =
                    tuisnap::snapshot::write_report_at(&st.report_path(), "tuisnap visual report", &[entry])?;
                eprintln!("report: {}", report.display());
                outcome
                    .ensure_matched()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            } else {
                let st = tuisnap::snapshot::Store::new(&store);
                let outcome = st.check(&name, &frame, &profile, &owned.faces(), pixel_threshold)?;
                let entry = st.report_entry(&outcome, &profile)?;
                let report =
                    tuisnap::snapshot::write_report(&st, "tuisnap visual report", &[entry])?;
                eprintln!("report: {}", report.display());
                outcome
                    .ensure_matched()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            println!("matched: {name}");
        }
        Cmd::Accept {
            store,
            name,
            all,
            grouped,
        } => {
            if grouped {
                let st = tuisnap::grouped::GroupedStore::new(&store);
                if all {
                    for n in st.accept_all()? {
                        println!("accepted: {n}");
                    }
                } else if let Some(n) = name {
                    st.accept(&n)?;
                    println!("accepted: {n}");
                } else {
                    anyhow::bail!("pass --name or --all");
                }
            } else {
                let st = tuisnap::snapshot::Store::new(&store);
                if all {
                    for n in st.actual_names()? {
                        st.accept(&n)?;
                        println!("accepted: {n}");
                    }
                } else if let Some(n) = name {
                    st.accept(&n)?;
                    println!("accepted: {n}");
                } else {
                    anyhow::bail!("pass --name or --all");
                }
            }
        }
        Cmd::Report {
            store,
            title,
            pixel_threshold,
            grouped,
            report_path,
            font_file,
        } => {
            let (owned, profile) = load_font_bytes(font_file.as_ref())?;
            let (path, failed) = if grouped {
                let mut st = tuisnap::grouped::GroupedStore::new(&store);
                if let Some(p) = report_path {
                    st = st.with_report_path(&p);
                }
                let report = st.report(&profile, &owned.faces(), pixel_threshold, &title)?;
                let failed = report.failed() as u32;
                (report.path, failed)
            } else {
                anyhow::ensure!(
                    report_path.is_none(),
                    "--report-path only applies to --grouped stores"
                );
                let st = tuisnap::snapshot::Store::new(&store);
                let report = st.report(&profile, &owned.faces(), pixel_threshold, &title)?;
                let failed = report.failed() as u32;
                (report.path, failed)
            };
            println!("report: {} ({} failed)", path.display(), failed);
            if failed > 0 {
                anyhow::bail!("{failed} snapshot(s) require review");
            }
        }
        Cmd::Run {
            cols,
            rows,
            sends,
            wait_for,
            timeout_ms,
            settle_ms,
            formats,
            out,
            store,
            name,
            pixel_threshold,
            font_file,
            argv,
        } => {
            let argv: Vec<String> = argv.into_iter().skip_while(|a| a == "--").collect();
            anyhow::ensure!(!argv.is_empty(), "pass the command after `--`");
            let opts = tuisnap::pty::PtyOptions {
                cols,
                rows,
                timeout: Duration::from_millis(timeout_ms),
                ..Default::default()
            };
            let mut steps = sends;
            if let Some(w) = wait_for {
                steps.push(format!("wait:{w}"));
            }
            let frame =
                tuisnap::pty::run_once(&argv, &opts, &steps, Duration::from_millis(settle_ms))?;
            match (store, name) {
                (Some(root), Some(n)) => {
                    let (owned, profile) = load_font_bytes(font_file.as_ref())?;
                    let st = tuisnap::snapshot::Store::new(&root);
                    let outcome = st.check(&n, &frame, &profile, &owned.faces(), pixel_threshold)?;
                    let entry = st.report_entry(&outcome, &profile)?;
                    let report =
                        tuisnap::snapshot::write_report(&st, "tuisnap visual report", &[entry])?;
                    eprintln!("report: {}", report.display());
                    outcome
                        .ensure_matched()
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("matched: {n}");
                }
                _ => {
                    let (owned, profile) = load_font_bytes(None)?;
                    // Frame name for the HTML <title>: the --out basename.
                    let name = std::path::Path::new(&out)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("frame");
                    let name = name.to_string();
                    render_formats(&frame, &profile, &owned.faces(), &formats, &out, &name)?;
                    println!("{}", frame.text());
                }
            }
        }
    }
    Ok(())
}
