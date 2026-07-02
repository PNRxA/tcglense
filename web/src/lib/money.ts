/**
 * Format a decimal USD string exactly as the API returns it (e.g. `"1234.5"`) as a
 * `$`-prefixed, thousands-grouped 2-dp amount (`"$1,234.50"`), or `null` when there's
 * nothing to show (`null`/`undefined`/empty — what the API sends when nothing owned is
 * priced). Falls back to a bare `$`-prefixed string if the value somehow isn't a finite
 * number, so a malformed value never renders as `NaN`.
 *
 * The `$` is emitted literally (rather than via `Intl` currency formatting, which renders
 * `"US$"` / `"USD"` in some locales) so every value reads as a plain `$X`. Only the digit
 * grouping / decimals follow the locale.
 *
 * Shared by every collection value display (the landing header + per-set tiles, and the
 * scoped value next to the browse count) so they format identically.
 */
export function formatUsd(raw: string | null | undefined): string | null {
  if (!raw) return null
  const n = Number(raw)
  if (!Number.isFinite(n)) return `$${raw}`
  return `$${n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}
