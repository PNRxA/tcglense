import { afterEach, describe, expect, it, vi } from 'vitest'
import { forgetSeatToken, playSeatKey, recallSeatToken, rememberSeatToken } from '@/lib/playSeat'

// A seat token is the only proof a guest holds a seat, so the two things worth pinning are the
// two ways this module could lose one: a key that doesn't round-trip (a code cased differently
// on the way back), and a storage failure escaping as an exception and taking the page with it.

afterEach(() => {
  localStorage.clear()
  vi.restoreAllMocks()
})

describe('playSeatKey', () => {
  it('namespaces per game and room, with the code normalised', () => {
    expect(playSeatKey('mtg', 'abc234')).toBe('tcglense_play_seat:mtg:ABC234')
    expect(playSeatKey('mtg', 'ABC234')).toBe(playSeatKey('mtg', 'abc234'))
  })
})

describe('remember/recall/forget', () => {
  it('round-trips a token for one room without touching another', () => {
    rememberSeatToken('mtg', 'ABC234', 'token-a')
    rememberSeatToken('mtg', 'XYZ789', 'token-b')

    expect(recallSeatToken('mtg', 'ABC234')).toBe('token-a')
    expect(recallSeatToken('mtg', 'XYZ789')).toBe('token-b')

    forgetSeatToken('mtg', 'ABC234')
    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
    // Forgetting one seat must not evict the others — a player can hold several at once.
    expect(recallSeatToken('mtg', 'XYZ789')).toBe('token-b')
  })

  it('recalls a token stored under a differently-cased code', () => {
    // The link a friend sent may be lower case and the hub's box upper-cases; both are the
    // same room, and a seat that only comes back for one of them is a seat lost on a refresh.
    rememberSeatToken('mtg', 'abc234', 'token-a')
    expect(recallSeatToken('mtg', 'ABC234')).toBe('token-a')
  })

  it('treats an empty token as forgetting the seat', () => {
    rememberSeatToken('mtg', 'ABC234', 'token-a')
    rememberSeatToken('mtg', 'ABC234', '')
    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
  })

  it('returns null rather than throwing when storage is unreadable', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
  })

  it('swallows a write failure so a blocked store still lets you play this tab', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota')
    })
    expect(() => rememberSeatToken('mtg', 'ABC234', 'token-a')).not.toThrow()
  })

  it('swallows a removal failure', () => {
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(() => forgetSeatToken('mtg', 'ABC234')).not.toThrow()
  })
})
