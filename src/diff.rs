//! Decoded-pixel comparison: approved PNG vs actual PNG.
//!
//! Kept separate from [`Frame::diff_cells`] on purpose: a renderer upgrade
//! can change pixels without changing application cells, and that must read
//! as a renderer event — not an app regression. Gates compare **decoded**
//! pixels, never compressed PNG bytes (re-encoding the same image must not
//! fail a gate).
//!
//! Engine: `image-compare` hybrid metric (structural + chroma). A strict gate
//! uses `score >= 1.0` with equal dimensions; looser review passes a lower
//! threshold explicitly.

use image::RgbImage;

/// Pixel-gate failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffError(pub String);

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pixel diff error: {}", self.0)
    }
}

impl std::error::Error for DiffError {}

/// Outcome of one PNG-vs-PNG comparison.
#[derive(Debug)]
pub struct PixelVerdict {
    pub dims_equal: bool,
    pub expected_dims: (u32, u32),
    pub actual_dims: (u32, u32),
    /// 1.0 = identical. Meaningful only when `dims_equal`.
    pub score: f64,
    /// Red-overlay diff image (empty when dimensions differ).
    pub diff_png: Vec<u8>,
}

fn decode_png(bytes: &[u8], label: &str) -> Result<RgbImage, DiffError> {
    image::load_from_memory(bytes)
        .map_err(|e| DiffError(format!("cannot decode {label} PNG: {e}")))
        .map(|d| d.to_rgb8())
}

/// Compare decoded pixels. Never compares compressed bytes.
pub fn compare_png(expected_png: &[u8], actual_png: &[u8]) -> Result<PixelVerdict, DiffError> {
    let expected = decode_png(expected_png, "expected")?;
    let actual = decode_png(actual_png, "actual")?;
    let expected_dims = (expected.width(), expected.height());
    let actual_dims = (actual.width(), actual.height());
    if expected_dims != actual_dims {
        return Ok(PixelVerdict {
            dims_equal: false,
            expected_dims,
            actual_dims,
            score: 0.0,
            diff_png: Vec::new(),
        });
    }
    let result = image_compare::rgb_hybrid_compare(&expected, &actual)
        .map_err(|e| DiffError(format!("comparison failed: {e}")))?;
    let mut diff_png = Vec::new();
    image::DynamicImage::ImageRgb8(result.image.to_color_map().to_rgb8())
        .write_to(
            &mut std::io::Cursor::new(&mut diff_png),
            image::ImageFormat::Png,
        )
        .map_err(|e| DiffError(format!("diff PNG encode: {e}")))?;
    Ok(PixelVerdict {
        dims_equal: true,
        expected_dims,
        actual_dims,
        score: result.score,
        diff_png,
    })
}
