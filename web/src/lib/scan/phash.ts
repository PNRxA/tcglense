// Deterministic 256-bit perceptual hash (DCT / "pHash") for a photographed card.
//
// A byte-for-byte twin of the Rust reference hasher (`api/src/phash/mod.rs`) that
// builds the fingerprint index: same integer grayscale + box downsample, same 2D
// DCT against the SHARED basis constants (`./phash-basis`, generated for both
// languages by `scripts/gen_phash_basis.py`), same median threshold and bit packing.
// So a hash computed here is directly comparable — by Hamming distance — to a
// reference hash built in Rust. The parity is pinned by golden fixtures in
// `__tests__/phash.spec.ts` (identical to the Rust suite's goldens).
//
// Pipeline: luma → 32×32 box-average → 2D DCT-II → keep the top-left 16×16
// low-frequency block (256 coefficients) → threshold each against the block median →
// pack MSB-first into 32 bytes. Buffers are flat typed arrays indexed row-major.

import { DCT_BASIS, DCT_N } from './phash-basis'

/** Length of a fingerprint in bytes (256 bits). */
export const PHASH_BYTES = 32

/** Side of the low-frequency DCT block kept for the hash (16×16 = 256 bits). */
const LOW_FREQ = 16

// Integer luma weights (sum to 256), matching the Rust twin so grayscale is identical.
const LUMA_R = 77
const LUMA_G = 150
const LUMA_B = 29

/** Hash a grayscale (luma) plane of `width`×`height` bytes into a 256-bit pHash
 * (32 bytes). Returns all-zero for an empty image. */
export function phashFromLuma(luma: Uint8Array, width: number, height: number): Uint8Array {
  if (width === 0 || height === 0 || luma.length < width * height) {
    return new Uint8Array(PHASH_BYTES)
  }
  return threshold(dctLowFreq(downsample(luma, width, height)))
}

/** Hash a packed RGBA (4 bytes/pixel) buffer — grayscale first, then
 * {@link phashFromLuma}. The alpha channel is ignored. This is the entry the scanner
 * uses on a canvas `ImageData.data`. */
export function phashFromRgba(
  rgba: Uint8ClampedArray | Uint8Array,
  width: number,
  height: number,
): Uint8Array {
  const n = width * height
  if (n === 0 || rgba.length < n * 4) return new Uint8Array(PHASH_BYTES)
  const luma = new Uint8Array(n)
  for (let i = 0; i < n; i++) {
    const r = rgba[i * 4]!
    const g = rgba[i * 4 + 1]!
    const b = rgba[i * 4 + 2]!
    luma[i] = (r * LUMA_R + g * LUMA_G + b * LUMA_B) >> 8
  }
  return phashFromLuma(luma, width, height)
}

/** Box-average `luma` down to a flat `DCT_N`×`DCT_N` grid of integer averages
 * (0..255). Each output cell averages the source pixels in its floor-partitioned box;
 * a box is guaranteed non-empty even when the source is smaller than `DCT_N`. */
function downsample(luma: Uint8Array, width: number, height: number): Float64Array {
  const grid = new Float64Array(DCT_N * DCT_N)
  for (let ty = 0; ty < DCT_N; ty++) {
    const y0 = Math.floor((ty * height) / DCT_N)
    let y1 = Math.floor(((ty + 1) * height) / DCT_N)
    if (y1 <= y0) y1 = y0 + 1
    if (y1 > height) y1 = height
    for (let tx = 0; tx < DCT_N; tx++) {
      const x0 = Math.floor((tx * width) / DCT_N)
      let x1 = Math.floor(((tx + 1) * width) / DCT_N)
      if (x1 <= x0) x1 = x0 + 1
      if (x1 > width) x1 = width
      let sum = 0
      for (let y = y0; y < y1; y++) {
        const row = y * width
        for (let x = x0; x < x1; x++) sum += luma[row + x]!
      }
      const count = (y1 - y0) * (x1 - x0)
      grid[ty * DCT_N + tx] = Math.floor(sum / count)
    }
  }
  return grid
}

/** 2D DCT-II of the 32×32 grid, keeping only the top-left `LOW_FREQ`×`LOW_FREQ`
 * coefficients (row-major). Separable: transform along x, then along y, against the
 * shared basis — identical multiply-add order to the Rust twin. */
function dctLowFreq(grid: Float64Array): Float64Array {
  // Rows: T[y][v] = sum_x BASIS[v][x] * grid[y][x], for v in 0..LOW_FREQ.
  const t = new Float64Array(DCT_N * LOW_FREQ)
  for (let y = 0; y < DCT_N; y++) {
    for (let v = 0; v < LOW_FREQ; v++) {
      let acc = 0
      for (let x = 0; x < DCT_N; x++) acc += DCT_BASIS[v * DCT_N + x]! * grid[y * DCT_N + x]!
      t[y * LOW_FREQ + v] = acc
    }
  }
  // Columns: F[u][v] = sum_y BASIS[u][y] * T[y][v], for u in 0..LOW_FREQ.
  const coeffs = new Float64Array(LOW_FREQ * LOW_FREQ)
  for (let u = 0; u < LOW_FREQ; u++) {
    for (let v = 0; v < LOW_FREQ; v++) {
      let acc = 0
      for (let y = 0; y < DCT_N; y++) acc += DCT_BASIS[u * DCT_N + y]! * t[y * LOW_FREQ + v]!
      coeffs[u * LOW_FREQ + v] = acc
    }
  }
  return coeffs
}

/** Threshold each coefficient against the median of all 256 and pack MSB-first into
 * 32 bytes. The median is the mean of the two middle values of the sorted set. A typed
 * array's `sort()` is numeric (unlike `Array`'s lexicographic default), matching Rust. */
function threshold(coeffs: Float64Array): Uint8Array {
  const sorted = coeffs.slice().sort()
  const mid = sorted.length / 2
  const median = (sorted[mid - 1]! + sorted[mid]!) / 2
  const out = new Uint8Array(PHASH_BYTES)
  for (let i = 0; i < coeffs.length; i++) {
    if (coeffs[i]! > median) {
      const byte = i >> 3
      out[byte] = out[byte]! | (1 << (7 - (i & 7)))
    }
  }
  return out
}

/** Hamming distance (differing bits) between two equal-length byte strings. Returns
 * `Infinity` when the lengths differ (never a real match). */
export function hamming(a: Uint8Array, b: Uint8Array): number {
  if (a.length !== b.length) return Infinity
  let dist = 0
  for (let i = 0; i < a.length; i++) {
    let x = a[i]! ^ b[i]!
    // Brian Kernighan popcount over a single byte.
    while (x) {
      x &= x - 1
      dist++
    }
  }
  return dist
}

/** Lowercase hex of a fingerprint, for logging / a stable cache key. */
export function toHex(bytes: Uint8Array): string {
  let s = ''
  for (let i = 0; i < bytes.length; i++) s += bytes[i]!.toString(16).padStart(2, '0')
  return s
}
