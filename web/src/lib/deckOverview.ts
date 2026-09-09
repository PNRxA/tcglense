import type {
  DeckBracketEstimate,
  DeckComposition,
  DeckLegality,
  DeckManaBase,
  DeckPricing,
  DeckSuggestions,
} from '@/lib/api'
import { colourFilterLabel, formatFilterLabel } from '@/lib/deckSuggestions'

// The deck page's collapsed **overview strip** (`components/decks/DeckOverview.vue`): one
// chip per analysis panel, worded so a reader who never expands the page still gets the
// answer each panel exists for. Every chip is a pure function of the response the panel
// itself renders, so the strip and the panel can't make different claims — the same stance
// `DeckStats`' resting rows and `DeckBracket`'s chips take, one level up.
//
// Each helper returns `null` when the panel would have nothing to say (a colourless deck has
// no mana base; a deck already at its cheapest printings has no saving), so the strip only
// ever carries chips that answer something. The wording is deliberately terse — a chip, not
// a sentence — and the fuller line each panel prints rides the chip's `title`.

export type GlanceTone = 'neutral' | 'success' | 'warning' | 'destructive'

export interface Glance {
  /** Stable per-panel key, so a chip row can key on it. */
  key: string
  label: string
  tone: GlanceTone
  /** Mana pips drawn after the label (`['G', 'U']`), for the mana-base chip. */
  pips?: string[]
  /** The longer reading, for the chip's tooltip. */
  title?: string
}

/**
 * The legality verdict, in `DeckLegalityBanner`'s three states and by its rule: illegal as
 * it stands (`!legal`) is the destructive chip, a deck that is only unfinished (warnings
 * alone) is the warning chip, and a clean deck the success one. A half-built deck is not
 * shouted at in red here either.
 */
export function legalityGlance(legality: DeckLegality): Glance {
  const issues = legality.issues.length
  const issueText = issues === 1 ? '1 card issue' : `${issues} card issues`
  if (!legality.legal) {
    // The breach that made it illegal, not the first warning in the list: the server sorts
    // neither, and the banner leads with errors for the same reason.
    const breach = legality.violations.find((v) => v.severity === 'error')?.message
    return {
      key: 'legality',
      label: `Not legal in ${legality.format_label}`,
      tone: 'destructive',
      title: issues > 0 ? issueText : breach,
    }
  }
  if (legality.violations.length > 0) {
    return {
      key: 'legality',
      label: `${legality.format_label} deck in progress`,
      tone: 'warning',
      title: legality.violations.map((v) => v.message).join(' '),
    }
  }
  return { key: 'legality', label: `Legal in ${legality.format_label}`, tone: 'success' }
}

/** The estimated Commander bracket: the rung and its name. */
export function bracketGlance(estimate: DeckBracketEstimate): Glance {
  return {
    key: 'bracket',
    label: `Bracket ${estimate.bracket} · ${estimate.label}`,
    tone: 'neutral',
    title: `Estimated bracket · ${estimate.bracket} of ${estimate.ladder.length}`,
  }
}

/**
 * The land count with its share — the denominator printed beside it, as `DeckStats` does,
 * because `total_copies` is the deck *proper* (sideboard and command zone included) and a
 * bare "38%" would be read against the 60 or 99 a player has in mind.
 */
export function landsGlance(deck: DeckComposition): Glance | null {
  if (deck.total_copies === 0) return null
  const share = Math.round((deck.land_copies / deck.total_copies) * 100)
  return {
    key: 'lands',
    label: `${deck.land_copies} lands · ${share}% of ${deck.total_copies}`,
    tone: 'neutral',
    title: `${deck.land_copies} of the deck's ${deck.total_copies} cards are lands`,
  }
}

/** The average mana value, to the two decimals `DeckStats` prints it with. */
export function manaValueGlance(deck: DeckComposition): Glance | null {
  if (deck.average_mana_value == null) return null
  return {
    key: 'mana-value',
    label: `Avg mana value ${deck.average_mana_value.toFixed(2)}`,
    tone: 'neutral',
  }
}

/**
 * The mana base folded to one verdict: the colours the deck is short on (a warning, with
 * their pips), else the colours whose answer depends on X, else "enough" when at least one
 * colour was judged — a deck whose every colour has no demand has nothing to say.
 */
export function manaGlance(base: DeckManaBase): Glance | null {
  if (base.colors.length === 0) return null
  const short = base.colors.filter((c) => c.status === 'short')
  if (short.length > 0) {
    return {
      key: 'mana',
      label: 'Mana base short on',
      pips: short.map((c) => c.color),
      tone: 'warning',
      title: short.map((c) => c.verdict).join(' '),
    }
  }
  const undecided = base.colors.filter((c) => c.status === 'undecided')
  if (undecided.length > 0) {
    return {
      key: 'mana',
      label: 'Mana base depends on X for',
      pips: undecided.map((c) => c.color),
      tone: 'neutral',
      title: undecided.map((c) => c.verdict).join(' '),
    }
  }
  if (!base.colors.some((c) => c.status === 'enough')) return null
  return {
    key: 'mana',
    label: 'Mana base: enough',
    tone: 'success',
    title: `${base.land_count} lands in a ${base.library_size}-card library`,
  }
}

/**
 * What swapping to the cheapest printings would save. Only stated when there is a saving:
 * the deck's value is already in the page header, and "already the cheapest" is a line the
 * pricing panel prints, not a chip worth the space.
 */
export function savingGlance(
  pricing: DeckPricing,
  formatUsd: (raw: string | null | undefined) => string | null,
): Glance | null {
  if (pricing.saving_usd == null || !(Number(pricing.saving_usd) > 0)) return null
  const amount = formatUsd(pricing.saving_usd)
  if (!amount) return null
  const cards = pricing.swappable_count
  return {
    key: 'saving',
    label: `Save ${amount} at cheapest printings`,
    tone: 'neutral',
    title: `${cards} card${cards === 1 ? ' has' : 's have'} a cheaper printing`,
  }
}

/**
 * How many cards the owner already holds that the deck could play — the count alone. The
 * tooltip states the two filters through the panel's own wording (`lib/deckSuggestions`),
 * because a filter the server didn't apply — no format, no colour to read — must not be
 * implied by the chip either.
 */
export function suggestionsGlance(suggestions: DeckSuggestions): Glance | null {
  const n = suggestions.candidate_count
  if (n === 0) return null
  return {
    key: 'suggestions',
    label: n === 1 ? '1 card you own fits' : `${n.toLocaleString()} cards you own fit`,
    tone: 'neutral',
    title: `From your collection — ${colourFilterLabel(suggestions)} · ${formatFilterLabel(suggestions)} · not in the deck yet`,
  }
}
