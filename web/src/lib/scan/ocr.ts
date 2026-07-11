// Pure text helpers for the card scanner's OCR pipeline. Kept out of the composable so
// the fiddly cleaning + hint-parsing rules are unit-tested, not buried in a component.
//
// The scanner OCRs two strips of a Magic card: the top title bar (the name) and the
// bottom-left info line (collector number + set code). OCR of small card text is noisy,
// so nothing here is trusted to *commit* a card — the name is resolved against the
// catalog autocomplete (which the user confirms) and the set/number are only ever used
// to *prefer* a printing.

/** Hints parsed from a modern (2015+) MTG card's bottom-left info line. Either field may
 * be absent when the OCR is too noisy; both are advisory (they pre-select a printing). */
export interface SetHint {
  /** Uppercased set code, e.g. `NEO`, `MH2`, `40K` — or undefined if unreadable. */
  setCode?: string
  /** Collector number with any zero-padding stripped, e.g. `123` — or undefined. */
  collectorNumber?: string
}

/** Minimum length of a cleaned name before it's worth querying the catalog. */
export const MIN_NAME_LENGTH = 3

/**
 * Clean a raw OCR'd title-bar line into a query for the name autocomplete: normalise
 * curly apostrophes, collapse whitespace/newlines, drop characters a card name never
 * contains (OCR loves to hallucinate mana pips and frame glyphs into the strip), and
 * trim stray leading/trailing punctuation. The autocomplete matches case-insensitive
 * *substrings*, so minor edge noise is tolerated as long as a clean run survives.
 */
export function cleanCardName(raw: string): string {
  return (
    raw
      .replace(/[’`´]/g, "'")
      .replace(/\s+/g, ' ')
      // Keep letters (with combining accents), digits, spaces, and the punctuation that
      // actually shows up in card names ( ' , - ! & . ); replace everything else with a space.
      .replace(/[^\p{L}\p{M}\p{N} '\-,!&.]/gu, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      // Trim leading/trailing punctuation that isn't part of a name (a name starts with a
      // letter/number; a trailing lone symbol is OCR noise).
      .replace(/^[^\p{L}\p{N}]+/u, '')
      .replace(/[^\p{L}\p{N}!)]+$/u, '')
      .trim()
  )
}

/**
 * Progressively shorter autocomplete queries for a cleaned name, most specific first.
 * The full string is tried first; if the OCR ran the type line into the title (making
 * the query longer than any real name, so a substring match fails) the leading 3- then
 * 2-word prefixes give a fallback that still resolves the common case. Deduped; only
 * entries of a useful length are kept.
 */
export function nameQueryCandidates(cleaned: string): string[] {
  const words = cleaned.split(' ').filter(Boolean)
  const out: string[] = []
  const push = (s: string) => {
    const t = s.trim()
    if (t.length >= MIN_NAME_LENGTH && !out.includes(t)) out.push(t)
  }
  push(cleaned)
  if (words.length > 3) push(words.slice(0, 3).join(' '))
  if (words.length > 2) push(words.slice(0, 2).join(' '))
  return out
}

/** Language codes printed on the info line — excluded when hunting for the set code, so
 * `NEO • EN` yields `NEO`, not `EN`. */
const LANGUAGE_CODES = new Set([
  'EN',
  'DE',
  'FR',
  'IT',
  'ES',
  'PT',
  'JA',
  'JP',
  'KO',
  'RU',
  'ZH',
  'ZHS',
  'ZHT',
  'CS',
  'PH',
  'HE',
  'LA',
  'AR',
  'GR',
  'SA',
])

/** Strip zero-padding from a collector number (`0123` -> `123`) while leaving a bare `0`
 * and any letter suffix (`0123a` -> `123a`) intact. */
export function normalizeCollectorNumber(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/^0+(?=[0-9])/, '')
}

/**
 * Best-effort parse of the bottom-left info strip — printed as `123/264 R` over
 * `SET • EN` (plus the artist) — into a set code + collector number. Returns whatever it
 * can read; both fields are optional and advisory.
 */
export function parseSetHint(raw: string): SetHint {
  const upper = raw.replace(/[|]/g, ' ').replace(/\s+/g, ' ').trim().toUpperCase()
  const hint: SetHint = {}

  // Collector number: the "123/264" form is unambiguous (a bare number could be the
  // card's power/toughness or CMC bleeding in, so only trust the slashed form).
  const numbered = upper.match(/(\d{1,5})\s*\/\s*\d{1,5}/)?.[1]
  if (numbered) hint.collectorNumber = normalizeCollectorNumber(numbered)

  // Set code: the first *whole* 3-5 char alphanumeric token that isn't a language code
  // or a pure number (the collector total). Splitting on non-alnum (rather than matching
  // a 3-5 run) avoids picking a fragment of a longer word like an artist's name. A false
  // positive here is harmless — it only *pre-selects* a printing, and a bogus code that
  // matches none of the current card's printings simply falls back to the newest.
  for (const token of upper.split(/[^A-Z0-9]+/)) {
    if (token.length < 3 || token.length > 5) continue
    if (LANGUAGE_CODES.has(token)) continue
    if (/^\d+$/.test(token)) continue
    hint.setCode = token
    break
  }

  return hint
}

/**
 * Whether two cleaned OCR name strings plausibly describe the same physical card still
 * held in front of the camera — used by the live loop to avoid re-resolving (and, worse,
 * re-committing) a card that hasn't changed. Exact match, or one a prefix of the other
 * (OCR trims/extends the last word between frames).
 */
export function sameCardText(a: string, b: string): boolean {
  const x = a.trim().toLowerCase()
  const y = b.trim().toLowerCase()
  if (!x || !y) return false
  if (x === y) return true
  const [short, long] = x.length <= y.length ? [x, y] : [y, x]
  return short.length >= MIN_NAME_LENGTH && long.startsWith(short)
}

/**
 * Stricter variant for the live loop's "the current card is still held up" gate. Like
 * {@link sameCardText}, but a read that appends a whole *new word* — e.g. "Island" vs
 * "Island Sanctuary" — is NOT treated as the same card: those are distinct real cards, so
 * it must fall through to the catalog-name comparison rather than being silently suppressed.
 * A truncated last word (no trailing space after the shared prefix, e.g. "Lightning Bol" vs
 * "Lightning Bolt") is still the same card — that's ordinary per-frame OCR jitter.
 */
export function isSameHeldCard(a: string, b: string): boolean {
  if (!sameCardText(a, b)) return false
  const x = a.trim().toLowerCase()
  const y = b.trim().toLowerCase()
  if (x === y) return true
  const [short, long] = x.length <= y.length ? [x, y] : [y, x]
  return !long.slice(short.length).startsWith(' ')
}
