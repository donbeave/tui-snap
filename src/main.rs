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
    },
    /// Re-verify every actual frame in the store and rewrite the report.
    Report {
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value = "tuisnap visual report")]
        title: String,
        #[arg(long, default_value_t = 1.0)]
        pixel_threshold: f64,
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

fn load_font_bytes(path: Option<&PathBuf>) -> Result<(Vec<u8>, tuisnap::Profile)> {
    let profile = tuisnap::Profile::default_profile();
    match path {
        None => Ok((tuisnap::VENDORED_FONT.to_vec(), profile)),
        Some(p) => {
            let bytes = std::fs::read(p).with_context(|| format!("read font {}", p.display()))?;
            let profile = profile.with_font_file(format!("{}", p.display()), &bytes);
            Ok((bytes, profile))
        }
    }
}

fn render_formats(
    frame: &tuisnap::Frame,
    profile: &tuisnap::Profile,
    font_bytes: &[u8],
    formats: &[String],
    out: &str,
) -> Result<()> {
    if formats.is_empty() {
        anyhow::bail!("no --format given");
    }
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
                // Standalone view: authoritative PNG embedded + frame JSON.
                let png = tuisnap::render::render_png(frame, profile, font_bytes)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);
                let html = format!(
                    "<!doctype html><html><head><meta charset=\"utf-8\"><title>tuisnap</title></head><body style=\"background:#141414\"><img src=\"data:image/png;base64,{b64}\" alt=\"frame\"><script type=\"application/json\">{}</script></body></html>",
                    tuisnap::snapshot::json_for_script(&frame.to_json())
                );
                std::fs::write(&path, html)?;
            }
            "png" => {
                let png = tuisnap::render::render_png(frame, profile, font_bytes)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                std::fs::write(&path, png)?;
            }
            _ => anyhow::bail!("unknown format: {f} (txt|ansi|json|svg|html|png)"),
        }
        eprintln!("wrote {path}");
    }
    Ok(())
}

fn report_entry(
    store: &tuisnap::snapshot::Store,
    outcome: &tuisnap::snapshot::CompareOutcome,
    profile: &tuisnap::Profile,
) -> Result<tuisnap::snapshot::ReportEntry> {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;
    let actual_png = std::fs::read(&outcome.actual_png)?;
    let actual_compact = std::fs::read_to_string(&outcome.actual_frame)?;
    let actual_frame =
        tuisnap::Frame::from_json(&actual_compact).map_err(|e| anyhow::anyhow!("{e}"))?;
    let actual_json = actual_frame.to_json_pretty();
    let expected_png_b64 = outcome
        .expected_png
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .map(|b| b64.encode(&b));
    let diff_png_b64 = outcome
        .diff_png
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .map(|b| b64.encode(&b));
    let expected_frame_json = std::fs::read_to_string(&outcome.expected_frame).ok();
    let _ = store;
    Ok(tuisnap::snapshot::ReportEntry {
        outcome: outcome.clone(),
        expected_png_b64,
        actual_png_b64: b64.encode(&actual_png),
        diff_png_b64,
        expected_frame_json,
        actual_frame_json: actual_json,
        actual_frame_compact: actual_compact,
        profile_desc: profile.name.clone(),
        font_sha256: profile.font_sha256.clone(),
    })
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
            let (font_bytes, profile) = load_font_bytes(font_file.as_ref())?;
            let text = std::fs::read_to_string(&input)
                .with_context(|| format!("read {}", input.display()))?;
            let frame = tuisnap::Frame::from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
            render_formats(&frame, &profile, &font_bytes, &formats, &out)?;
            println!("{}", frame.text());
        }
        Cmd::Check {
            store,
            name,
            input,
            pixel_threshold,
            font_file,
        } => {
            let (font_bytes, profile) = load_font_bytes(font_file.as_ref())?;
            let text = std::fs::read_to_string(&input)
                .with_context(|| format!("read {}", input.display()))?;
            let frame = tuisnap::Frame::from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
            let st = tuisnap::snapshot::Store::new(&store);
            let outcome = st.check(&name, &frame, &profile, &font_bytes, pixel_threshold)?;
            let entry = report_entry(&st, &outcome, &profile)?;
            let report = tuisnap::snapshot::write_report(&st, "tuisnap visual report", &[entry])?;
            eprintln!("report: {}", report.display());
            outcome
                .ensure_matched()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("matched: {name}");
        }
        Cmd::Accept { store, name, all } => {
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
        Cmd::Report {
            store,
            title,
            pixel_threshold,
            font_file,
        } => {
            let (font_bytes, profile) = load_font_bytes(font_file.as_ref())?;
            let st = tuisnap::snapshot::Store::new(&store);
            let mut entries = Vec::new();
            let mut failed = 0u32;
            for path in glob_actual(&store)? {
                let text = std::fs::read_to_string(&path)?;
                let frame = tuisnap::Frame::from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_suffix(".frame"))
                    .unwrap_or("?")
                    .to_string();
                let outcome = st.check(&name, &frame, &profile, &font_bytes, pixel_threshold)?;
                if !outcome.status.matched() {
                    failed += 1;
                }
                entries.push(report_entry(&st, &outcome, &profile)?);
            }
            let report = tuisnap::snapshot::write_report(&st, &title, &entries)?;
            println!("report: {} ({} failed)", report.display(), failed);
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
                    let (font_bytes, profile) = load_font_bytes(font_file.as_ref())?;
                    let st = tuisnap::snapshot::Store::new(&root);
                    let outcome = st.check(&n, &frame, &profile, &font_bytes, pixel_threshold)?;
                    let entry = report_entry(&st, &outcome, &profile)?;
                    let report =
                        tuisnap::snapshot::write_report(&st, "tuisnap visual report", &[entry])?;
                    eprintln!("report: {}", report.display());
                    outcome
                        .ensure_matched()
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("matched: {n}");
                }
                _ => {
                    let (font_bytes, profile) = load_font_bytes(None)?;
                    render_formats(&frame, &profile, &font_bytes, &formats, &out)?;
                    println!("{}", frame.text());
                }
            }
        }
    }
    Ok(())
}

fn glob_actual(store: &std::path::Path) -> Result<Vec<PathBuf>> {
    let dir = store.join("actual");
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    // Same contract as `Store::actual_names`: canonical `*.frame.json` only.
    for entry in std::fs::read_dir(&dir)? {
        let p = entry?.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json")
            && p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.ends_with(".frame"))
        {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}
