import { describe, expect, it } from 'vitest'
import { defaultFormatFor, formatGroupsFor } from '@/lib/deckFormats'

describe('defaultFormatFor', () => {
  it("preselects MTG's most-played format for the New-deck dialog", () => {
    expect(defaultFormatFor('mtg')).toBe('Commander')
  })

  it('defaults to no format for a game without curated formats', () => {
    expect(defaultFormatFor('somegame')).toBe('')
  })

  it('always names a real select option, so the field can resolve it', () => {
    const options = formatGroupsFor('mtg').flatMap((group) => group.options)
    expect(options).toContain(defaultFormatFor('mtg'))
  })
})
