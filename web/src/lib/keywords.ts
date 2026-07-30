// Finding rules keywords inside a card's rules text, and the shared vocabulary the
// glossary surfaces speak.
//
// The definitions themselves are the API's ([`getKeywords`]); this module only decides
// *where in a card's text a keyword name is actually the keyword*, which is harder than
// it looks. Plenty of keyword names are ordinary English — a "+1/+1 counter" is not the
// keyword action Counter, "Storm Crow" is not Storm, and "Fear of Isolation" is not
// Fear. Guessing wrong doesn't just add clutter: it tells the reader something false.
//
// So each entry arrives from the API with a `match_mode` saying how far its own name
// can be trusted (see the Rust `MatchMode`), and this module applies four further
// guards on top:
//
//   1. **Longest name first** — `Flashback` must never be found as `Flash`, and
//      `Multikicker` never as `Kicker`.
//   2. **Reminder text is skipped** — text inside parentheses already explains the
//      keyword, so tooltipping inside it is circular ("Flying (This creature can't be
//      blocked except by creatures with flying…)" would mark that second "flying").
//   3. **The card's own name is skipped** — rules text refers to the card by its full
//      printed name, which routinely contains a keyword word.
//   4. **First mention only** — one tooltip per keyword per block of text. A card that
//      says "trample" four times gets one marker, not four.
//
// Pure and side-effect free, so it unit-tests without mounting anything — same posture
// as `lib/legality.ts` and `lib/deckRules.ts`.

import type { KeywordEntry, KeywordKind } from '@/lib/api'

/** The game whose glossary the app's `/keywords` pages serve. The routes are
 * game-flat (`/keywords/{slug}`) because a one-segment URL is what someone searching
 * "tcglense vigilance" lands on; a second game means game-scoped routes on both sides.
 * Mirrored by `GLOSSARY_GAME` in `api/src/handlers/sitemap.rs`, which advertises these
 * URLs — the two must name the same game. */
export const GLOSSARY_GAME = 'mtg'

/** Human labels for the three kinds of glossary entry. */
export const KIND_LABELS: Readonly<Record<KeywordKind, string>> = {
  ability: 'Keyword ability',
  action: 'Keyword action',
  ability_word: 'Ability word',
}

/** One-line explanations of what each kind *is*, shown on the glossary pages so a
 * reader knows why an ability word carries no rules weight. */
export const KIND_BLURBS: Readonly<Record<KeywordKind, string>> = {
  ability:
    'A named ability a permanent or spell has — the rules attach a meaning to the word itself.',
  action: 'A verb the rules define, so a card can say it in one word instead of a sentence.',
  ability_word:
    'An italic label with no rules meaning of its own. It just marks a recurring pattern, and the ' +
    'text after it is what actually does something.',
}

/** Derive a keyword's URL slug from its name.
 *
 * The API already sends the canonical slug on every entry — this exists to normalise a
 * slug a *user* typed or a search engine remembered (`/keywords/First-Strike`), so the
 * page can find the entry and redirect to the canonical spelling. It must stay in step
 * with `slugify` in `api/src/catalog/keywords/mod.rs`; both are pinned by the same
 * fixtures on either side. Idempotent on an already-canonical slug. */
