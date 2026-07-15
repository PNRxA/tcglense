import { formatUsd } from './money'

export const CURRENCY_OPTIONS = [
  { code: 'USD', label: 'US Dollar' },
  { code: 'AUD', label: 'Australian Dollar' },
  { code: 'CAD', label: 'Canadian Dollar' },
  { code: 'EUR', label: 'Euro' },
  { code: 'GBP', label: 'British Pound' },
  { code: 'JPY', label: 'Japanese Yen' },
  { code: 'NZD', label: 'New Zealand Dollar' },
] as const

export type SupportedCurrency = (typeof CURRENCY_OPTIONS)[number]['code']

const SUPPORTED = new Set<string>(CURRENCY_OPTIONS.map((option) => option.code))

export function isSupportedCurrency(value: unknown): value is SupportedCurrency {
  return typeof value === 'string' && SUPPORTED.has(value)
}

/** Format a canonical USD amount in the selected currency at `usdRate` units per USD.
 * A missing rate deliberately falls back to the honest USD rendering rather than putting
 * a different currency symbol on an unconverted amount. */
export function formatConvertedUsd(
  raw: string | null | undefined,
  currency: SupportedCurrency,
  usdRate: number | null,
): string | null {
  if (currency === 'USD') return formatUsd(raw)
  if (usdRate == null) return formatExplicitUsd(raw)
  if (!raw) return null

  const amount = Number(raw)
  if (!Number.isFinite(amount)) return `${currency} ${raw}`
  return new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency,
    currencyDisplay: 'narrowSymbol',
  }).format(amount * usdRate)
}

/** A fallback for someone who selected another dollar currency. Plain `$12.50` can be
 * mistaken for AUD/CAD/NZD, so cold-feed failures always spell out the canonical unit. */
function formatExplicitUsd(raw: string | null | undefined): string | null {
  if (!raw) return null
  const amount = Number(raw)
  if (!Number.isFinite(amount)) return `USD ${raw}`
  return `USD ${amount.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`
}

/** Convert a canonical USD decimal string for charting while preserving null gaps. */
export function convertUsd(
  raw: string | null,
  currency: SupportedCurrency,
  usdRate: number | null,
): string | null {
  if (raw == null || currency === 'USD' || usdRate == null) return raw
  const amount = Number(raw)
  return Number.isFinite(amount) ? String(amount * usdRate) : raw
}
