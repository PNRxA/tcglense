// The "bulk" threshold: the per-card price under which a card counts toward the bulk
// slice of a collection's value (the low-value commons/uncommons shown separately from
// the total). Like the card size / theme, it's a personal display preference persisted in
// localStorage (see stores/bulkThreshold); it's also sent to the collection value
// endpoints so the server splits the bulk subtotal at the user's chosen cutoff (issue
// #289). Stored and sent in integer USD cents; shown to the user in dollars.

/** The default cutoff: $1.00, matching the server's default (`BULK_THRESHOLD_CENTS`). */
export const DEFAULT_BULK_THRESHOLD_CENTS = 100

/** Lower bound: $0, i.e. nothing counts as bulk. Mirrors the server clamp. */
export const MIN_BULK_THRESHOLD_CENTS = 0

/** Upper bound: $10,000. Well past any real single-card price; mirrors the server clamp. */
export const MAX_BULK_THRESHOLD_CENTS = 1_000_000

/**
 * Clamp an arbitrary cents value to a valid, whole-cent threshold in `[MIN, MAX]`. A
 * non-finite input (e.g. a cleared number field) falls back to the default rather than
 * becoming `NaN`, so the stored/sent value is always a legal integer.
 */
export function clampBulkThresholdCents(cents: number): number {
  if (!Number.isFinite(cents)) return DEFAULT_BULK_THRESHOLD_CENTS
  const whole = Math.round(cents)
  return Math.min(Math.max(whole, MIN_BULK_THRESHOLD_CENTS), MAX_BULK_THRESHOLD_CENTS)
}

/** Cents -> dollars, for display in the settings control. */
export function centsToDollars(cents: number): number {
  return cents / 100
}

/**
 * Dollars -> whole cents, clamped to the valid range (rounding any sub-cent input). A
 * non-finite input — including the `null` a number field emits when cleared, which would
 * otherwise coerce to `0 * 100` and read as "$0, nothing is bulk" — resolves to the
 * default rather than a surprise zero. (`Number.isFinite`, unlike global `isFinite`,
 * does not coerce, so it rejects `null`/`NaN`/`undefined` before the multiply.)
 */
export function dollarsToCents(dollars: number): number {
  if (!Number.isFinite(dollars)) return DEFAULT_BULK_THRESHOLD_CENTS
  return clampBulkThresholdCents(Math.round(dollars * 100))
}
