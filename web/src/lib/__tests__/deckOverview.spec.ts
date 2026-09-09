import { describe, expect, it } from 'vitest'
import type {
  DeckBracketEstimate,
  DeckComposition,
  DeckLegality,
  DeckManaBase,
  DeckManaColor,
  DeckPricing,
  DeckSuggestions,
} from '@/lib/api'
import {
  bracketGlance,
  landsGlance,
  legalityGlance,
  manaGlance,
  manaValueGlance,
  savingGlance,
  suggestionsGlance,
} from '@/lib/deckOverview'

// The overview strip's chips are pure functions of the panels' responses; these pin that
// each one words the same verdict the panel does, and stays silent where the panel would
// have nothing to say.

function legality(over: Partial<DeckLegality> = {}): DeckLegality {
  return {
    format_key: 'commander',
    format_label: 'Commander',
    issues: [],
    violations: [],
    card_statuses: {},
    unknown_count: 0,
    legal: true,
    ...over,
  }
}

function composition(over: Partial<DeckComposition> = {}): DeckComposition {
  return {
    total_copies: 100,
    unique_cards: 90,
    land_copies: 38,
    average_mana_value: 3.2,
    mana_curve: [],
    colors: [],
    card_types: [],
    card_odds: [],
    ...over,
  }
}

function colour(over: Partial<DeckManaColor>): DeckManaColor {
  return {
    color: 'G',
    label: 'Green',
    pips: 4,
    hybrid_pips: 0,
    demand_count: 10,
    demand: [],
    sources: 20,
    land_sources: 18,
    nonland_sources: 2,
    source_count: 20,
    source_cards: [],
    sources_needed: 19,
    shortfall: 0,
    status: 'enough',
    verdict: 'Enough green.',
    ...over,
  }
}

function manaBase(colors: DeckManaColor[]): DeckManaBase {
  return {
    deck_size: 99,
    table_size: 99,
    library_size: 99,
    land_count: 37,
    colors,
    unchecked_count: 0,
    caveats: [],
    source: 'Karsten 2022',
  }
}

function pricing(over: Partial<DeckPricing> = {}): DeckPricing {
  return {
    lines: [],
    total_usd: '412.00',
    cheapest_total_usd: '210.00',
    saving_usd: '202.00',
    unpriced_count: 0,
    swappable_count: 12,
    ...over,
  }
}

const usd = (raw: string | null | undefined) => (raw == null ? null : `$${raw}`)

describe('legalityGlance', () => {
  it('follows the banner: illegal is destructive, unfinished is a warning, clean is success', () => {
    expect(
      legalityGlance(
        legality({
          legal: false,
          issues: [{ card_id: 'a', name: 'Sol Ring', status: 'banned', quantity: 1 }],
        }),
      ),
    ).toMatchObject({ label: 'Not legal in Commander', tone: 'destructive', title: '1 card issue' })
    expect(
      legalityGlance(
        legality({
          violations: [{ rule: 'deck-size', severity: 'warning', message: '40 of 100 cards.' }],
        }),
      ),
    ).toMatchObject({
      label: 'Commander deck in progress',
      tone: 'warning',
      title: '40 of 100 cards.',
    })
    expect(legalityGlance(legality())).toMatchObject({
      label: 'Legal in Commander',
      tone: 'success',
    })
  })

  it('names the breach that made it illegal, not a warning listed ahead of it', () => {
    const glance = legalityGlance(
      legality({
        legal: false,
        violations: [
          { rule: 'deck-size', severity: 'warning', message: '98 of 100 cards.' },
          { rule: 'colour-identity', severity: 'error', message: 'Off colour.' },
        ],
      }),
    )
    expect(glance.title).toBe('Off colour.')
  })
})

describe('bracketGlance', () => {
  it('states the rung, its name, and its place on the ladder', () => {
    const estimate: DeckBracketEstimate = {
      format_key: 'commander',
      format_label: 'Commander',
      bracket: 3,
      label: 'Upgraded',
      description: '',
      ladder: [1, 2, 3, 4, 5].map((bracket) => ({ bracket, label: '', description: '' })),
      reasons: [],
      caveats: [],
      categories: [],
      exhibition_possible: false,
    }
    expect(bracketGlance(estimate)).toMatchObject({
      label: 'Bracket 3 · Upgraded',
      title: 'Estimated bracket · 3 of 5',
    })
  })
})

