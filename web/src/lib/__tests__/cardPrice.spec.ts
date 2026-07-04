import { describe, it, expect } from 'vitest'

import type { CardPrices } from '../api'
import { displayUsdPrice } from '../cardPrice'

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
