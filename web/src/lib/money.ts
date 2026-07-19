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

/**
 * Sum canonical USD decimal strings, skipping any `null`/`undefined`/empty entries (the API
 * sends those when nothing is priced). Returns a 2-dp canonical string ready for
 * {@link formatUsd}, or `null` when every input is absent — so the formatted result
 * self-hides. Used to roll the cards + sealed-product values into the landing's combined
 * total.
 */
export function sumUsd(...values: (string | null | undefined)[]): string | null {
  const present = values.filter((v): v is string => !!v)
  if (!present.length) return null
  const total = present.reduce((acc, v) => acc + Number(v), 0)
  return Number.isFinite(total) ? total.toFixed(2) : null
}