describe('landsGlance / manaValueGlance', () => {
  it('prints the share against the deck proper, as the analytics panel does', () => {
    expect(landsGlance(composition())).toMatchObject({ label: '38 lands · 38% of 100' })
    expect(landsGlance(composition({ total_copies: 75, land_copies: 30 }))).toMatchObject({
      label: '30 lands · 40% of 75',
    })
    expect(landsGlance(composition({ total_copies: 0, land_copies: 0 }))).toBeNull()
  })

  it('rounds the average to two decimals and skips a deck with none', () => {
    expect(manaValueGlance(composition())).toMatchObject({ label: 'Avg mana value 3.20' })
    expect(manaValueGlance(composition({ average_mana_value: null }))).toBeNull()
  })
})

describe('manaGlance', () => {
  it('leads with the colours the deck is short on, pips included', () => {
    const glance = manaGlance(
      manaBase([
        colour({}),
        colour({ color: 'U', status: 'short', shortfall: 3, verdict: 'Three blue short.' }),
        colour({ color: 'R', status: 'short', shortfall: 1, verdict: 'One red short.' }),
      ]),
    )
    expect(glance).toMatchObject({
      label: 'Mana base short on',
      pips: ['U', 'R'],
      tone: 'warning',
      title: 'Three blue short. One red short.',
    })
  })

  it('reports an X-dependent colour before calling the base enough', () => {
    expect(
      manaGlance(manaBase([colour({}), colour({ color: 'B', status: 'undecided' })])),
    ).toMatchObject({
      label: 'Mana base depends on X for',
      pips: ['B'],
      tone: 'neutral',
    })
    expect(manaGlance(manaBase([colour({}), colour({ color: 'W' })]))).toMatchObject({
      label: 'Mana base: enough',
      tone: 'success',
      title: '37 lands in a 99-card library',
    })
  })

  it('says nothing for a deck with no colours, or none that demand anything', () => {
    expect(manaGlance(manaBase([]))).toBeNull()
    expect(manaGlance(manaBase([colour({ status: 'no_demand' })]))).toBeNull()
  })
})

describe('savingGlance', () => {
  it('states the saving and how many cards carry one', () => {
    expect(savingGlance(pricing(), usd)).toMatchObject({
      label: 'Save $202.00 at cheapest printings',
      title: '12 cards have a cheaper printing',
    })
    expect(savingGlance(pricing({ swappable_count: 1 }), usd)?.title).toBe(
      '1 card has a cheaper printing',
    )
  })

  it('is silent at the cheapest printings already, when unpriced, or when the currency is', () => {
    expect(savingGlance(pricing({ saving_usd: '0.00', swappable_count: 0 }), usd)).toBeNull()
    expect(
      savingGlance(pricing({ total_usd: null, cheapest_total_usd: null, saving_usd: null }), usd),
    ).toBeNull()
    expect(savingGlance(pricing(), () => null)).toBeNull()
  })
})

describe('suggestionsGlance', () => {
  function suggestions(candidate_count: number): DeckSuggestions {
    return {
      format_key: 'commander',
      format_label: 'Commander',
      color_identity: ['G'],
      commanders: [],
      candidate_count,
      scanned_count: candidate_count,
      cards: [],
      top: [],
      roles: [],
      unclassified_count: 0,
      caveats: [],
    }
  }

  it('counts the cards that fit, singular and plural, and skips zero', () => {
    expect(suggestionsGlance(suggestions(1))?.label).toBe('1 card you own fits')
    expect(suggestionsGlance(suggestions(1200))?.label).toBe('1,200 cards you own fit')
    expect(suggestionsGlance(suggestions(0))).toBeNull()
  })

  it('words the filters as the panel does, and never implies one the server skipped', () => {
    expect(suggestionsGlance(suggestions(3))?.title).toBe(
      "From your collection — in the deck's colours (G) · legal in Commander · not in the deck yet",
    )
    const untracked = { ...suggestions(3), format_key: null, format_label: null }
    expect(suggestionsGlance(untracked)?.title).toContain(
      'any format (the deck’s format isn’t tracked)',
    )
  })
})
