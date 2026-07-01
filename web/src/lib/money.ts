/**
 * Format a decimal USD string exactly as the API returns it (e.g. `"1234.5"`) as a
 * localized currency string (`"$1,234.50"`), or `null` when there's nothing to show
 * (`null`/`undefined`/empty — what the API sends when nothing owned is priced). Falls back
 * to a bare `$`-prefixed string if the value somehow isn't a finite number, so a malformed
 * value never renders as `NaN`.
 *
 * Shared by every collection value display (the landing header + per-set tiles, and the
 * scoped value next to the browse count) so they format identically.
 */
export function formatUsd(raw: string | null | undefined): string | null {
  if (!raw) return null
  const n = Number(raw)
  return Number.isFinite(n)
    ? n.toLocaleString(undefined, { style: 'currency', currency: 'USD' })
    : `$${raw}`
}
