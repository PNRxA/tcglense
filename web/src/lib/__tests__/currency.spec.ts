import { describe, expect, it } from 'vitest'
import { convertUsd, formatConvertedUsd, isSupportedCurrency } from '../currency'

describe('currency display conversion', () => {
  it('recognises only the account currencies supported by the API', () => {
    expect(isSupportedCurrency('USD')).toBe(true)
    expect(isSupportedCurrency('AUD')).toBe(true)
    expect(isSupportedCurrency('aud')).toBe(false)
    expect(isSupportedCurrency('BTC')).toBe(false)
  })

  it('converts canonical USD values with the supplied daily rate', () => {
    expect(convertUsd('10.00', 'AUD', 1.52)).toBe('15.2')
    expect(convertUsd('0.25', 'JPY', 150)).toBe('37.5')
    expect(convertUsd(null, 'AUD', 1.52)).toBeNull()
  })

  it('formats the converted value with the selected currency', () => {
    expect(formatConvertedUsd('10', 'JPY', 150)).toContain('1,500')
    expect(formatConvertedUsd(null, 'JPY', 150)).toBeNull()
  })

  it('falls back honestly to USD when no conversion rate is available', () => {
    expect(formatConvertedUsd('12.5', 'AUD', null)).toBe('$12.50')
    expect(convertUsd('12.5', 'AUD', null)).toBe('12.5')
  })
})
