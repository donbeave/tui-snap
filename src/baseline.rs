//! Digest baselines with atomic `BLESS=1` regeneration.
//!
//! Borrowed from tcc `digest.rs` (merge-under-lock, atomic rename, sorted
//! output) and `insta`/`expect-test` (`BLESS`/`UPDATE_*` env to accept).
//! Simplified to single-process atomicity: write tmp + rename.

use crate::digest::{digest_frame, parse_baseline, scene_key};
use crate::frame::Frame;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub const HEADER: &str =
    "# tuisnap baseline: name cols rows hash — regenerate with BLESS=1, review like source\n";

#[derive(Debug, Clone)]
pub struct Baseline {
    pub path: String,
}

impl Baseline {
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
        }
    }

    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .map(|t| parse_baseline(&t))
            .unwrap_or_default()
    }

    fn bless_enabled() -> bool {
        let on = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        if std::env::var_os("BLESS").is_some() {
            return on("BLESS");
        }
        on("UPDATE_SNAPSHOT") || on("UPDATE_BASELINE")
    }

    /// Compare or bless. Returns the digest hex either way.
    pub fn assert_frame(&self, name: &str, frame: &Frame) -> Result<String> {
        let key = scene_key(name, frame);
        let hex = format!("{:016x}", digest_frame(frame));
        if Self::bless_enabled() {
            let mut entries = self.load();
            // merge with current disk content (concurrent-bless safe pattern)
            entries.insert(key, hex.clone());
            let mut text = HEADER.to_string();
            for (k, v) in &entries {
                text.push_str(&format!("{k} {v}\n"));
            }
            if let Some(dir) = std::path::Path::new(&self.path).parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            let tmp = format!("{}.tmp.{}", self.path, std::process::id());
            std::fs::write(&tmp, &text)
                .with_context(|| format!("write baseline tmp {}", self.path))?;
            std::fs::rename(&tmp, &self.path)
                .with_context(|| format!("publish baseline {}", self.path))?;
            return Ok(hex);
        }
        let entries = self.load();
        match entries.get(&key) {
            None => anyhow::bail!(
                "no baseline for `{key}` in {}; run with BLESS=1 to record it\n{}",
                self.path,
                frame.text()
            ),
            Some(expected) if *expected == hex => Ok(hex),
            Some(expected) => anyhow::bail!(
                "snapshot `{key}` changed: baseline {expected}, got {hex}; review the diff, then BLESS=1\n{}",
                frame.text()
            ),
        }
    }
}
