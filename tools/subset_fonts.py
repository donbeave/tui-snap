#!/usr/bin/env python3
"""Rebuild the vendored fallback font subsets under assets/fonts/.

The PNG renderer's fallback chain (see src/profile.rs `VENDORED_FALLBACK_FACES`
and assets/fonts/FONTS.md) uses three Noto subsets so glyphs the primary
JetBrainsMono family lacks still rasterize as real glyphs, with zero system
font dependence (determinism across machines). This script regenerates those
subsets from pinned upstream bytes:

  1. downloads the three upstream fonts from commit-pinned URLs and verifies
     their SHA-256 (fail-closed: a changed upstream aborts the build);
  2. instantiates the Noto Sans Symbols variable font at wght=400 (static);
  3. subsets all three with pyftsubset (exact commands printed as they run);
  4. asserts every required codepoint survived subsetting;
  5. writes assets/fonts/ and prints the SHA-256 of each output (the values
     pinned in src/profile.rs and tests/render.rs).

Requirements: Python 3.10+ with fontTools (`pip install fonttools` or
`pip install --user fonttools`; pyftsubset on PATH is not required — the
subset step runs `python3 -m fontTools.subset`).

Usage:
  python3 tools/subset_fonts.py            # download, rebuild, overwrite assets
  python3 tools/subset_fonts.py --check    # verify committed assets only (offline)

Extending the subset (e.g. JIS X 0208 level-2 kanji, Hangul, more symbol
blocks): widen the ranges in `cjk_unicodes()` / `SYMBOLS2_RANGES` /
`SYMBOLS1_RANGES` below, rerun, then update the SHA-256 pins in
src/profile.rs (`VENDORED_*_FONT_SHA256`) and tests/render.rs, and the
coverage table in assets/fonts/FONTS.md. Mind repo size: a full Noto Sans
CJK face is ~16 MB — keep the subset to a few MB max.

The subset outputs are committed to the repo; this script is run manually
when the subset changes, never in CI. The pipeline is deterministic for
pinned inputs (the instancer's wall-clock `head.modified` stamp is
normalized back to the source VF's value; pyftsubset then preserves it), so
re-running reproduces the committed bytes exactly. The committed bytes plus
the pinned output hashes in tests/render.rs are the authority.
"""

import argparse
import hashlib
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets" / "fonts"

# Commit-pinned upstream sources (SHA-256 verified after download).
UPSTREAMS = {
    "symbols_vf": {
        "url": "https://github.com/google/fonts/raw/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/"
               "ofl/notosanssymbols/NotoSansSymbols%5Bwght%5D.ttf",
        "sha256": "f7e7e04b4a24b6c78893d50cbfd2b2f6cae49617ab047bfef668d252adb128f7",
        "file": "NotoSansSymbols[wght].ttf",
    },
    "symbols2": {
        "url": "https://github.com/google/fonts/raw/7b6724ac7ececc713e9ba93af309f7520c9a80a3/"
               "ofl/notosanssymbols2/NotoSansSymbols2-Regular.ttf",
        "sha256": "7d5fb73b7ca67a6798101741f5d280a3d016a56a197afcd4199dbb57b4b82a21",
        "file": "NotoSansSymbols2-Regular.ttf",
    },
    "cjk": {
        "url": "https://github.com/notofonts/noto-cjk/raw/165c01b46ea533872e002e0785ff17e44f6d97d8/"
               "Sans/OTF/Japanese/NotoSansCJKjp-Regular.otf",
        "sha256": "68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5",
        "file": "NotoSansCJKjp-Regular.otf",
    },
}

# Subset ranges. Symbols v1/v2 have DISJOINT coverage in U+2600-26FF (probed):
# v1 holds the astrological/misc block incl. U+26B7 (⚷), v2 holds ★☕ and the
# shape blocks. Both are needed.
SYMBOLS1_RANGES = [(0x2600, 0x26FF)]   # ⚷ ⚙ ♻ ☰-class misc symbols unique to v1
SYMBOLS2_RANGES = [
    (0x25A0, 0x25FF),                  # Geometric Shapes: ◐ ● ◆ ■
    (0x2600, 0x26FF),                  # Miscellaneous Symbols: ★ ☕ ⚠ ♠
    (0x2700, 0x27BF),                  # Dingbats: ❤ ✔ ✈
    (0x2B00, 0x2BFF),                  # Misc Symbols and Arrows: ⬤ ⭐ ⏹
]
CJK_STATIC_RANGES = [
    (0x3000, 0x30FF),                  # CJK punctuation, hiragana, katakana
    (0xFF01, 0xFF65),                  # fullwidth forms (Ａ１！…)
]

# Glyphs that MUST survive subsetting (asserted before writing assets).
REQUIRED = {
    "symbols": [0x26B7, 0x2699, 0x267B],
    "symbols2": [0x25D0, 0x2605, 0x2615, 0x2764, 0x2714, 0x25CF, 0x2B24, 0x2B50],
    "cjk": [0x6771, 0x4EAC, 0x65E5, 0x672C, 0x8A9E, 0x3042, 0x30A2, 0x3005, 0xFF21, 0x3000],
}

