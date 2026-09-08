import { describe, expect, it } from 'vitest'
import {
  ALERT_FINISHES,
  ALERT_FINISHES_BY_KIND,
  ALERT_FINISH_LABELS,
  cardAlertFinishes,
  productAlertFinishes,
} from '../alertFinishes'

// Mirrors `handlers::alerts::validate_finish` (api/src/handlers/alerts/mod.rs), whose unit
// test pins the same table — a finish added on one side only is either rejected by the API
// or never offered by the picker.
describe('alert finish vocabulary', () => {
  it('lists three finishes for a card and two for a sealed product, in picker order', () => {
    expect(ALERT_FINISHES).toEqual(['nonfoil', 'foil', 'etched'])
    expect(ALERT_FINISHES_BY_KIND.card).toEqual(['nonfoil', 'foil', 'etched'])
    expect(ALERT_FINISHES_BY_KIND.product).toEqual(['nonfoil', 'foil'])
  })

  it('labels every finish', () => {
    for (const finish of ALERT_FINISHES) {
      expect(ALERT_FINISH_LABELS[finish]).toBeTruthy()
    }
    expect(ALERT_FINISH_LABELS.etched).toBe('Etched')
  })
})

describe('cardAlertFinishes', () => {
  it('offers exactly the finishes the card is priced in, etched included', () => {
    expect(cardAlertFinishes({ usd: '1.00', usd_foil: '2.00', usd_etched: '3.00' })).toEqual([
      'nonfoil',
      'foil',
      'etched',
    ])
    expect(cardAlertFinishes({ usd: '1.00', usd_foil: null, usd_etched: '3.00' })).toEqual([
      'nonfoil',
      'etched',
    ])
    expect(cardAlertFinishes({ usd: null, usd_foil: null, usd_etched: '3.00' })).toEqual(['etched'])
  })

  it('never offers etched to a card without an etched price', () => {
    // The evaluator reads `price_usd_etched` and only that column, so an etched alert on a
    // foil-only card could never fire — the picker must not arm one.
    expect(cardAlertFinishes({ usd: '1.00', usd_foil: '2.00', usd_etched: null })).toEqual([
      'nonfoil',
      'foil',
    ])
    expect(cardAlertFinishes({ usd: '1.00', usd_foil: '2.00' })).toEqual(['nonfoil', 'foil'])
  })

  it('falls back to regular for a fully unpriced (or missing) card', () => {
    expect(cardAlertFinishes({ usd: null, usd_foil: null, usd_etched: null })).toEqual(['nonfoil'])
    expect(cardAlertFinishes(undefined)).toEqual(['nonfoil'])
  })
})

describe('productAlertFinishes', () => {
  it('watches the regular price, or foil only when that is the sole priced column', () => {
    expect(productAlertFinishes({ usd: '4.50', usd_foil: null })).toEqual(['nonfoil'])
    expect(productAlertFinishes({ usd: '4.50', usd_foil: '9.00' })).toEqual(['nonfoil'])
    expect(productAlertFinishes({ usd: null, usd_foil: '9.00' })).toEqual(['foil'])
    expect(productAlertFinishes({ usd: null, usd_foil: null })).toEqual(['nonfoil'])
    expect(productAlertFinishes(undefined)).toEqual(['nonfoil'])
  })
})
