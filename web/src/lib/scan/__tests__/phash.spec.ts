import { describe, expect, it } from 'vitest'
import { hamming, phashFromLuma, phashFromRgba, toHex, PHASH_BYTES } from '../phash'

// Cross-language parity for the perceptual hash. These synthetic-luma generators are
// bit-for-bit mirrors of the Rust suite's `synth_luma` (api/src/phash/mod.rs), and the
// GOLDEN_* hexes below are the exact values the Rust `golden_hashes_are_stable` test
// pins. If either side changes, regenerate BOTH (the algorithm and, if touched, the
// shared basis via `scripts/gen_phash_basis.py`) and update both suites.

function synthLuma(kind: 'grad' | 'checker' | 'lcg', width: number, height: number): Uint8Array {
  const out = new Uint8Array(width * height)
  if (kind === 'grad') {
    const denom = Math.max(width - 1, 1)
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) out[y * width + x] = Math.floor((x * 255) / denom)
    }
  } else if (kind === 'checker') {
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        out[y * width + x] = ((x >> 3) + (y >> 3)) & 1 ? 255 : 0
      }
    }
  } else {
    // 32-bit unsigned LCG, matching Rust's wrapping u32 arithmetic via Math.imul.
    let s = (0x12345678 + width) >>> 0
    s = Math.imul(s, 2654435761) >>> 0
    s = (s + height) >>> 0
    for (let i = 0; i < out.length; i++) {
      s = (Math.imul(s, 1664525) + 1013904223) >>> 0
      out[i] = (s >>> 24) & 0xff
    }
  }
  return out
}

// Pinned goldens — identical to the Rust suite's GOLDEN_* constants.
const GOLDEN_GRAD = '8aa888847457626370578aa88aa87557755775578aaa75577557aaaa8aa87557'
const GOLDEN_CHECKER = 'fffeebaa88a8aaaa88aaaaaa88aa88a8aaaa88a8aaaaaaaa88a8ffff88aaeaaa'
const GOLDEN_LCG = '8de06a3b6e022449d537b5954e8bd9fe27830b71232d9c2a96c159ffb143b645'

describe('phash cross-language parity', () => {
  it('matches the Rust reference goldens byte-for-byte', () => {
    expect(toHex(phashFromLuma(synthLuma('grad', 146, 204), 146, 204))).toBe(GOLDEN_GRAD)
    expect(toHex(phashFromLuma(synthLuma('checker', 146, 204), 146, 204))).toBe(GOLDEN_CHECKER)
    expect(toHex(phashFromLuma(synthLuma('lcg', 64, 64), 64, 64))).toBe(GOLDEN_LCG)
  })
})

describe('phash', () => {
  it('produces a 32-byte (256-bit) hash', () => {
    expect(phashFromLuma(synthLuma('lcg', 100, 100), 100, 100)).toHaveLength(PHASH_BYTES)
  })

  it('is deterministic for identical input', () => {
    const a = phashFromLuma(synthLuma('lcg', 146, 204), 146, 204)
    const b = phashFromLuma(synthLuma('lcg', 146, 204), 146, 204)
    expect(hamming(a, b)).toBe(0)
  })

  it('separates visually distinct images', () => {
    const a = phashFromLuma(synthLuma('grad', 146, 204), 146, 204)
    const b = phashFromLuma(synthLuma('checker', 146, 204), 146, 204)
    expect(hamming(a, b)).toBeGreaterThan(20)
  })

  it('hashes gray RGBA the same as the equivalent luma plane', () => {
    const w = 80
    const h = 100
    const rgba = new Uint8Array(w * h * 4)
    const luma = new Uint8Array(w * h)
    for (let i = 0; i < w * h; i++) {
      const v = (i * 7) % 256
      luma[i] = v
      rgba[i * 4] = v
      rgba[i * 4 + 1] = v
      rgba[i * 4 + 2] = v
      rgba[i * 4 + 3] = 255
    }
    expect(toHex(phashFromRgba(rgba, w, h))).toBe(toHex(phashFromLuma(luma, w, h)))
  })

  it('returns Infinity Hamming distance for mismatched lengths', () => {
    expect(hamming(new Uint8Array(32), new Uint8Array(16))).toBe(Infinity)
  })
})
