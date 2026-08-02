// MTG format vocabulary — the *presentation* half of legality (issues #557, #596).
//
// The verdict itself is the server's: `GET /api/decks/{game}/{deck_id}/legality` returns
// the offending cards and the deck-wide construction breaches, computed from the same
// catalog rows the card page reads (see `api/src/handlers/decks/analysis/`). What lives
// here is what a *renderer* needs and a JSON payload shouldn't carry: how a format is
// spelled in a select, which six fill the card page's legality panel, what colour a breach
// chip is, and what each status is called in English.
//
// **Mirrored table.** `MTG_FORMATS` is the same list as `analysis::formats::MTG_FORMATS`,
// duplicated so a dropdown and the card-page panel draw without waiting on a request. Both
// sides carry a test pinning it (`__tests__/legality.spec.ts` here, `format_vocabulary_is_pinned`
// there), so a format added to one alone fails that side rather than silently disagreeing —
// the arrangement `lib/lifeLayout.ts` already uses. `GET /api/games/{game}/formats`
// publishes the server's copy for clients that would otherwise hard-code it.

import type { Card, DeckIssueStatus } from '@/lib/api'

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
  /**
   * One of the six most-played formats — what the card-page legality panel shows
   * before its "Show all formats" expansion (six fills the panel's 3-column grid
   * with exactly two rows).
   */
  popular?: boolean
}

/**
 * Every format we track legality for, in display order (card-page panel and the
 * deck-format select both render this order, grouped). Keys match the stored
 * Scryfall legalities object; `future` and `tlr` exist in the data but are
 * deliberately not surfaced (meaningless to deck builders).
 */
export const MTG_FORMATS: MtgFormat[] = [
  { key: 'standard', label: 'Standard', group: 'Constructed', popular: true },
  { key: 'pioneer', label: 'Pioneer', group: 'Constructed', popular: true },
  { key: 'modern', label: 'Modern', group: 'Constructed', popular: true },
  { key: 'legacy', label: 'Legacy', group: 'Constructed', popular: true },
  { key: 'vintage', label: 'Vintage', group: 'Constructed' },
  { key: 'pauper', label: 'Pauper', group: 'Constructed', popular: true },
  {
    key: 'commander',
    label: 'Commander',
    group: 'Commander',
    aliases: ['edh', 'cedh'],
    popular: true,
  },
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
 * legality-tracked format (custom text, "Cube", "Casual", …). Used by the format field to
 * tell the user whether what they typed will be checked; the server runs the same
 * normalisation before evaluating, so the two agree on what "tracked" means.
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

/** Human label for a deck breach ("not_legal" -> "Not Legal"). */
export function deckIssueLabel(status: DeckIssueStatus): string {
  switch (status) {
    case 'banned':
      return 'Banned'
    case 'not_legal':
      return 'Not Legal'
    case 'commander_only':
      return 'Commander Only'
    case 'off_colour':
      return 'Off Colour'
    case 'over_limit':
      return 'Over Limit'
    case 'restricted':
      return 'Restricted'
  }
}

/**
 * Text colour per breach, for the chip both deck views overlay on a tile and the one
 * DeckCardRow puts in its list column (the same three renderers, so it lives here rather
 * than being copy-pasted a third time): red for outright illegal, muted for merely
 * out-of-format, amber for "you're running it wrong".
 */
export const DECK_ISSUE_TEXT_CLASS: Record<DeckIssueStatus, string> = {
  banned: 'text-red-600 dark:text-red-400',
  not_legal: 'text-muted-foreground',
  commander_only: 'text-amber-600 dark:text-amber-400',
  off_colour: 'text-amber-600 dark:text-amber-400',
  over_limit: 'text-amber-600 dark:text-amber-400',
  restricted: 'text-amber-600 dark:text-amber-400',
}

/**
 * Every breach status, most severe first — the order the banner groups its summary in.
 * The server sorts `issues` and picks a card's worst status by the same order (its
 * `DeckIssueStatus` enum is declared in it), so this list must stay in step with
 * `api/src/handlers/decks/analysis/legality.rs`; the spec beside this file pins it.
 */
export const DECK_ISSUE_STATUSES: readonly DeckIssueStatus[] = [
  'banned',
  'not_legal',
  'commander_only',
  'off_colour',
  'over_limit',
  'restricted',
]

/** A card's status in one format, or `null` when unknown (no data / unexpected value).
 * The card page's per-format panel reads a single card's own data, which needs no deck and
 * no request — the deck-wide verdict is the server's (`useDeckLegalityQuery`). */
export function statusOf(card: Card, formatKey: string): LegalityStatus | null {
  const raw = card.legalities?.[formatKey]
  return raw === 'legal' || raw === 'not_legal' || raw === 'banned' || raw === 'restricted'
    ? raw
    : null
}
