import type { Card, CardSet } from '@/lib/api'

/** A neutral card-printing fixture for component/composable tests. */
export function makeCard(id: string, over: Partial<Card> = {}): Card {
  return {
    id,
    name: 'Island',
    set_code: 'tst',
    set_name: 'Test Set',
    collector_number: '1',
    rarity: 'common',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: 0,
    type_line: 'Basic Land — Island',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: ['U'],
    colors: [],
    layout: 'normal',
    prices: { usd: '0.25', usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...over,
  }
}

/**
 * A neutral `CardSet` fixture for tests: every field defaults to a benign value so a spec
 * only spells out what it asserts on (override the rest via `over`). Shared across the
 * set-grouping / set-tile specs so a new `CardSet` field is one edit here, not three.
 */
export function makeCardSet(code: string, over: Partial<CardSet> = {}): CardSet {
  return {
    code,
    name: code.toUpperCase(),
    set_type: 'expansion',
    released_at: '2024-01-01',
    card_count: 100,
    icon_svg_uri: null,
    parent_set_code: null,
    has_drops: false,
    has_subtypes: false,
    ...over,
  }
}
