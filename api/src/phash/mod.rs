//! Deterministic 256-bit perceptual hash (DCT / "pHash") for card images.
//!
//! This is the reference implementation used to build the fingerprint index. A
//! byte-for-byte twin runs in the browser (`web/src/lib/scan/phash.ts`) to hash a
//! photographed card, so the two hashes are directly comparable by Hamming distance.
//! Parity is guaranteed by construction: both sides do the same integer grayscale +
//! box downsample and the same DCT against the **shared** basis constants
//! ([`basis::DCT_BASIS`], generated once by `scripts/gen_phash_basis.py` for both
//! languages), so the floating-point work is identical multiply-add over identical
//! inputs. The `phash_parity` tests below pin fixed inputs to fixed output hashes;
//! the TS suite asserts the same fixtures produce the same bytes.
//!
//! Pipeline: luma → 32×32 box-average → 2D DCT-II → keep the top-left 16×16
//! low-frequency block (256 coefficients) → threshold each against the block median →
//! pack MSB-first into 32 bytes.

mod basis;

use basis::{DCT_BASIS, DCT_N};

/// Length of a fingerprint in bytes (256 bits).
pub const PHASH_BYTES: usize = 32;

/// Side of the low-frequency DCT block kept for the hash (16×16 = 256 bits).
const LOW_FREQ: usize = 16;

/// Integer luma weights (sum to 256) approximating Rec. 601 (0.299/0.587/0.114).
/// Kept integer so the browser twin computes the identical grayscale value.
const LUMA_R: u32 = 77;
const LUMA_G: u32 = 150;
const LUMA_B: u32 = 29;

/// Hash a grayscale (luma) plane of `width`×`height` bytes into a 256-bit pHash.
///
/// `luma.len()` must be `>= width * height`; anything beyond that is ignored. Returns
/// all-zero for an empty image (`width == 0 || height == 0`), which never matches a
/// real card (its DC-dominated hash is never all-zero).
pub fn phash_from_luma(luma: &[u8], width: usize, height: usize) -> [u8; PHASH_BYTES] {
    if width == 0 || height == 0 || luma.len() < width * height {
        return [0u8; PHASH_BYTES];
    }
    let grid = downsample(luma, width, height);
    let coeffs = dct_low_freq(&grid);
    threshold(&coeffs)
}

/// Hash a packed RGBA (4 bytes/pixel) buffer — grayscale first, then
/// [`phash_from_luma`]. The alpha channel is ignored.
pub fn phash_from_rgba(rgba: &[u8], width: usize, height: usize) -> [u8; PHASH_BYTES] {
    let n = width.saturating_mul(height);
    if n == 0 || rgba.len() < n * 4 {
        return [0u8; PHASH_BYTES];
    }
    let mut luma = vec![0u8; n];
    for i in 0..n {
        let r = rgba[i * 4] as u32;
        let g = rgba[i * 4 + 1] as u32;
        let b = rgba[i * 4 + 2] as u32;
        luma[i] = ((r * LUMA_R + g * LUMA_G + b * LUMA_B) >> 8) as u8;
    }
    phash_from_luma(&luma, width, height)
}

