import { describe, expect, it } from 'vitest'
import { DEFAULT_GHOST_STYLE, isGhostStyle } from '../ghostDisplay'

describe('isGhostStyle', () => {
  it('accepts the known styles', () => {
    expect(isGhostStyle('grayscale')).toBe(true)
    expect(isGhostStyle('color')).toBe(true)
  })

  it('rejects anything else (incl. the raw null/undefined a storage read can yield)', () => {
    expect(isGhostStyle('greyscale')).toBe(false)
    expect(isGhostStyle('colour')).toBe(false)
    expect(isGhostStyle('')).toBe(false)
    expect(isGhostStyle(null)).toBe(false)
    expect(isGhostStyle(undefined)).toBe(false)
    expect(isGhostStyle(1)).toBe(false)
  })

  it('defaults to grayscale', () => {
    expect(DEFAULT_GHOST_STYLE).toBe('grayscale')
    expect(isGhostStyle(DEFAULT_GHOST_STYLE)).toBe(true)
  })
})