# pyftsubset flags shared by all three subsets: no hinting (fontdue rasterizes
# raw outlines), minimal name table keeping the OFL-required license entries,
# no GSUB/GPOS (the renderer does its own per-cell glyph placement).
SUBSET_FLAGS = [
    "--no-hinting",
    "--name-IDs=0,1,2,4,6,13,14",
    "--name-legacy",
    "--name-languages=*",
    "--no-layout-closure",
    "--drop-tables+=GSUB,GPOS",
]

OUTPUTS = {
    "symbols": "NotoSansSymbols-subset.ttf",
    "symbols2": "NotoSansSymbols2-subset.ttf",
    "cjk": "NotoSansCJKjp-subset.otf",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def jis_x_0208_level1_kanji() -> list[int]:
    """JIS X 0208 level-1 kanji (rows 16-47): the 2965 most common kanji.

    Derived via the euc_jp codec: a level-1 kanji encodes as two bytes with
    lead byte 0xB0-0xCF (row = lead - 0xA0). No data file needed.
    """
    out = []
    for cp in range(0x4E00, 0xA000):
        try:
            b = chr(cp).encode("euc_jp")
        except UnicodeEncodeError:
            continue
        if len(b) == 2 and 0xB0 <= b[0] <= 0xCF:
            out.append(cp)
    return out


def cjk_unicodes() -> list[int]:
    cps = set(jis_x_0208_level1_kanji())
    for lo, hi in CJK_STATIC_RANGES:
        cps.update(range(lo, hi + 1))
    return sorted(cps)


def ranged_unicodes(ranges) -> list[int]:
    cps = set()
    for lo, hi in ranges:
        cps.update(range(lo, hi + 1))
    return sorted(cps)


def download(work: Path) -> dict[str, Path]:
    import urllib.request

    out = {}
    for key, spec in UPSTREAMS.items():
        dest = work / f"{key}.upstream"
        print(f"download {spec['url']}")
        with urllib.request.urlopen(spec["url"], timeout=120) as r:
            dest.write_bytes(r.read())
        digest = sha256(dest)
        if digest != spec["sha256"]:
            raise SystemExit(
                f"FAIL: {spec['file']} sha256 {digest} != pinned {spec['sha256']} — "
                "upstream bytes changed; re-review before updating the pin"
            )
        out[key] = dest
    return out


def run(cmd: list[str]) -> None:
    print("+", " ".join(cmd))
    subprocess.run(cmd, check=True)


def subset(src: Path, dest: Path, unicodes: list[int]) -> None:
    ufile = dest.with_suffix(".unicodes")
    ufile.write_text(",".join(f"U+{c:04X}" for c in unicodes))
    run([sys.executable, "-m", "fontTools.subset", str(src),
         f"--output-file={dest}", f"--unicodes-file={ufile}", *SUBSET_FLAGS])
    ufile.unlink()


def assert_coverage(path: Path, required: list[int]) -> None:
    from fontTools.ttLib import TTFont

    with TTFont(path, lazy=True) as f:
        cmap = f.getBestCmap()
        missing = [f"U+{cp:04X}" for cp in required if cp not in cmap]
    if missing:
        raise SystemExit(f"FAIL: {path.name} lost required codepoints: {missing}")


def instancer_static_regular(src_vf: Path, dest: Path) -> None:
    """Static wght=400 instance of the Symbols variable font.

    The instancer stamps `head.modified` with the wall clock; copy the source
    VF's value back so the pipeline is deterministic for pinned inputs.
    """
    run([sys.executable, "-m", "fontTools.varLib.instancer",
         "-o", str(dest), str(src_vf), "wght=400"])
    from fontTools.ttLib import TTFont

    with TTFont(src_vf) as vf:
        modified = vf["head"].modified
    # recalcTimestamp=False: TTFont.save() would otherwise re-stamp
    # head.modified with the wall clock.
    with TTFont(dest, recalcTimestamp=False) as inst:
        inst["head"].modified = modified
        inst.save(dest)


def build() -> None:
    with tempfile.TemporaryDirectory() as td:
        work = Path(td)
        src = download(work)
        # Static Regular instance of the Symbols variable font.
        symbols_regular = work / "NotoSansSymbols-Regular.ttf"
        instancer_static_regular(src["symbols_vf"], symbols_regular)
        outputs = {
            "symbols": (symbols_regular, ranged_unicodes(SYMBOLS1_RANGES)),
            "symbols2": (src["symbols2"], ranged_unicodes(SYMBOLS2_RANGES)),
            "cjk": (src["cjk"], cjk_unicodes()),
        }
        for key, (font, unicodes) in outputs.items():
            dest = ASSETS / OUTPUTS[key]
            subset(font, dest, unicodes)
            assert_coverage(dest, REQUIRED[key])
            print(f"wrote {dest} ({dest.stat().st_size} bytes) sha256 {sha256(dest)}")
    print("done. Update VENDORED_*_FONT_SHA256 in src/profile.rs and the pins in "
          "tests/render.rs if any hash changed, and assets/fonts/FONTS.md coverage.")


def check() -> None:
    ok = True
    for key, name in OUTPUTS.items():
        path = ASSETS / name
        if not path.exists():
            print(f"MISSING {path}")
            ok = False
            continue
        assert_coverage(path, REQUIRED[key])
        print(f"{name}: {path.stat().st_size} bytes sha256 {sha256(path)} coverage OK")
    if not ok:
        raise SystemExit(1)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify committed assets only (offline)")
    args = ap.parse_args()
    check() if args.check else build()