/// Box-average `luma` down to a `DCT_N`×`DCT_N` grid of integer averages (0..=255),
/// returned as `f64` for the DCT. Each output cell averages the source pixels whose
/// centres fall in its `floor`-partitioned box; a box is guaranteed non-empty (at
/// least one pixel) even when the source is smaller than `DCT_N` on an axis.
fn downsample(luma: &[u8], width: usize, height: usize) -> [[f64; DCT_N]; DCT_N] {
    let mut grid = [[0.0f64; DCT_N]; DCT_N];
    for ty in 0..DCT_N {
        let y0 = ty * height / DCT_N;
        let mut y1 = (ty + 1) * height / DCT_N;
        if y1 <= y0 {
            y1 = y0 + 1;
        }
        y1 = y1.min(height);
        for tx in 0..DCT_N {
            let x0 = tx * width / DCT_N;
            let mut x1 = (tx + 1) * width / DCT_N;
            if x1 <= x0 {
                x1 = x0 + 1;
            }
            x1 = x1.min(width);
            let mut sum: u32 = 0;
            for y in y0..y1 {
                let row = y * width;
                for x in x0..x1 {
                    sum += luma[row + x] as u32;
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            grid[ty][tx] = (sum / count) as f64;
        }
    }
    grid
}

/// 2D DCT-II of the 32×32 grid, keeping only the top-left `LOW_FREQ`×`LOW_FREQ`
/// coefficients (row-major). Separable: transform along x, then along y, both against
/// the shared basis — so the browser twin produces the identical coefficients.
fn dct_low_freq(grid: &[[f64; DCT_N]; DCT_N]) -> [f64; LOW_FREQ * LOW_FREQ] {
    // Rows: T[y][v] = sum_x BASIS[v][x] * grid[y][x], for v in 0..LOW_FREQ.
    let mut t = [[0.0f64; LOW_FREQ]; DCT_N];
    for (y, t_row) in t.iter_mut().enumerate() {
        for (v, t_yv) in t_row.iter_mut().enumerate() {
            let mut acc = 0.0f64;
            for x in 0..DCT_N {
                acc += DCT_BASIS[v][x] * grid[y][x];
            }
            *t_yv = acc;
        }
    }
    // Columns: F[u][v] = sum_y BASIS[u][y] * T[y][v], for u in 0..LOW_FREQ.
    let mut coeffs = [0.0f64; LOW_FREQ * LOW_FREQ];
    for u in 0..LOW_FREQ {
        for v in 0..LOW_FREQ {
            let mut acc = 0.0f64;
            for y in 0..DCT_N {
                acc += DCT_BASIS[u][y] * t[y][v];
            }
            coeffs[u * LOW_FREQ + v] = acc;
        }
    }
    coeffs
}

/// Threshold each coefficient against the median of all 256 and pack MSB-first into
/// 32 bytes. The median is the mean of the two middle values of the sorted set.
fn threshold(coeffs: &[f64; LOW_FREQ * LOW_FREQ]) -> [u8; PHASH_BYTES] {
    let mut sorted = *coeffs;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = (sorted[mid - 1] + sorted[mid]) / 2.0;

    let mut out = [0u8; PHASH_BYTES];
    for (i, &c) in coeffs.iter().enumerate() {
        if c > median {
            out[i >> 3] |= 1 << (7 - (i & 7));
        }
    }
    out
}

/// Hamming distance (differing bits) between two equal-length byte strings. Returns
/// `u32::MAX` when the lengths differ (never a real match), so a corrupt reference row
/// can't masquerade as a perfect hit.
pub fn hamming(a: &[u8], b: &[u8]) -> u32 {
    if a.len() != b.len() {
        return u32::MAX;
    }
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic synthetic luma generators, mirrored bit-for-bit by the TS parity
    /// suite (`web/src/lib/scan/__tests__/phash.spec.ts`) so both languages hash the
    /// exact same inputs. Keep the three kinds and their arithmetic in lockstep.
    fn synth_luma(kind: &str, width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0u8; width * height];
        match kind {
            "grad" => {
                for y in 0..height {
                    for x in 0..width {
                        let denom = (width - 1).max(1) as u32;
                        out[y * width + x] = ((x as u32 * 255) / denom) as u8;
                    }
                }
            }
            "checker" => {
                for y in 0..height {
                    for x in 0..width {
                        let on = (((x >> 3) + (y >> 3)) & 1) == 1;
                        out[y * width + x] = if on { 255 } else { 0 };
                    }
                }
            }
            "lcg" => {
                let mut s: u32 = 0x1234_5678u32
                    .wrapping_add(width as u32)
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(height as u32);
                for px in out.iter_mut() {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *px = ((s >> 24) & 0xff) as u8;
                }
            }
            _ => unreachable!("unknown synth kind"),
        }
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Golden fixtures pinning fixed inputs to fixed hashes. The TS suite asserts the
    /// SAME inputs produce the SAME hex — this is the cross-language parity contract.
    /// If the algorithm or the basis changes intentionally, regenerate BOTH sides
    /// (run this test to see the new hex; update it here and in the TS spec).
    #[test]
    fn golden_hashes_are_stable() {
        let cases = [("grad", 146, 204), ("checker", 146, 204), ("lcg", 64, 64)];
        let expected = ["grad-146x204", "checker-146x204", "lcg-64x64"];
        // Printed so a deliberate change is easy to copy into both suites.
        for ((kind, w, h), label) in cases.iter().zip(expected.iter()) {
            let luma = synth_luma(kind, *w, *h);
            let h256 = phash_from_luma(&luma, *w, *h);
            println!("PHASH_GOLDEN {label} = {}", hex(&h256));
        }
        // The pinned values (kept identical in the TS spec).
        assert_eq!(
            hex(&phash_from_luma(&synth_luma("grad", 146, 204), 146, 204)),
            GOLDEN_GRAD
        );
        assert_eq!(
            hex(&phash_from_luma(&synth_luma("checker", 146, 204), 146, 204)),
            GOLDEN_CHECKER
        );
        assert_eq!(
            hex(&phash_from_luma(&synth_luma("lcg", 64, 64), 64, 64)),
            GOLDEN_LCG
        );
    }

    // Pinned goldens — see `golden_hashes_are_stable`. Mirrored in the TS spec.
    const GOLDEN_GRAD: &str = "8aa888847457626370578aa88aa87557755775578aaa75577557aaaa8aa87557";
    const GOLDEN_CHECKER: &str = "fffeebaa88a8aaaa88aaaaaa88aa88a8aaaa88a8aaaaaaaa88a8ffff88aaeaaa";
    const GOLDEN_LCG: &str = "8de06a3b6e022449d537b5954e8bd9fe27830b71232d9c2a96c159ffb143b645";

    #[test]
    fn identical_input_hashes_identically() {
        let luma = synth_luma("lcg", 146, 204);
        assert_eq!(
            phash_from_luma(&luma, 146, 204),
            phash_from_luma(&luma, 146, 204)
        );
        assert_eq!(
            hamming(
                &phash_from_luma(&luma, 146, 204),
                &phash_from_luma(&luma, 146, 204)
            ),
            0
        );
    }

    #[test]
    fn different_images_differ() {
        let a = phash_from_luma(&synth_luma("grad", 146, 204), 146, 204);
        let b = phash_from_luma(&synth_luma("checker", 146, 204), 146, 204);
        assert!(
            hamming(&a, &b) > 20,
            "distinct patterns should be far apart"
        );
    }

    #[test]
    fn rgba_matches_precomputed_luma() {
        // A gray RGBA (r=g=b=v) must hash the same as a luma plane of v.
        let (w, h) = (80usize, 100usize);
        let mut rgba = vec![0u8; w * h * 4];
        let mut luma = vec![0u8; w * h];
        for i in 0..(w * h) {
            let v = ((i * 7) % 256) as u8;
            luma[i] = v;
            rgba[i * 4] = v;
            rgba[i * 4 + 1] = v;
            rgba[i * 4 + 2] = v;
            rgba[i * 4 + 3] = 255;
        }
        assert_eq!(phash_from_rgba(&rgba, w, h), phash_from_luma(&luma, w, h));
    }

    #[test]
    fn empty_image_is_zero() {
        assert_eq!(phash_from_luma(&[], 0, 0), [0u8; PHASH_BYTES]);
    }

    #[test]
    fn hamming_mismatched_lengths_is_max() {
        assert_eq!(hamming(&[0u8; 32], &[0u8; 16]), u32::MAX);
    }
}
