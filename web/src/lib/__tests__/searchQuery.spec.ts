import { describe, it, expect } from 'vitest'

import {
  parseToken,
  readFilter,
  readRange,
  removeFilter,
  setRange,
  tokenizeQuery,
  upsertFilter,
} from '../searchQuery'

// The colour control's key group — the helpers take an explicit key list, so the tests
// mirror the aliases the builder passes for a representative control.
const COLOR_KEYS = ['c', 'color', 'colors'] as const

describe('tokenizeQuery', () => {
  it('splits on whitespace', () => {
    expect(tokenizeQuery('a b')).toEqual(['a', 'b'])
  })

  it('collapses runs of whitespace and trims the edges', () => {
    expect(tokenizeQuery('  a   b\tc\n')).toEqual(['a', 'b', 'c'])
  })

  it('keeps a quoted filter value intact as one token', () => {
    expect(tokenizeQuery('o:"draw a card" bolt')).toEqual(['o:"draw a card"', 'bolt'])
  })

  it('keeps a bare quoted phrase as a single token', () => {
    expect(tokenizeQuery('"lightning bolt"')).toEqual(['"lightning bolt"'])
  })

  it('returns no tokens for an empty or whitespace-only string', () => {
    expect(tokenizeQuery('')).toEqual([])
    expect(tokenizeQuery('   ')).toEqual([])
  })
})

describe('parseToken', () => {
  it('parses a two-char operator token', () => {
    expect(parseToken('c>=wu')).toEqual({ neg: false, key: 'c', op: '>=', value: 'wu' })
  })

  it('flags a leading `-` as negated', () => {
    expect(parseToken('-t:land')).toEqual({ neg: true, key: 't', op: ':', value: 'land' })
  })

  it('lowercases the key but preserves the value verbatim', () => {
    expect(parseToken('C:R')).toEqual({ neg: false, key: 'c', op: ':', value: 'R' })
  })

  it('returns null for a bareword, exact phrase or parenthesised group', () => {
    expect(parseToken('bolt')).toBeNull()
    expect(parseToken('!"Lightning Bolt"')).toBeNull()
    expect(parseToken('(c:r')).toBeNull()
  })

  it('returns null for a key with no value', () => {
    expect(parseToken('t:')).toBeNull()
  })

  it('prefers the longest matching operator', () => {
    expect(parseToken('usd<=1')).toEqual({ neg: false, key: 'usd', op: '<=', value: '1' })
    expect(parseToken('mv>=3')).toEqual({ neg: false, key: 'mv', op: '>=', value: '3' })
  })

  it('reads a cross-column comparison as a single `>` op', () => {
    expect(parseToken('pow>tou')).toEqual({ neg: false, key: 'pow', op: '>', value: 'tou' })
  })

  it('treats a paren-bearing token as opaque so groups stay intact', () => {
    // The trailing `)` of `(t:creature or t:artifact)` glues onto the last token.
    expect(parseToken('t:artifact)')).toBeNull()
    expect(parseToken('t:creature(')).toBeNull()
  })
})

describe('readFilter', () => {
  it('returns the first matching non-negated token', () => {
    expect(readFilter('bolt c:r', COLOR_KEYS)).toEqual({
      neg: false,
      key: 'c',
      op: ':',
      value: 'r',
    })
  })

  it('matches any alias in the key group', () => {
    expect(readFilter('color:u', COLOR_KEYS)?.value).toBe('u')
    expect(readFilter('colors:g', COLOR_KEYS)?.value).toBe('g')
  })

  it('ignores negated tokens', () => {
    expect(readFilter('-c:r', COLOR_KEYS)).toBeNull()
  })

  it('returns null when nothing matches', () => {
    expect(readFilter('bolt t:land', COLOR_KEYS)).toBeNull()
  })

  it('narrows to the given operators when `ops` is passed', () => {
    expect(readFilter('c<=1', COLOR_KEYS, ['>', '>='])).toBeNull()
    expect(readFilter('c>=1', COLOR_KEYS, ['>', '>='])?.op).toBe('>=')
  })
})

