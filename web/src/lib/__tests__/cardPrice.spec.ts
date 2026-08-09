import { describe, it, expect } from 'vitest'

import type { CardPrices } from '../api'
import { displayUsdPrice, finishUsdPrice } from '../cardPrice'

function prices(p: Partial<CardPrices>): CardPrices {
  return { usd: null, usd_foil: null, eur: null, tix: null, ...p }
}

describe('displayUsdPrice', () => {
  it('uses the regular USD price when present', () => {
    expect(displayUsdPrice(prices({ usd: '5.00', usd_foil: '50.00' }))).toEqual({
      amount: '5.00',
      foil: false,
    })
  })

  it('falls back to the foil price when there is no regular USD price', () => {
    expect(displayUsdPrice(prices({ usd: null, usd_foil: '19.99' }))).toEqual({
      amount: '19.99',
      foil: true,
    })
  })

  it('prefers the regular price even when a foil price also exists', () => {
    const result = displayUsdPrice(prices({ usd: '1.00', usd_foil: '2.00' }))
    expect(result?.foil).toBe(false)
  })

  it('treats an empty-string regular price as absent and uses the foil price', () => {
    expect(displayUsdPrice(prices({ usd: '', usd_foil: '7.50' }))).toEqual({
      amount: '7.50',
      foil: true,
    })
  })

  it('returns null when neither USD price is set', () => {
    expect(displayUsdPrice(prices({}))).toBeNull()
  })

  it('also accepts the bare USD shape a sealed product carries (no eur/tix)', () => {
    expect(displayUsdPrice({ usd: '99.99', usd_foil: null })).toEqual({
      amount: '99.99',
      foil: false,
    })
    expect(displayUsdPrice({ usd: null, usd_foil: null })).toBeNull()
  })
})

describe('finishUsdPrice', () => {
  it('prices a foil copy at the foil price, not the regular one', () => {
    expect(finishUsdPrice(prices({ usd: '5.00', usd_foil: '50.00' }), true)).toEqual({
      amount: '50.00',
      foil: true,
    })
  })

  it('prices a regular copy the same way a tile does, even when a foil price exists', () => {
    expect(finishUsdPrice(prices({ usd: '5.00', usd_foil: '50.00' }), false)).toEqual({
      amount: '5.00',
      foil: false,
    })
  })

  it('falls back to the regular price for a foil copy of an unfoiled printing', () => {
    // Better a number, correctly flagged as the regular price, than a blank row — the flag
    // is what stops the fallback reading as the foil price it is not.
    expect(finishUsdPrice(prices({ usd: '2.00', usd_foil: null }), true)).toEqual({
      amount: '2.00',
      foil: false,
    })
  })

  it('falls back to the foil price for a regular copy of a foil-only printing', () => {
    expect(finishUsdPrice(prices({ usd: null, usd_foil: '19.99' }), false)).toEqual({
      amount: '19.99',
      foil: true,
    })
  })

  it('returns null when the printing carries no USD price at all', () => {
    expect(finishUsdPrice(prices({}), true)).toBeNull()
    expect(finishUsdPrice(prices({}), false)).toBeNull()
  })
})
