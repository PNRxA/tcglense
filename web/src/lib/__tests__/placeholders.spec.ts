import { describe, it, expect } from 'vitest'
import { QueryClient } from '@tanstack/vue-query'

import { findCardInCache, findProductInCache, findSetInCache } from '../placeholders'

// The scavengers read live query-cache entries, so drive them against a real QueryClient
// seeded via setQueryData under the runtime key families (built from refs, but plain
// values once cached). Minimal shapes stand in for the DTOs — the scan only reads
// `.id`/`.code`/`.name` and unwraps `.card`/`.product`.
function client() {
  return new QueryClient()
}

const card = (id: string, name = `Card ${id}`) => ({ id, name })
const product = (id: string, name = `Product ${id}`) => ({ id, name, product_type: 'booster_box' })
const set = (code: string, name = `Set ${code}`) => ({ code, name })

describe('findCardInCache', () => {
  it('hits a card-list page', () => {
    const qc = client()
    qc.setQueryData(['cards', 'mtg', '', 'default', 1], { data: [card('a'), card('b')] })
    expect(findCardInCache(qc, 'mtg', 'b')?.name).toBe('Card b')
  })

  it('hits a set-cards page', () => {
    const qc = client()
    qc.setQueryData(['set-cards', 'mtg', 'neo', '', 'default', 1, false], { data: [card('x')] })
    expect(findCardInCache(qc, 'mtg', 'x')?.id).toBe('x')
  })

  it('unwraps a collection holding entry', () => {
    const qc = client()
    qc.setQueryData(['collection', 'mtg', undefined, '', 'default', 1, false], {
      data: [{ card: card('own'), quantity: 2, foil_quantity: 0 }],
    })
    expect(findCardInCache(qc, 'mtg', 'own')?.id).toBe('own')
  })

  it('unwraps a wishlist holding entry', () => {
    const qc = client()
    qc.setQueryData(['wishlist', 'mtg', undefined, '', 'default', 1, false], {
      data: [{ card: card('want'), quantity: 1, foil_quantity: 0 }],
    })
    expect(findCardInCache(qc, 'mtg', 'want')?.id).toBe('want')
  })

  it('hits a card-prints entry', () => {
    const qc = client()
    qc.setQueryData(['card-prints', 'mtg', 'src'], { data: [card('p1'), card('p2')] })
    expect(findCardInCache(qc, 'mtg', 'p2')?.id).toBe('p2')
  })

  it('hits a card-printings entry', () => {
    const qc = client()
    qc.setQueryData(['card-printings', 'mtg', 'Lightning Bolt'], { data: [card('lb')] })
    expect(findCardInCache(qc, 'mtg', 'lb')?.id).toBe('lb')
  })

  it('returns undefined on a miss', () => {
    const qc = client()
    qc.setQueryData(['cards', 'mtg', '', 'default', 1], { data: [card('a')] })
    expect(findCardInCache(qc, 'mtg', 'zzz')).toBeUndefined()
  })

  it('does not cross games', () => {
    const qc = client()
    qc.setQueryData(['cards', 'pokemon', '', 'default', 1], { data: [card('a')] })
    expect(findCardInCache(qc, 'mtg', 'a')).toBeUndefined()
  })

  it('is defensive about malformed / placeholder entries', () => {
    const qc = client()
    qc.setQueryData(['cards', 'mtg', '', 'default', 1], null)
    qc.setQueryData(['collection', 'mtg', undefined, '', 'default', 1, false], {
      data: [null, { notacard: true }, { card: null }],
    })
    expect(() => findCardInCache(qc, 'mtg', 'a')).not.toThrow()
    expect(findCardInCache(qc, 'mtg', 'a')).toBeUndefined()
  })

  it('takes the first hit in scan order', () => {
    const qc = client()
    qc.setQueryData(['card-prints', 'mtg', 'src'], { data: [card('dup', 'from prints')] })
    qc.setQueryData(['cards', 'mtg', '', 'default', 1], { data: [card('dup', 'from list')] })
    expect(findCardInCache(qc, 'mtg', 'dup')?.name).toBe('from list')
  })
})

describe('findProductInCache', () => {
  it('hits a product-list page', () => {
    const qc = client()
    qc.setQueryData(['products', 'mtg', '', '', '', 'default', 1], { data: [product('bb')] })
    expect(findProductInCache(qc, 'mtg', 'bb')?.id).toBe('bb')
  })

  it('unwraps a card-sealed section ref', () => {
    const qc = client()
    qc.setQueryData(['card-sealed', 'mtg', 'card1'], {
      data: [{ product: product('sealed1'), membership: 'contains', foil: false }],
    })
    expect(findProductInCache(qc, 'mtg', 'sealed1')?.id).toBe('sealed1')
  })

  it('unwraps forward and reverse product-composition refs', () => {
    const qc = client()
    qc.setQueryData(['product-contents', 'mtg', 'box'], {
      data: [{ product: product('pack'), kind: 'sealed', quantity: 36 }],
    })
    qc.setQueryData(['product-containers', 'mtg', 'pack'], {
      data: [{ product: product('box'), quantity: 36 }],
    })
    expect(findProductInCache(qc, 'mtg', 'pack')?.id).toBe('pack')
    expect(findProductInCache(qc, 'mtg', 'box')?.id).toBe('box')
  })

  it('returns undefined on a miss', () => {
    const qc = client()
    qc.setQueryData(['products', 'mtg', '', '', '', 'default', 1], { data: [product('bb')] })
    expect(findProductInCache(qc, 'mtg', 'nope')).toBeUndefined()
  })
})

describe('findSetInCache', () => {
  it('hits the sets list', () => {
    const qc = client()
    qc.setQueryData(['sets', 'mtg'], { data: [set('neo'), set('bro')] })
    expect(findSetInCache(qc, 'mtg', 'bro')?.name).toBe('Set bro')
  })

  it('returns undefined on a miss (and does not cross games)', () => {
    const qc = client()
    qc.setQueryData(['sets', 'pokemon'], { data: [set('base')] })
    expect(findSetInCache(qc, 'mtg', 'base')).toBeUndefined()
    expect(findSetInCache(qc, 'pokemon', 'nope')).toBeUndefined()
  })
})
