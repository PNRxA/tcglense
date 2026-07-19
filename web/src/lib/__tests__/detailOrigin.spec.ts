import { describe, expect, it } from 'vitest'
import type { LocationQueryRaw } from 'vue-router'
import {
  applyDetailOrigin,
  DETAIL_ORIGIN_KEY,
  encodeDetailOrigin,
  parseDetailOrigin,
} from '../detailOrigin'

describe('encodeDetailOrigin', () => {
  it('joins kind and id with a colon', () => {
    expect(encodeDetailOrigin('card', 'abc')).toBe('card:abc')
    expect(encodeDetailOrigin('product', 'xyz')).toBe('product:xyz')
  })
})

describe('parseDetailOrigin', () => {
  it('round-trips an encoded marker', () => {
    expect(parseDetailOrigin(encodeDetailOrigin('product', 'p1'))).toEqual({
      kind: 'product',
      id: 'p1',
    })
  })

  it('splits on the FIRST colon so an id containing a colon survives', () => {
    expect(parseDetailOrigin('card:a:b:c')).toEqual({ kind: 'card', id: 'a:b:c' })
  })

  it('rejects a non-string', () => {
    expect(parseDetailOrigin(undefined)).toBeNull()
    expect(parseDetailOrigin(null)).toBeNull()
    expect(parseDetailOrigin(['card:x'])).toBeNull()
    expect(parseDetailOrigin(42)).toBeNull()
  })

  it('rejects a value with no colon, a leading colon, or an empty id', () => {
    expect(parseDetailOrigin('card')).toBeNull()
    expect(parseDetailOrigin(':x')).toBeNull()
    expect(parseDetailOrigin('card:')).toBeNull()
    expect(parseDetailOrigin('')).toBeNull()
  })

  it('rejects an unknown kind', () => {
    expect(parseDetailOrigin('set:blc')).toBeNull()
    expect(parseDetailOrigin('deck:5')).toBeNull()
  })
})

describe('applyDetailOrigin', () => {
  it('sets the marker when an origin id is given', () => {
    const query: LocationQueryRaw = { card: 'a' }
    applyDetailOrigin(query, 'product', 'p1')
    expect(query[DETAIL_ORIGIN_KEY]).toBe('product:p1')
  })

  it('drops the marker when there is no origin id', () => {
    const query: LocationQueryRaw = { card: 'a', [DETAIL_ORIGIN_KEY]: 'product:stale' }
    applyDetailOrigin(query, 'product', null)
    expect(DETAIL_ORIGIN_KEY in query).toBe(false)

    const query2: LocationQueryRaw = { card: 'a', [DETAIL_ORIGIN_KEY]: 'product:stale' }
    applyDetailOrigin(query2, 'product', undefined)
    expect(DETAIL_ORIGIN_KEY in query2).toBe(false)
  })

  it('mutates and returns the same query object', () => {
    const query: LocationQueryRaw = {}
    expect(applyDetailOrigin(query, 'card', 'c1')).toBe(query)
  })
})
