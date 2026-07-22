// MTG format + legality domain logic (issue #557). Pure data/functions — no Vue.
//
// `Card.legalities` is the Scryfall per-format object stored verbatim by the catalog
// (`{ "modern": "banned", ... }`), and `deck.format` is a free-form label the user
// picked or typed. This module owns the bridge between the two: the curated format
// table (select options + display order), the free-text → format-key normalizer, and
// the deck-wide legality evaluation the deck views render as a breach banner.

import type { Card, DeckCardEntry } from '@/lib/api'

/** A legality value as Scryfall writes it. Anything else is treated as unknown. */
export type LegalityStatus = 'legal' | 'not_legal' | 'banned' | 'restricted'

export interface MtgFormat {
  /** The key used in `Card.legalities` (Scryfall's format slug). */
  key: string
  /** Display label; also the string stored in `deck.format` when picked. */
  label: string
  /** Select-menu grouping. */
  group: 'Constructed' | 'Commander' | 'Arena' | 'Other'
  /** Extra spellings `normalizeFormatKey` accepts (canonicalized before compare). */
  aliases?: string[]
}

/**
 * Every format we track legality for, in display order (card-page panel and the
 * deck-format select both render this order, grouped). Keys match the stored
 * Scryfall legalities object; `future` and `tlr` exist in the data but are
 * deliberately not surfaced (meaningless to deck builders).
 */
export const MTG_FORMATS: MtgFormat[] = [
  { key: 'standard', label: 'Standard', group: 'Constructed' },
  { key: 'pioneer', label: 'Pioneer', group: 'Constructed' },
  { key: 'modern', label: 'Modern', group: 'Constructed' },
  { key: 'legacy', label: 'Legacy', group: 'Constructed' },
  { key: 'vintage', label: 'Vintage', group: 'Constructed' },
  { key: 'pauper', label: 'Pauper', group: 'Constructed' },
  { key: 'commander', label: 'Commander', group: 'Commander', aliases: ['edh', 'cedh'] },
  { key: 'oathbreaker', label: 'Oathbreaker', group: 'Commander' },
  {
    key: 'paupercommander',
    label: 'Pauper Commander',
    group: 'Commander',
    aliases: ['pdh', 'pauperedh'],
  },
  {
    key: 'duel',
    label: 'Duel Commander',
    group: 'Commander',
    aliases: ['duelcommander', 'frenchcommander'],
  },
  { key: 'predh', label: 'PreDH', group: 'Commander', aliases: ['preedh'] },
  { key: 'alchemy', label: 'Alchemy', group: 'Arena' },
  { key: 'historic', label: 'Historic', group: 'Arena' },
  { key: 'timeless', label: 'Timeless', group: 'Arena' },
  { key: 'gladiator', label: 'Gladiator', group: 'Arena' },
  { key: 'brawl', label: 'Brawl', group: 'Arena', aliases: ['historicbrawl'] },
  { key: 'standardbrawl', label: 'Standard Brawl', group: 'Arena' },
  {
    key: 'competitivebrawl',
    label: 'Competitive Brawl',
    group: 'Arena',
    aliases: ['compbrawl'],
  },
  { key: 'penny', label: 'Penny Dreadful', group: 'Other' },
  { key: 'oldschool', label: 'Old School', group: 'Other', aliases: ['oldschool9394', '9394'] },
  { key: 'premodern', label: 'Premodern', group: 'Other' },
]

/** Lowercase and strip everything but letters/digits, so "Comp. Brawl" == "compbrawl". */
function canon(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]/g, '')
}

const FORMAT_BY_CANON: ReadonlyMap<string, MtgFormat> = (() => {
  const map = new Map<string, MtgFormat>()
  for (const format of MTG_FORMATS) {
    for (const spelling of [format.key, format.label, ...(format.aliases ?? [])]) {
      map.set(canon(spelling), format)
    }
  }
  return map
})()

const FORMAT_BY_KEY: ReadonlyMap<string, MtgFormat> = new Map(
  MTG_FORMATS.map((format) => [format.key, format]),
)

/**
 * Map a free-form deck format label to a legality key, or `null` when it isn't a
 * legality-tracked format (custom text, "Cube", "Casual", …) — `null` means "don't
 * evaluate legality", never "illegal".
 */
export function normalizeFormatKey(text: string | null | undefined): string | null {
  if (!text) return null
  return FORMAT_BY_CANON.get(canon(text))?.key ?? null
}

