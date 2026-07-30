import { describe, expect, it } from 'vitest'
import type { KeywordEntry, MatchMode } from '@/lib/api'
import {
  findKeywords,
  firstSentence,
  keywordSlug,
  relatedKeywords,
  splitKeywords,
} from '@/lib/keywords'

function keyword(
  name: string,
  matchMode: MatchMode = 'anywhere',
  over: Partial<KeywordEntry> = {},
) {
  return {
    name,
    slug: keywordSlug(name),
    kind: 'ability',
    text: `${name} does a thing. And then another thing.`,
    parameterized: false,
    match_mode: matchMode,
    ...over,
  } as KeywordEntry
}

describe('keywordSlug', () => {
  // These pairs are asserted again on the Rust side (`slugify_matches_the_spa_fixtures`
  // in api/src/catalog/keywords/mod.rs). If one side changes, both must.
  it('matches the API fixtures', () => {
    expect(keywordSlug('Vigilance')).toBe('vigilance')
    expect(keywordSlug('First strike')).toBe('first-strike')
    expect(keywordSlug("Council's dilemma")).toBe('councils-dilemma')
    expect(keywordSlug('Doctor’s companion')).toBe('doctors-companion')
    expect(keywordSlug('Start your engines!')).toBe('start-your-engines')
    expect(keywordSlug('For Mirrodin!')).toBe('for-mirrodin')
    expect(keywordSlug('Jump-start')).toBe('jump-start')
    expect(keywordSlug('The Ring tempts you')).toBe('the-ring-tempts-you')
    expect(keywordSlug('Hexproof from black')).toBe('hexproof-from-black')
  })

  it('is idempotent, so a canonical slug from the API survives a round trip', () => {
    for (const name of ['First strike', "Council's dilemma", 'For Mirrodin!']) {
      const slug = keywordSlug(name)
      expect(keywordSlug(slug)).toBe(slug)
    }
  })

  it('normalises a slug a user or search engine typed', () => {
    expect(keywordSlug('First-Strike')).toBe('first-strike')
    expect(keywordSlug('VIGILANCE')).toBe('vigilance')
  })
})

describe('firstSentence', () => {
  it('stops at the first sentence break', () => {
    expect(firstSentence('One. Two. Three.')).toBe('One.')
  })

  it('returns the whole text when there is only one sentence', () => {
    expect(firstSentence('Just the one.')).toBe('Just the one.')
  })

  it("doesn't split on a decimal or an abbreviation mid-sentence", () => {
    expect(firstSentence('It costs 1.5 mana. Then it resolves.')).toBe('It costs 1.5 mana.')
  })
})

describe('findKeywords', () => {
  const glossary = [keyword('Flying'), keyword('Trample'), keyword('Vigilance')]

  it('finds a keyword on its own line', () => {
    const found = findKeywords('Flying', glossary)
    expect(found.map((m) => m.entry.name)).toEqual(['Flying'])
  })

  it('finds keywords mid-sentence, since that is where a reader meets them', () => {
    const found = findKeywords(
      'Target creature gains flying and trample until end of turn.',
      glossary,
    )
    expect(found.map((m) => m.entry.name)).toEqual(['Flying', 'Trample'])
    // The matched text keeps the card's own lower-case spelling.
    expect(
      found.map((m) =>
        'Target creature gains flying and trample until end of turn.'.slice(m.start, m.end),
      ),
    ).toEqual(['flying', 'trample'])
  })

  it('returns matches in document order regardless of name length', () => {
    const found = findKeywords('Vigilance, flying', glossary)
    expect(found.map((m) => m.entry.name)).toEqual(['Vigilance', 'Flying'])
  })

  it('marks only the first mention of a keyword', () => {
    const found = findKeywords(
      'Trample. Whenever this creature with trample attacks, trample.',
      glossary,
    )
    expect(found).toHaveLength(1)
    expect(found[0]?.start).toBe(0)
  })

  it('skips reminder text in parentheses', () => {
    const text = "Flying (This creature can't be blocked except by creatures with flying or reach.)"
    const found = findKeywords(text, glossary)
    expect(found).toHaveLength(1)
    expect(found[0]?.start).toBe(0)
  })

  it('tolerates an unclosed parenthesis without matching inside it', () => {
    const found = findKeywords('Trample (this creature has flying', glossary)
    expect(found.map((m) => m.entry.name)).toEqual(['Trample'])
  })

  it("skips the card's own name", () => {
    const found = findKeywords('Flying Men gains flying.', glossary, 'Flying Men')
    expect(found).toHaveLength(1)
    // The surviving match is the lower-case one after "gains", not the title.
    expect(found[0]?.start).toBeGreaterThan(10)
  })

  it("skips each face's name on a multi-faced card", () => {
    const found = findKeywords(
      'Trample Cliffs deals damage.',
      glossary,
      'Trample Cliffs // Flying Men',
    )
    expect(found).toHaveLength(0)
  })

  it('prefers the longest name at the same position', () => {
    const entries = [keyword('Flash'), keyword('Flashback', 'anywhere', { parameterized: true })]
    const found = findKeywords('Flashback {2}{R}', entries)
    expect(found.map((m) => m.entry.name)).toEqual(['Flashback'])
  })

  it("never matches a 'never' keyword", () => {
    const entries = [keyword('Counter', 'never'), keyword('Trample')]
    const found = findKeywords('Counter target spell. Trample.', entries)
    expect(found.map((m) => m.entry.name)).toEqual(['Trample'])
  })

  it('matches only whole words', () => {
    const found = findKeywords('The reachable trampling flier.', glossary)
    expect(found).toHaveLength(0)
  })

  it('handles a name ending in punctuation', () => {
    const entries = [keyword('For Mirrodin!')]
    const found = findKeywords('For Mirrodin! When this Equipment enters…', entries)
    expect(found.map((m) => m.entry.name)).toEqual(['For Mirrodin!'])
  })

  it('is empty for empty text or an empty glossary', () => {
    expect(findKeywords('', glossary)).toEqual([])
    expect(findKeywords('Flying', [])).toEqual([])
  })
})

