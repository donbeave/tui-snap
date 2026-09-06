//! FNV-1a digest over (symbol, fg, bg, attrs) per cell.
//!
//! Same idea as tcc `crates/tui-testing/src/digest.rs`: one
//! `name cols rows hash` line per scene, hash reviewed like source.

use crate::frame::Frame;
use std::collections::BTreeMap;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Stable digest of a frame.
#[must_use]
pub fn digest_frame(frame: &Frame) -> u64 {
    let mut h = FNV_OFFSET;
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            let c = frame.get(x, y).cloned().unwrap_or_default();
            h = fnv(h, c.symbol.as_bytes());
            h = fnv(
                h,
                format!(
                    "{:?}|{:?}|{}{}{}{}{}",
                    c.fg, c.bg, c.bold, c.dim, c.italic, c.underline, c.reverse
                )
                .as_bytes(),
            );
        }
    }
    h
}

/// Baseline key for a named scene.
#[must_use]
pub fn scene_key(name: &str, frame: &Frame) -> String {
    format!("{} {} {}", name, frame.cols, frame.rows)
}

/// Parse `name cols rows hash` baseline text.
#[must_use]
pub fn parse_baseline(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.rsplit_once(' ') {
            out.insert(k.to_owned(), v.to_owned());
        }
    }
    out
}