/** Display label for a legality key (falls back to the key itself). */
export function formatLabel(key: string): string {
  return FORMAT_BY_KEY.get(key)?.label ?? key
}

/** Human label for a legality status ("not_legal" -> "Not Legal"). */
export function legalityLabel(status: LegalityStatus): string {
  switch (status) {
    case 'legal':
      return 'Legal'
    case 'not_legal':
      return 'Not Legal'
    case 'banned':
      return 'Banned'
    case 'restricted':
      return 'Restricted'
  }
}

/** A breach-worthy status (what the deck banner reports). */
export type DeckIssueStatus = 'banned' | 'not_legal' | 'restricted'

/** One offending card name in a deck (all printings of a name fold into one issue). */
export interface DeckLegalityIssue {
  /** External card id of one printing (for keys/links). */
  cardId: string
  name: string
  status: DeckIssueStatus
  /** Total copies across every section and printing (regular + foil). */
  quantity: number
}

export interface DeckLegality {
  formatKey: string
  formatLabel: string
  /** Sorted banned -> not legal -> restricted, alphabetical within each group. */
  issues: DeckLegalityIssue[]
  /** Per-printing status for every entry belonging to an offending name (tile chips). */
  statusByCardId: ReadonlyMap<string, DeckIssueStatus>
  /** Cards whose catalog row carries no legality data at all (not counted as issues). */
  unknownCount: number
}

const ISSUE_ORDER: Record<DeckIssueStatus, number> = { banned: 0, not_legal: 1, restricted: 2 }

/**
 * Evaluate a deck's cards against its format. Returns `null` when the format doesn't
 * map to a legality-tracked one (nothing to evaluate). Semantics:
 *
 * - `banned` / `not_legal` in the format -> an issue, always.
 * - `restricted` -> an issue only when more than one total copy of that name is in
 *   the deck (Vintage's "max 1 copy" rule).
 * - A card with no legality data, or a legalities object missing this format's key,
 *   is counted in `unknownCount` and never flagged — a false "in breach" is worse
 *   than a miss.
 *
 * Copy counts fold across sections AND printings by card name, so 2x of one printing
 * of a restricted card plus 1x of another printing is still a breach.
 */
export function evaluateDeckLegality(
  format: string | null | undefined,
  entries: DeckCardEntry[],
): DeckLegality | null {
  const key = normalizeFormatKey(format)
  if (!key) return null

  // Pass 1: fold total copies per card name (restricted needs cross-printing totals).
  const copiesByName = new Map<string, number>()
  for (const entry of entries) {
    const copies = entry.quantity + entry.foil_quantity
    copiesByName.set(entry.card.name, (copiesByName.get(entry.card.name) ?? 0) + copies)
  }

  // Pass 2: judge each name once; remember every printing's id for the tile chips.
  const issueByName = new Map<string, DeckLegalityIssue>()
  const statusByCardId = new Map<string, DeckIssueStatus>()
  let unknownCount = 0
  for (const entry of entries) {
    const status = statusOf(entry.card, key)
    if (status == null) {
      unknownCount += 1
      continue
    }
    const totalCopies = copiesByName.get(entry.card.name) ?? 0
    const issue: DeckIssueStatus | null =
      status === 'banned' || status === 'not_legal'
        ? status
        : status === 'restricted' && totalCopies > 1
          ? 'restricted'
          : null
    if (issue == null) continue
    statusByCardId.set(entry.card.id, issue)
    if (!issueByName.has(entry.card.name)) {
      issueByName.set(entry.card.name, {
        cardId: entry.card.id,
        name: entry.card.name,
        status: issue,
        quantity: totalCopies,
      })
    }
  }

  const issues = [...issueByName.values()].sort(
    (a, b) => ISSUE_ORDER[a.status] - ISSUE_ORDER[b.status] || a.name.localeCompare(b.name),
  )
  return { formatKey: key, formatLabel: formatLabel(key), issues, statusByCardId, unknownCount }
}

/** A card's status in one format, or `null` when unknown (no data / unexpected value). */
export function statusOf(card: Card, formatKey: string): LegalityStatus | null {
  const raw = card.legalities?.[formatKey]
  return raw === 'legal' || raw === 'not_legal' || raw === 'banned' || raw === 'restricted'
    ? raw
    : null
}
