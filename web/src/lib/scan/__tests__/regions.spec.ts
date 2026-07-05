import { describe, it, expect } from 'vitest'
import {
  CARD_ASPECT,
  NAME_REGION,
  SET_REGION,
  guideRect,
  rectToPercentStyle,
  regionInRect,
} from '../regions'

describe('guideRect', () => {
  it('centres a 61:85 box inside a portrait viewport (width-limited)', () => {
    const rect = guideRect(400, 800, 0)
    expect(rect.width).toBe(400)
    expect(rect.height).toBeCloseTo(400 / CARD_ASPECT, 5)
    expect(rect.left).toBe(0)
    expect(rect.top).toBeCloseTo((800 - 400 / CARD_ASPECT) / 2, 5)
  })

  it('centres a 61:85 box inside a landscape viewport (height-limited)', () => {
    const rect = guideRect(1600, 900, 0)
    expect(rect.height).toBe(900)
    expect(rect.width).toBeCloseTo(900 * CARD_ASPECT, 5)
    expect(rect.top).toBe(0)
    expect(rect.left).toBeCloseTo((1600 - 900 * CARD_ASPECT) / 2, 5)
  })

  it('applies a proportional margin on the limiting axis', () => {
    const rect = guideRect(400, 800, 0.1)
    expect(rect.width).toBeCloseTo(320, 5)
    expect(rect.left).toBeCloseTo(40, 5)
  })

  it('keeps the card aspect ratio', () => {
    const rect = guideRect(1234, 987)
    expect(rect.width / rect.height).toBeCloseTo(CARD_ASPECT, 5)
  })
})

describe('regionInRect', () => {
  it('maps a fractional region into an absolute parent rect', () => {
    const parent = { left: 100, top: 200, width: 400, height: 800 }
    const region = regionInRect({ left: 0.05, top: 0.036, width: 0.8, height: 0.086 }, parent)
    expect(region.left).toBeCloseTo(120, 5)
    expect(region.top).toBeCloseTo(200 + 0.036 * 800, 5)
    expect(region.width).toBeCloseTo(320, 5)
    expect(region.height).toBeCloseTo(0.086 * 800, 5)
  })

  it('keeps the name region above the set region', () => {
    expect(NAME_REGION.top + NAME_REGION.height).toBeLessThan(SET_REGION.top)
  })
})

describe('rectToPercentStyle', () => {
  it('renders each fraction as a CSS percentage', () => {
    expect(rectToPercentStyle({ left: 0.05, top: 0.9, width: 0.6, height: 0.1 })).toEqual({
      left: '5%',
      top: '90%',
      width: '60%',
      height: '10%',
    })
  })
})