describe("findKeywords with match_mode 'ability_line'", () => {
  const fear = keyword('Fear', 'ability_line')
  const storm = keyword('Storm', 'ability_line', { parameterized: true })
  const landfall = keyword('Landfall', 'ability_line', { kind: 'ability_word' })

  it('matches the keyword heading its own line', () => {
    expect(findKeywords('Fear', [fear])).toHaveLength(1)
    expect(findKeywords('Flying\nFear\nTrample', [fear])).toHaveLength(1)
  })

  it('matches inside a leading keyword run', () => {
    expect(findKeywords('Flying, fear, trample', [fear])).toHaveLength(1)
  })

  it("does not match the card's title sharing the word", () => {
    // The trap the anchored rule exists for: a line START is not enough, because the
    // card's own name also starts a line.
    expect(findKeywords('Fear of Isolation gets +1/+1.', [fear])).toHaveLength(0)
  })

  it('does not match the word used in an ordinary sentence', () => {
    expect(findKeywords('Creatures you control gain fear until end of turn.', [fear])).toHaveLength(
      0,
    )
    expect(findKeywords('Storm Crow flies.', [storm])).toHaveLength(0)
  })

  it('matches a parameterised keyword followed by its value', () => {
    expect(findKeywords('Storm 2', [storm])).toHaveLength(1)
    expect(findKeywords('Storm {1}{R}', [storm])).toHaveLength(1)
  })

  it('matches an ability word followed by its em dash', () => {
    expect(findKeywords('Landfall — Whenever a land enters…', [landfall])).toHaveLength(1)
  })

  it('does not match an ability word named mid-sentence', () => {
    expect(findKeywords('This deck is built around landfall triggers.', [landfall])).toHaveLength(0)
  })

  it('is not fooled by a non-keyword line that merely starts with the word', () => {
    expect(findKeywords('Fear grips the battlefield.', [fear])).toHaveLength(0)
  })
})

describe('splitKeywords', () => {
  const glossary = [keyword('Flying'), keyword('Trample')]

  it('splits a run into plain text and keyword tokens, losing nothing', () => {
    const text = 'Flying, trample'
    const tokens = splitKeywords(text, glossary)
    expect(tokens.map((t) => (t.type === 'keyword' ? t.value : t.value)).join('')).toBe(text)
    expect(tokens.filter((t) => t.type === 'keyword')).toHaveLength(2)
  })

  it('returns one plain token when nothing matches', () => {
    expect(splitKeywords('Draw a card.', glossary)).toEqual([
      { type: 'text', value: 'Draw a card.' },
    ])
  })

  it('preserves the surrounding text exactly, including newlines', () => {
    const text = 'Flying\nWhenever this creature attacks, draw a card.'
    const tokens = splitKeywords(text, glossary)
    expect(tokens.map((t) => t.value).join('')).toBe(text)
  })
})

describe('relatedKeywords', () => {
  const flying = keyword('Flying', 'anywhere', {
    text: "This creature can't be blocked except by creatures with flying or reach.",
  })
  const reach = keyword('Reach', 'anywhere', {
    text: 'This creature can block creatures with flying.',
  })
  const trample = keyword('Trample')
  const scry = keyword('Scry', 'anywhere', { kind: 'action' })
  const all = [flying, reach, trample, scry]

  it('links keywords the entry mentions', () => {
    expect(relatedKeywords(flying, all).map((k) => k.name)).toContain('Reach')
  })

  it('links keywords that mention the entry', () => {
    expect(relatedKeywords(trample, all).map((k) => k.name)).not.toContain('Trample')
    expect(relatedKeywords(reach, all).map((k) => k.name)).toContain('Flying')
  })

  it('never includes the entry itself', () => {
    for (const entry of all) {
      expect(relatedKeywords(entry, all).map((k) => k.slug)).not.toContain(entry.slug)
    }
  })

  it('is deterministic, so the page does not reshuffle between renders', () => {
    expect(relatedKeywords(flying, all)).toEqual(relatedKeywords(flying, all))
  })

  it('fills up with same-kind neighbours', () => {
    const names = relatedKeywords(trample, all).map((k) => k.name)
    expect(names).toContain('Flying')
    expect(names).toContain('Reach')
  })

  it('fills from entries near this one, not from the head of the glossary', () => {
    // Otherwise every page in a 365-entry glossary ends with the same few A-names.
    const many = Array.from({ length: 40 }, (_, i) =>
      keyword(`Kw${String(i).padStart(2, '0')}`, 'anywhere', { text: 'Unrelated prose.' }),
    )
    const middle = many[20]!
    const names = relatedKeywords(middle, many).map((k) => k.name)
    expect(names).not.toContain('Kw00')
    // Its immediate neighbours on both sides come first.
    expect(names.slice(0, 2).sort()).toEqual(['Kw19', 'Kw21'])
  })

  it('never exceeds the link budget', () => {
    const many = Array.from({ length: 40 }, (_, i) => keyword(`Kw${i}`))
    expect(relatedKeywords(many[5]!, many).length).toBeLessThanOrEqual(8)
  })
})