export function keywordSlug(name: string): string {
  return name
    .toLowerCase()
    .replace(/['’]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/** The first sentence of an explanation, for a list tile or a meta description. */
export function firstSentence(text: string): string {
  const end = text.search(/\.\s/)
  return end === -1 ? text : text.slice(0, end + 1)
}

// ---------- Matching keywords in rules text ----------

/** One keyword found in a run of rules text. */
export interface KeywordMatch {
  /** Where the name starts in the run. */
  start: number
  /** One past where it ends. */
  end: number
  /** The glossary entry the matched text refers to. */
  entry: KeywordEntry
}

/** Escape a keyword name for embedding in a regular expression. */
function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/**
 * Spans of `text` the matcher must not look inside.
 *
 * Two kinds: parenthesised reminder text, and the card's own name. Both are computed
 * once per text run rather than re-tested per candidate match.
 */
function blockedSpans(text: string, cardName?: string): Array<[number, number]> {
  const spans: Array<[number, number]> = []
  for (const match of text.matchAll(/\([^)]*\)?/g)) {
    spans.push([match.index, match.index + match[0].length])
  }
  // A multi-faced card's rules text names the face, so try the whole name and each
  // "//"-separated face. Short names aren't worth blocking — a one-word card name that
  // *is* a keyword ("Flying Men") only shows up as the full name anyway.
  const names = cardName ? [cardName, ...cardName.split('//')] : []
  for (const name of names) {
    const trimmed = name.trim()
    if (trimmed.length < 3) continue
    for (const match of text.matchAll(new RegExp(escapeRegExp(trimmed), 'g'))) {
      spans.push([match.index, match.index + match[0].length])
    }
  }
  return spans
}

function isBlocked(spans: Array<[number, number]>, start: number, end: number): boolean {
  return spans.some(([from, to]) => start < to && end > from)
}

/**
 * Whether a match at `[start, end)` sits in *keyword position* — the rule
 * `match_mode: 'ability_line'` demands, for names that are also everyday words.
 *
 * Keyword position means the name heads an ability line, or follows a comma in the
 * leading keyword run of one ("Flying, first strike, vigilance"), **and** what comes
 * after it ends the run: the line's end, a comma, a period, an em dash (how an ability
 * word is always printed), a `{` cost, or a digit (`Annihilator 3`).
 *
 * Both halves are needed. Requiring only the line start would match "Fear of Isolation
 * gets +1/+1" at the top of a line; requiring only the terminator would match the
 * "storm" in a sentence that happens to end there.
 */
function inKeywordPosition(text: string, start: number, end: number): boolean {
  const before = text.slice(0, start)
  const lineStart = before.lastIndexOf('\n') + 1
  const lead = before.slice(lineStart)
  // The head of the line, or a later item in its comma-separated keyword run. Anything
  // else on the line first (a verb, "Whenever…") means this isn't a keyword run.
  if (!/^(?:[A-Za-z' ’-]*,\s*)*$/.test(lead)) return false

  const after = text.slice(end)
  return /^(?:$|[\n,.;:]|\s*[—-]|\s*\{|\s+\d)/.test(after)
}

/**
 * Find every keyword worth marking in one run of rules text.
 *
 * Returns non-overlapping matches in document order, at most one per keyword. `entries`
 * is the glossary as the API sent it; `cardName` (when known) blocks matches inside the
 * card's own name.
 */
export function findKeywords(
  text: string,
  entries: readonly KeywordEntry[],
  cardName?: string,
): KeywordMatch[] {
  if (!text || entries.length === 0) return []

  const spans = blockedSpans(text, cardName)
  // Longest name first, so `Flashback` wins over `Flash` at the same position and the
  // shorter name's match is discarded as an overlap below.
  const candidates = [...entries]
    .filter((entry) => entry.match_mode !== 'never')
    .sort((a, b) => b.name.length - a.name.length)

  const found: KeywordMatch[] = []
  const taken: Array<[number, number]> = []
  for (const entry of candidates) {
    // `\b` is wrong at a name that ends in punctuation ("For Mirrodin!"), so the
    // trailing boundary is only asserted when the name ends in a word character.
    const trailing = /\w$/.test(entry.name) ? '\\b' : ''
    const pattern = new RegExp(`\\b${escapeRegExp(entry.name)}${trailing}`, 'gi')
    for (const match of text.matchAll(pattern)) {
      const start = match.index
      const end = start + match[0].length
      if (isBlocked(spans, start, end)) continue
      if (isBlocked(taken, start, end)) continue
      if (entry.match_mode === 'ability_line' && !inKeywordPosition(text, start, end)) continue
      found.push({ start, end, entry })
      taken.push([start, end])
      // First mention only: one marker per keyword per run of text.
      break
    }
  }

  return found.sort((a, b) => a.start - b.start)
}

/** A run of text between keyword matches. */
export interface PlainSegment {
  type: 'text'
  value: string
}

/** A matched keyword, carrying the text as the card actually spells it. */
export interface KeywordSegment {
  type: 'keyword'
  /** The matched text verbatim, so the card's own casing survives. */
  value: string
  entry: KeywordEntry
}

export type KeywordSegmentToken = PlainSegment | KeywordSegment

/** Split one run of rules text into plain runs and keyword runs, ready to render. */
export function splitKeywords(
  text: string,
  entries: readonly KeywordEntry[],
  cardName?: string,
): KeywordSegmentToken[] {
  const matches = findKeywords(text, entries, cardName)
  if (matches.length === 0) return [{ type: 'text', value: text }]

  const tokens: KeywordSegmentToken[] = []
  let last = 0
  for (const match of matches) {
    if (match.start > last) tokens.push({ type: 'text', value: text.slice(last, match.start) })
    tokens.push({
      type: 'keyword',
      value: text.slice(match.start, match.end),
      entry: match.entry,
    })
    last = match.end
  }
  if (last < text.length) tokens.push({ type: 'text', value: text.slice(last) })
  return tokens
}

// ---------- Glossary-page helpers ----------

/** How many related keywords a keyword page links to. */
const RELATED_LIMIT = 8

/** Keywords worth linking from `entry`'s page: ones its explanation names, ones that
 * name it, then its same-kind *neighbours in the list* to fill the row. Deduplicated,
 * never `entry` itself, and deterministic — the page must not reshuffle between renders.
 *
 * The fill deliberately walks outward from `entry`'s own position rather than from the
 * top of the glossary: taking the first same-kind entries instead would put the same
 * handful of A-names ("Absorb, Affinity, Afflict…") at the foot of all 365 pages. */
export function relatedKeywords(entry: KeywordEntry, all: readonly KeywordEntry[]): KeywordEntry[] {
  const picked = new Map<string, KeywordEntry>()
  const others = all.filter((other) => other.slug !== entry.slug)

  const mentions = (haystack: string, needle: string) =>
    new RegExp(`\\b${escapeRegExp(needle)}\\b`, 'i').test(haystack)

  // Keywords this one's explanation refers to — the strongest signal (Flying names Reach).
  for (const other of others) {
    if (picked.size >= RELATED_LIMIT) break
    if (mentions(entry.text, other.name)) picked.set(other.slug, other)
  }
  // …then the reverse: keywords whose explanations refer to this one.
  for (const other of others) {
    if (picked.size >= RELATED_LIMIT) break
    if (mentions(other.text, entry.name)) picked.set(other.slug, other)
  }
  // …then same-kind entries near this one, alternating outward from its position.
  const at = all.findIndex((item) => item.slug === entry.slug)
  if (at !== -1) {
    for (let step = 1; step < all.length && picked.size < RELATED_LIMIT; step += 1) {
      for (const neighbour of [all[at - step], all[at + step]]) {
        if (picked.size >= RELATED_LIMIT) break
        if (neighbour && neighbour.kind === entry.kind && neighbour.slug !== entry.slug) {
          picked.set(neighbour.slug, neighbour)
        }
      }
    }
  }
  return [...picked.values()].slice(0, RELATED_LIMIT)
}
