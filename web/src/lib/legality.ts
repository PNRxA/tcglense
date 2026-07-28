// MTG format + legality domain logic (issue #557). Pure data/functions — no Vue.
//
// `Card.legalities` is the Scryfall per-format object stored verbatim by the catalog
// (`{ "modern": "banned", ... }`), and `deck.format` is a free-form label the user
// picked or typed. This module owns the bridge between the two: the curated format
// table (select options + display order), the free-text → format-key normalizer, and
// the deck-wide legality evaluation the deck views render as a breach banner.
//
// Two halves make that verdict: the per-card one here (is this card banned/restricted in
// the format?) and the deck-construction one in `lib/deckRules.ts` (deck size, the copy
// limit, the command zone, colour identity). `evaluateDeckLegality` composes them into one
// result so the views have a single thing to render.

import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import {
  commandZoneSectionIds,
  evaluateDeckRules,
  type DeckRuleCardStatus,
  type DeckRuleViolation,
} from '@/lib/deckRules'

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

/**
 * A breach-worthy status for one card (what the deck banner and the tile chips report).
 * The first four come from the card's own legality data; `off_colour` and `over_limit`
 * come from the deck-construction rules.
 */
export type DeckIssueStatus =
  | 'banned'
  | 'not_legal'
  | 'commander_only'
  | 'restricted'
  | DeckRuleCardStatus

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
  /** Sorted most severe first, alphabetical within each status. */
  issues: DeckLegalityIssue[]
  /** Deck-wide construction breaches (size, command zone, colour identity). */
  violations: DeckRuleViolation[]
  /** Per-printing status for every entry belonging to an offending name (tile chips). */
  statusByCardId: ReadonlyMap<string, DeckIssueStatus>
  /** Cards whose catalog row carries no legality data at all (not counted as issues). */
  unknownCount: number
  /** No card issues and no error-severity violation — a deck you could sit down with. */
  legal: boolean
}

/**
 * Every breach status, most severe first. Sorts the issue list, picks a card's worst
 * status when two rules catch it, and gives the banner its summary order.
 */
export const DECK_ISSUE_STATUSES: readonly DeckIssueStatus[] = [
  'banned',
  'not_legal',
  'commander_only',
  'off_colour',
  'over_limit',
  'restricted',
]

const ISSUE_ORDER = Object.fromEntries(
  DECK_ISSUE_STATUSES.map((status, index) => [status, index]),
) as Record<DeckIssueStatus, number>

/**
 * Evaluate a deck against its format. Returns `null` when the format doesn't map to a
 * legality-tracked one (nothing to evaluate). Per-card semantics:
 *
 * - `banned` / `not_legal` in the format -> an issue, always.
 * - `restricted` -> an issue only when more than one total copy of that name is in
 *   the deck (Vintage's "max 1 copy" rule). Pauper Commander is the exception: Scryfall
 *   writes `restricted` there to mean "legal only as the commander" (an uncommon
 *   creature), so it's an issue when the card sits anywhere *but* the command zone.
 * - A card with no legality data, or a legalities object missing this format's key,
 *   is counted in `unknownCount` and never flagged — a false "in breach" is worse
 *   than a miss.
 *
 * Copy counts fold across sections AND printings by card name, so 2x of one printing
 * of a restricted card plus 1x of another printing is still a breach.
 *
 * Deck-construction rules (size, the copy limit, the command zone, colour identity) come
 * from `evaluateDeckRules` and need the deck's `sections` to tell the zones apart; pass
 * them, or those checks simply don't run.
 */
export function evaluateDeckLegality(
  format: string | null | undefined,
  entries: DeckCardEntry[],
  sections: DeckSection[] = [],
): DeckLegality | null {
  const key = normalizeFormatKey(format)
  if (!key) return null

  // Pass 1: fold total copies per card name (restricted needs cross-printing totals).
  const copiesByName = new Map<string, number>()
  for (const entry of entries) {
    const copies = entry.quantity + entry.foil_quantity
    copiesByName.set(entry.card.name, (copiesByName.get(entry.card.name) ?? 0) + copies)
  }

  // Pass 2: judge each printing against the card's own legality data.
  const commandZone = commandZoneSectionIds(sections)
  const found: DeckLegalityIssue[] = []
  let unknownCount = 0
  for (const entry of entries) {
    const status = statusOf(entry.card, key)
    if (status == null) {
      unknownCount += 1
      continue
    }
    const quantity = copiesByName.get(entry.card.name) ?? 0
    const issue: DeckIssueStatus | null =
      status === 'banned' || status === 'not_legal'
        ? status
        : status !== 'restricted'
          ? null
          : key === 'paupercommander'
            ? commandZone.has(entry.section_id)
              ? null
              : 'commander_only'
            : quantity > 1
              ? 'restricted'
              : null
    if (issue != null)
      found.push({ cardId: entry.card.id, name: entry.card.name, status: issue, quantity })
  }

  // Pass 3: the deck-wide rules, whose per-card breaches join the same list.
  const rules = evaluateDeckRules(key, entries, sections)
  found.push(...rules.cardIssues)

  // Fold to one issue per name and one chip per printing, keeping the worst status of each.
  const issueByName = new Map<string, DeckLegalityIssue>()
  const statusByCardId = new Map<string, DeckIssueStatus>()
  for (const issue of found) {
    const worseThan = (previous: DeckIssueStatus | undefined) =>
      previous == null || ISSUE_ORDER[issue.status] < ISSUE_ORDER[previous]
    if (worseThan(statusByCardId.get(issue.cardId))) statusByCardId.set(issue.cardId, issue.status)
    const existing = issueByName.get(issue.name)
    if (worseThan(existing?.status)) issueByName.set(issue.name, issue)
  }

  const issues = [...issueByName.values()].sort(
    (a, b) => ISSUE_ORDER[a.status] - ISSUE_ORDER[b.status] || a.name.localeCompare(b.name),
  )
  return {
    formatKey: key,
    formatLabel: formatLabel(key),
    issues,
    violations: rules.violations,
    statusByCardId,
    unknownCount,
    legal: issues.length === 0 && !rules.violations.some((v) => v.severity === 'error'),
  }
}

/** A card's status in one format, or `null` when unknown (no data / unexpected value). */
export function statusOf(card: Card, formatKey: string): LegalityStatus | null {
  const raw = card.legalities?.[formatKey]
  return raw === 'legal' || raw === 'not_legal' || raw === 'banned' || raw === 'restricted'
    ? raw
    : null
}