describe('removeFilter', () => {
  it('drops non-negated matching tokens and preserves order + free text', () => {
    expect(removeFilter('bolt c:r fire', COLOR_KEYS)).toBe('bolt fire')
  })

  it('does not remove a negated token', () => {
    expect(removeFilter('-t:land', ['t', 'type'])).toBe('-t:land')
  })

  it('keeps a negated sibling while dropping the positive one', () => {
    expect(removeFilter('bolt -t:land t:goblin', ['t', 'type'])).toBe('bolt -t:land')
  })

  it('narrows to the given operators', () => {
    expect(removeFilter('mv>=2 mv<=5', ['mv'], ['>='])).toBe('mv<=5')
  })

  it('rejoins with single spaces, trimmed', () => {
    expect(removeFilter('  bolt   c:r   fire  ', COLOR_KEYS)).toBe('bolt fire')
  })

  it('leaves a filter inside a parenthesised group intact', () => {
    // Neither `(t:creature` (leading paren) nor `t:artifact)` (trailing paren) is a
    // matchable token, so removing type filters can't unbalance the group.
    expect(removeFilter('(t:creature or t:artifact)', ['t', 'type'])).toBe(
      '(t:creature or t:artifact)',
    )
  })
})

describe('upsertFilter', () => {
  it('replaces an existing filter rather than duplicating it', () => {
    expect(upsertFilter('bolt c:u', COLOR_KEYS, 'c', ':', 'r')).toBe('bolt c:r')
  })

  it('appends the filter when none exists', () => {
    expect(upsertFilter('bolt', COLOR_KEYS, 'c', ':', 'r')).toBe('bolt c:r')
  })

  it('emits just the token when the query was empty', () => {
    expect(upsertFilter('', COLOR_KEYS, 'c', ':', 'r')).toBe('c:r')
  })

  it('clears the filter for an empty value', () => {
    expect(upsertFilter('c:r', COLOR_KEYS, 'c', ':', '')).toBe('')
  })

  it('clears only its own filter, keeping free text', () => {
    expect(upsertFilter('bolt c:r', COLOR_KEYS, 'c', ':', '')).toBe('bolt')
  })
})

describe('readRange', () => {
  it('reads min from `>`/`>=` and max from `<`/`<=`', () => {
    expect(readRange('mv>=2 mv<=5', ['mv'])).toEqual({ min: '2', max: '5' })
  })

  it('reads a strict-inequality range', () => {
    expect(readRange('usd>1 usd<10', ['usd'])).toEqual({ min: '1', max: '10' })
  })

  it('leaves a missing bound as an empty string', () => {
    expect(readRange('mv>=2', ['mv'])).toEqual({ min: '2', max: '' })
    expect(readRange('mv<=5', ['mv'])).toEqual({ min: '', max: '5' })
  })

  it('is empty when no range token is present', () => {
    expect(readRange('bolt mv=3', ['mv'])).toEqual({ min: '', max: '' })
  })
})

describe('setRange', () => {
  it('rewrites both bounds, removing any prior tokens for the key', () => {
    expect(setRange('foo mv=3', ['mv'], 'mv', '2', '5')).toBe('foo mv>=2 mv<=5')
  })

  it('emits only the given bound', () => {
    expect(setRange('', ['mv'], 'mv', '2', '')).toBe('mv>=2')
    expect(setRange('', ['mv'], 'mv', '', '5')).toBe('mv<=5')
  })

  it('clears the key when both bounds are empty, keeping free text', () => {
    expect(setRange('bolt mv>=2 mv<=5', ['mv'], 'mv', '', '')).toBe('bolt')
  })

  it('preserves unrelated tokens around the range', () => {
    expect(setRange('bolt c:r', ['mv'], 'mv', '1', '4')).toBe('bolt c:r mv>=1 mv<=4')
  })
})
