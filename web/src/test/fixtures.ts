import type { CardSet } from '@/lib/api'

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
