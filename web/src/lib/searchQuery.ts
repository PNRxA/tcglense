// Low-level helpers for reading and editing individual filter tokens inside a
// Scryfall-style query string, without disturbing the rest of it. The advanced-search
// builder (AdvancedSearchPanel.vue) uses these to reflect the live search box as
// point-and-click controls and to write chosen filters back into it: each control
// "owns" a set of keys (e.g. the colour control owns `c`/`color`/`colors`) and only
// ever touches its own non-negated `key op value` tokens, leaving free text, quoted
// phrases, parenthesised groups, regexes and negated tokens (`-t:land`) verbatim.
//
// These are deliberately string-level, not a re-implementation of the backend parser
// (api/src/scryfall/search/): they recognise the flat `key op value` token shape the
// builder emits and pattern-match it back, which is enough to round-trip the handful
// of filters the panel exposes while being non-destructive to everything else.

/** A `key op value` token split into its parts (key lowercased, value verbatim). */
export interface ParsedToken {
  /** True when the token was negated with a leading `-` (e.g. `-t:land`). */
  neg: boolean
  key: string
  /** One of `:` `=` `!=` `>` `>=` `<` `<=`. */
  op: string
  value: string
}

// Optional `-`, a letters-only key, then the longest matching operator (two-char ops
// listed first so `>=` wins over `>`), then the rest as the value. A bareword, quoted
// phrase, `!"exact"`, `/regex/` or `(group` has no leading key+operator, so it fails
// this and is treated as opaque "other" text.
const TOKEN_RE = /^(-)?([a-z]+)(!=|>=|<=|[:=><])(.*)$/i

/**
 * Split a query into whitespace-separated tokens, keeping a `"quoted phrase"` (and a
 * quoted filter value like `o:"draw a card"`) intact as a single token so an edit
 * never severs it.
 */
export function tokenizeQuery(query: string): string[] {
  const tokens: string[] = []
  let current = ''
  let inQuote = false
  for (const ch of query) {
    if (ch === '"') {
      inQuote = !inQuote
      current += ch
    } else if (!inQuote && /\s/.test(ch)) {
      if (current) {
        tokens.push(current)
        current = ''
      }
    } else {
      current += ch
    }
  }
  if (current) tokens.push(current)
  return tokens
}

/** Parse a `key op value` token, or null when it isn't one (a name term, group, …). */
export function parseToken(token: string): ParsedToken | null {
  const match = TOKEN_RE.exec(token)
  if (!match) return null
  const [, neg, key, op, value] = match
  // The key/op groups always capture when the regex matches; a key with no value
  // (`t:`) isn't a usable filter, so treat it — and any impossible empty group — as opaque.
  if (!key || !op || !value) return null
  // A structural paren means the token is inside a `(group)` — e.g. the trailing `)` in
  // `(t:creature or t:artifact)` glues onto `t:artifact)`. The builder never emits parens,
  // so treat any paren-bearing value as opaque, leaving the group intact rather than
  // editing off its closing `)` and unbalancing the query.
  if (value.includes('(') || value.includes(')')) return null
  return { neg: neg === '-', key: key.toLowerCase(), op, value }
}

/** Rejoin tokens with single spaces, trimmed — the canonical form the builder writes. */
function join(tokens: string[]): string {
  return tokens.join(' ').trim()
}

function keyMatches(
  parsed: ParsedToken,
  keys: readonly string[],
  ops?: readonly string[],
): boolean {
  if (parsed.neg) return false // the builder only manages positive tokens
  if (!keys.includes(parsed.key)) return false
  return ops ? ops.includes(parsed.op) : true
}

/**
 * The first non-negated token whose key is one of `keys` (and, if `ops` is given,
 * whose operator is one of them), or null. Case-insensitive on the key.
 */
export function readFilter(
  query: string,
  keys: readonly string[],
  ops?: readonly string[],
): ParsedToken | null {
  for (const token of tokenizeQuery(query)) {
    const parsed = parseToken(token)
    if (parsed && keyMatches(parsed, keys, ops)) return parsed
  }
  return null
}

/** Drop every non-negated token matching `keys` (optionally narrowed to `ops`). */
export function removeFilter(
  query: string,
  keys: readonly string[],
  ops?: readonly string[],
): string {
  const kept = tokenizeQuery(query).filter((token) => {
    const parsed = parseToken(token)
    return !(parsed && keyMatches(parsed, keys, ops))
  })
  return join(kept)
}

/**
 * Replace-or-append a single-valued filter: remove the builder's existing tokens for
 * `keys`, then (when `value` is non-empty) append `canonicalKey + op + value`. An empty
 * value just clears the filter.
 */
export function upsertFilter(
  query: string,
  keys: readonly string[],
  canonicalKey: string,
  op: string,
  value: string,
): string {
  const base = removeFilter(query, keys)
  if (!value) return base
  const token = `${canonicalKey}${op}${value}`
  return base ? `${base} ${token}` : token
}

/**
 * Whether a non-negated `key:value` flag token is present — for multi-valued keys like
 * `is:`, where several values coexist (`is:foil is:promo`) and each toggle owns only
 * its own value, never the whole key. Value comparison is case-insensitive.
 */
export function hasFlag(query: string, keys: readonly string[], value: string): boolean {
  const wanted = value.toLowerCase()
  for (const token of tokenizeQuery(query)) {
    const parsed = parseToken(token)
    if (parsed && keyMatches(parsed, keys, [':']) && parsed.value.toLowerCase() === wanted)
      return true
  }
  return false
}

/**
 * Add or remove a single `key:value` flag token, leaving the key's other values (and
 * negated tokens) untouched — the multi-valued counterpart of upsertFilter.
 */
export function setFlag(
  query: string,
  keys: readonly string[],
  canonicalKey: string,
  value: string,
  on: boolean,
): string {
  const wanted = value.toLowerCase()
  const kept = tokenizeQuery(query).filter((token) => {
    const parsed = parseToken(token)
    return !(parsed && keyMatches(parsed, keys, [':']) && parsed.value.toLowerCase() === wanted)
  })
  if (on) kept.push(`${canonicalKey}:${value}`)
  return join(kept)
}

/** The `{ min, max }` of a numeric range filter, read from its `>`/`>=` and `<`/`<=`
 * tokens (empty string when a bound isn't set). */
export function readRange(query: string, keys: readonly string[]): { min: string; max: string } {
  const min = readFilter(query, keys, ['>', '>='])
  const max = readFilter(query, keys, ['<', '<='])
  return { min: min?.value ?? '', max: max?.value ?? '' }
}

/**
 * Set both bounds of a numeric range at once: the range control owns the key, so this
 * removes *all* the builder's tokens for `keys` (any operator) and re-appends `key>=min`
 * and/or `key<=max` for whichever bound is given. Passing both empty clears the filter.
 */
export function setRange(
  query: string,
  keys: readonly string[],
  canonicalKey: string,
  min: string,
  max: string,
): string {
  let next = removeFilter(query, keys)
  if (min) next = join([...tokenizeQuery(next), `${canonicalKey}>=${min}`])
  if (max) next = join([...tokenizeQuery(next), `${canonicalKey}<=${max}`])
  return next
}
