//! `tuisnap`: snapshot any TUI to ANSI/TXT/SVG/HTML/PNG/JSON.
//!
//! ```text
//! # one-shot black-box capture (no tmux):
//! tuisnap run --cols 120 --rows 40 --send enter --wait-for "Ready" \
//!   --format txt --format png --format svg --out shots/home -- -- ./my-tui --flag
//!
//! # offline re-render of a saved stream:
//! tuisnap render --input shots/home.ansi --cols 120 --rows 40 \
//!   --format png --format html --out shots/home
//!
//! # digest + baseline gate for big refactorings:
//! tuisnap digest --input shots/home.ansi --name home --baseline baselines/tui.txt
//! BLESS=1 tuisnap digest --input shots/home.ansi --name home --baseline baselines/tui.txt
//! ```

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "tuisnap",
    version,
    about = "TUI snapshots: PTY capture + ANSI render + digest baselines"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Spawn a command in a real PTY, drive it, save snapshot artifacts.
    Run {
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 40)]
        rows: u16,
        /// Input steps, repeatable: text via `type:<..>`, keys
        /// (enter|escape|tab|up|down|left|right|space|ctrl-x|text:..),
        /// `sleep:<ms>`, `wait:<needle>`.
        #[arg(long = "send")]
        sends: Vec<String>,
        /// Wait for this text before snapshotting (in addition to --send waits).
        #[arg(long)]
        wait_for: Option<String>,
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// Artifact formats, repeatable (cellshot-style).
        #[arg(long = "format", default_values_t = vec!["txt".to_string(), "ansi".to_string(), "png".to_string()])]
        formats: Vec<String>,
        /// Output prefix: `--out shots/home` + `--format png` = `shots/home.png`.
        #[arg(long, default_value = "shot")]
        out: String,
        /// Command after `--`.
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Re-render a saved `.ansi` stream to any formats (no process launch).
    Render {
        #[arg(long)]
        input: String,
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 40)]
        rows: u16,
        #[arg(long = "format", default_values_t = vec!["png".to_string(), "svg".to_string(), "html".to_string()])]
        formats: Vec<String>,
        #[arg(long, default_value = "shot")]
        out: String,
    },
    /// Digest a snapshot and compare/bless a baseline line.
    Digest {
        #[arg(long)]
        input: String,
        #[arg(long, default_value = "screen")]
        name: String,
        #[arg(long)]
        baseline: String,
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 40)]
        rows: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            cols,
            rows,
            sends,
            wait_for,
            timeout_ms,
            formats,
            out,
            argv,
        } => {
            let argv: Vec<String> = argv.into_iter().skip_while(|a| a == "--").collect();
            anyhow::ensure!(!argv.is_empty(), "pass the command after `--`");
            let opts = tuisnap::pty::PtyOptions {
                cols,
                rows,
                timeout: Duration::from_millis(timeout_ms),
                settle: Duration::from_millis(300),
                ..Default::default()
            };
            let mut steps = sends;
            if let Some(w) = wait_for {
                steps.push(format!("wait:{w}"));
            }
            let frame = tuisnap::pty::run_once(&argv, &opts, &steps)?;
            for f in &formats {
                let p = tuisnap::render::write_format(&frame, f, &out)?;
                eprintln!("wrote {p}");
            }
            println!("{}", frame.text());
        }
        Cmd::Render {
            input,
            cols,
            rows,
            formats,
            out,
        } => {
            let text = std::fs::read_to_string(&input).with_context(|| format!("read {input}"))?;
            let frame = tuisnap::ansi::parse_ansi(&text, cols, rows);
            for f in &formats {
                let p = tuisnap::render::write_format(&frame, f, &out)?;
                eprintln!("wrote {p}");
            }
        }
        Cmd::Digest {
            input,
            name,
            baseline,
            cols,
            rows,
        } => {
            let raw = std::fs::read(&input).with_context(|| format!("read {input}"))?;
            let frame = if input.ends_with(".ansi") {
                let text = String::from_utf8_lossy(&raw).into_owned();
                tuisnap::ansi::parse_ansi(&text, cols, rows)
            } else if input.ends_with(".json") {
                serde_json::from_slice::<tuisnap::Frame>(&raw)?
            } else {
                // plain .txt: single-style frame
                let text = String::from_utf8_lossy(&raw).into_owned();
                let mut f = tuisnap::Frame::blank(cols, rows);
                for (y, line) in text.split('\n').take(rows as usize).enumerate() {
                    for (x, ch) in line.chars().take(cols as usize).enumerate() {
                        f.set(
                            x as u16,
                            y as u16,
                            tuisnap::Cell {
                                symbol: ch.to_string(),
                                ..Default::default()
                            },
                        );
                    }
                }
                f
            };
            let base = tuisnap::Baseline::new(&baseline);
            let hex = base.assert_frame(&name, &frame)?;
            println!("{name} {} {} {hex}", frame.cols, frame.rows);
        }
    }
    Ok(())
}
