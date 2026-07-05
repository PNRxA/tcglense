import { describe, it, expect } from 'vitest'

import {
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS,
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS,
  PRODUCT_CARDS_DEFAULT_SORT,
  PRODUCT_CARDS_SORT_OPTIONS,
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS,
  toSortParam,
} from '../cardSort'

describe('toSortParam', () => {
  it('splits a field:dir value into API params', () => {
    expect(toSortParam('price:desc', SET_DEFAULT_SORT)).toEqual({ sort: 'price', dir: 'desc' })
    expect(toSortParam('name:asc', SET_DEFAULT_SORT)).toEqual({ sort: 'name', dir: 'asc' })
  })

  it('falls back to the default for an empty value', () => {
    expect(toSortParam('', SET_DEFAULT_SORT)).toEqual({ sort: 'number', dir: 'asc' })
    expect(toSortParam('', ALL_CARDS_DEFAULT_SORT)).toEqual({ sort: 'name', dir: 'asc' })
  })

  it('defaults a missing or odd direction to ascending', () => {
    expect(toSortParam('cmc', SET_DEFAULT_SORT)).toEqual({ sort: 'cmc', dir: 'asc' })
    expect(toSortParam('cmc:sideways', SET_DEFAULT_SORT)).toEqual({ sort: 'cmc', dir: 'asc' })
  })
})

describe('sort option lists', () => {
  it('expose their defaults as a selectable option', () => {
    expect(SET_SORT_OPTIONS.some((o) => o.value === SET_DEFAULT_SORT)).toBe(true)
    expect(ALL_CARDS_SORT_OPTIONS.some((o) => o.value === ALL_CARDS_DEFAULT_SORT)).toBe(true)
  })

  it('offer collector number in every card sort dropdown, whatever the ghost state', () => {
    // Collector number is a selectable sort everywhere a card list sorts, so it no longer
    // disappears when the collection / wish-list ghost toggle swaps the option set. It stays
    // a non-default option outside a single set (name / recency remain the defaults there).
    expect(SET_SORT_OPTIONS.some((o) => o.value.startsWith('number:'))).toBe(true)
    expect(ALL_CARDS_SORT_OPTIONS.some((o) => o.value.startsWith('number:'))).toBe(true)
    expect(COLLECTION_SORT_OPTIONS.some((o) => o.value.startsWith('number:'))).toBe(true)
    expect(ALL_CARDS_DEFAULT_SORT.startsWith('number:')).toBe(false)
    expect(COLLECTION_DEFAULT_SORT.startsWith('number:')).toBe(false)
  })

  it('parse every option value into a valid sort param', () => {
    for (const option of SET_SORT_OPTIONS) {
      const { sort, dir } = toSortParam(option.value, SET_DEFAULT_SORT)
      expect(sort).toBeTruthy()
      expect(['asc', 'desc']).toContain(dir)
    }
  })

  it('offers a natural-order sentinel plus real card sorts for a product’s cards', () => {
    // The `default` sentinel is a selectable option but is NOT a backend sort key: the query
    // layer maps it to *no* sort param, so it must never be handed to `toSortParam` (which
    // would emit {sort:'default'} → the API rejects it as an unknown sort). It's the default.
    expect(PRODUCT_CARDS_DEFAULT_SORT).toBe('default')
    expect(PRODUCT_CARDS_SORT_OPTIONS.some((o) => o.value === PRODUCT_CARDS_DEFAULT_SORT)).toBe(true)

    // Every *other* option is a real `field:dir` that parses to a valid, non-sentinel param.
    const realOptions = PRODUCT_CARDS_SORT_OPTIONS.filter(
      (o) => o.value !== PRODUCT_CARDS_DEFAULT_SORT,
    )
    expect(realOptions.length).toBeGreaterThan(0)
    for (const option of realOptions) {
      const { sort, dir } = toSortParam(option.value, ALL_CARDS_DEFAULT_SORT)
      expect(sort).toBeTruthy()
      expect(sort).not.toBe('default')
      expect(['asc', 'desc']).toContain(dir)
    }
  })

  it('offer a total-copies quantity sort on the collection/wish-list view (issue #228)', () => {
    // Both directions are selectable and map to the backend's holdings-only `quantity` key.
    expect(toSortParam('quantity:desc', COLLECTION_DEFAULT_SORT)).toEqual({
      sort: 'quantity',
      dir: 'desc',
    })
    expect(toSortParam('quantity:asc', COLLECTION_DEFAULT_SORT)).toEqual({
      sort: 'quantity',
      dir: 'asc',
    })
    expect(COLLECTION_SORT_OPTIONS.filter((o) => o.value.startsWith('quantity:'))).toHaveLength(2)
    // The default stays recency, and every option still parses cleanly.
    expect(COLLECTION_SORT_OPTIONS.some((o) => o.value === COLLECTION_DEFAULT_SORT)).toBe(true)
    for (const option of COLLECTION_SORT_OPTIONS) {
      const { sort, dir } = toSortParam(option.value, COLLECTION_DEFAULT_SORT)
      expect(sort).toBeTruthy()
      expect(['asc', 'desc']).toContain(dir)
    }
  })
})
