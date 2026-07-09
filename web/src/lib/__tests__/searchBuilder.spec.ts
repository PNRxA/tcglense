import { describe, it, expect } from 'vitest'

import {
  activeFilterCount,
  CARD_FLAG_OPTIONS,
  clearBuilderFilters,
  COLOR_MODES,
  COLOR_PIPS,
  FORMAT_OPTIONS,
  getArtist,
  getCardFlags,
  getColors,
  getFormat,
  getManaValue,
  getOracleText,
  getPower,
  getRarity,
  getSetCode,
  getToughness,
  getType,
  getUsd,
  RARITY_OPTIONS,
  setArtist,
  setCardFlag,
  setColors,
  setFormat,
  setManaValue,
  setOracleText,
  setPower,
  setRarity,
  setSetCode,
  setToughness,
  setType,
  setUsd,
  TYPE_OPTIONS,
  type ColorSelection,
} from '../searchBuilder'

describe('option lists', () => {
  it('lists the five WUBRG pips in canonical order', () => {
    expect(COLOR_PIPS.map((p) => p.letter)).toEqual(['w', 'u', 'b', 'r', 'g'])
  })

  it('offers the three colour comparison modes', () => {
    expect(COLOR_MODES.map((m) => m.value)).toEqual(['including', 'exactly', 'atMost'])
  })

  it('starts the type/rarity/format lists with an "Any" empty value', () => {
    expect(TYPE_OPTIONS[0]?.value).toBe('')
    expect(RARITY_OPTIONS[0]?.value).toBe('')
    expect(FORMAT_OPTIONS[0]?.value).toBe('')
  })
})

describe('colours', () => {
  it('reads an empty selection from a query with no colour filter', () => {
    expect(getColors('')).toEqual({ letters: [], colorless: false, mode: 'including' })
    expect(getColors('bolt t:creature')).toEqual({
      letters: [],
      colorless: false,
      mode: 'including',
    })
  })

  it('writes chosen colours in WUBRG order', () => {
    const sel: ColorSelection = { letters: ['u', 'w'], colorless: false, mode: 'including' }
    expect(setColors('', sel)).toBe('c:wu')
  })

  it('maps the mode to the comparison operator', () => {
    const base: ColorSelection = { letters: ['w', 'u'], colorless: false, mode: 'including' }
    expect(setColors('', { ...base, mode: 'including' })).toBe('c:wu')
    expect(setColors('', { ...base, mode: 'exactly' })).toBe('c=wu')
    expect(setColors('', { ...base, mode: 'atMost' })).toBe('c<=wu')
  })

  it('reads the mode back from the operator', () => {
    expect(getColors('c:wu').mode).toBe('including')
    expect(getColors('c>=wu').mode).toBe('including') // `>=` is the same "at least" as `:`
    expect(getColors('c=wu').mode).toBe('exactly')
    expect(getColors('c<=wu').mode).toBe('atMost')
  })

  it('ignores an operator the pips cannot round-trip rather than inverting it', () => {
    // `!=`/`<`/`>` mean things the three modes cannot express, so they stay as raw text.
    expect(getColors('c!=w')).toEqual({ letters: [], colorless: false, mode: 'including' })
    expect(getColors('c<u')).toEqual({ letters: [], colorless: false, mode: 'including' })
    expect(getColors('c>w')).toEqual({ letters: [], colorless: false, mode: 'including' })
  })

  it('round-trips colourless', () => {
    const sel: ColorSelection = { letters: [], colorless: true, mode: 'including' }
    expect(setColors('', sel)).toBe('c:c')
    expect(getColors('c:c').colorless).toBe(true)
    expect(getColors('c:colorless').colorless).toBe(true)
  })

  it('ignores a non-letter colour value rather than misreading it', () => {
    expect(getColors('c:azorius')).toEqual({ letters: [], colorless: false, mode: 'including' })
    expect(getColors('c:m')).toEqual({ letters: [], colorless: false, mode: 'including' })
  })

  it('round-trips every selection through set then get', () => {
    const selections: ColorSelection[] = [
      { letters: ['w'], colorless: false, mode: 'including' },
      { letters: ['w', 'u'], colorless: false, mode: 'exactly' },
      { letters: ['b', 'r', 'g'], colorless: false, mode: 'atMost' },
      { letters: [], colorless: true, mode: 'including' },
    ]
    for (const sel of selections) {
      expect(getColors(setColors('', sel))).toEqual(sel)
    }
  })

  it('does not disturb free text when setting a colour', () => {
    const sel: ColorSelection = { letters: ['r'], colorless: false, mode: 'including' }
    expect(setColors('lightning bolt o:draw', sel)).toBe('lightning bolt o:draw c:r')
  })

  it('clears the colour filter for an empty selection', () => {
    const sel: ColorSelection = { letters: [], colorless: false, mode: 'including' }
    expect(setColors('bolt c:r', sel)).toBe('bolt')
  })
})

describe('type', () => {
  it('reflects a recognised type value back', () => {
    expect(getType('t:creature')).toBe('creature')
    expect(getType('type:land')).toBe('land')
  })

  it('ignores a value outside the option list', () => {
    expect(getType('t:goblin')).toBe('')
    expect(getType('')).toBe('')
  })

  it('writes and clears the type filter', () => {
    expect(setType('', 'creature')).toBe('t:creature')
    expect(setType('t:creature', '')).toBe('')
  })

  it('preserves free text when setting the type', () => {
    expect(setType('bolt', 'instant')).toBe('bolt t:instant')
  })

  it('does not unbalance a hand-typed parenthesised group', () => {
    // The group is opaque to the builder, so the new type is appended after it, not
    // spliced into it (which used to eat the closing paren -> a 422 on the backend).
    expect(setType('(t:creature or t:artifact)', 'land')).toBe('(t:creature or t:artifact) t:land')
  })
})

describe('format', () => {
  it('reflects a recognised format value back', () => {
    expect(getFormat('f:modern')).toBe('modern')
    expect(getFormat('format:commander')).toBe('commander')
  })

  it('ignores an unknown format', () => {
    expect(getFormat('f:frontier')).toBe('')
    expect(getFormat('')).toBe('')
  })

  it('writes and clears the format filter', () => {
    expect(setFormat('', 'modern')).toBe('f:modern')
    expect(setFormat('f:modern', '')).toBe('')
  })
})

describe('rarity', () => {
  it('reads an at-least comparison as orHigher', () => {
    expect(getRarity('r>=rare')).toEqual({ value: 'rare', orHigher: true })
  })

  it('reads an exact match without orHigher', () => {
    expect(getRarity('r:common')).toEqual({ value: 'common', orHigher: false })
  })

  it('is empty for an unknown rarity or no filter', () => {
    expect(getRarity('r:special')).toEqual({ value: '', orHigher: false })
    expect(getRarity('')).toEqual({ value: '', orHigher: false })
  })

  it('writes the rarity with the right operator', () => {
    expect(setRarity('', { value: 'mythic', orHigher: false })).toBe('r:mythic')
    expect(setRarity('', { value: 'mythic', orHigher: true })).toBe('r>=mythic')
  })
})

describe('mana value (range)', () => {
  it('reads both bounds', () => {
    expect(getManaValue('mv>=2 mv<=5')).toEqual({ min: '2', max: '5' })
  })

  it('keeps the other bound when changing one', () => {
    let query = 'mv>=2 mv<=5'
    query = setManaValue(query, { ...getManaValue(query), min: '3' })
    expect(query).toBe('mv>=3 mv<=5')
    query = setManaValue(query, { ...getManaValue(query), max: '6' })
    expect(query).toBe('mv>=3 mv<=6')
  })

  it('reads a mv range written against a `cmc` alias key', () => {
    expect(setManaValue('cmc>=1', { min: '2', max: '' })).toBe('mv>=2')
  })
})

describe('price (usd, range)', () => {
  it('reads both bounds', () => {
    expect(getUsd('usd>=1 usd<=10')).toEqual({ min: '1', max: '10' })
  })

  it('keeps the other bound when changing one', () => {
    let query = 'usd>=1 usd<=10'
    query = setUsd(query, { ...getUsd(query), max: '20' })
    expect(query).toBe('usd>=1 usd<=20')
    query = setUsd(query, { ...getUsd(query), min: '2' })
    expect(query).toBe('usd>=2 usd<=20')
  })
})

describe('power / toughness (ranges)', () => {
  it('reads and writes power bounds', () => {
    expect(getPower('pow>=2 pow<=5')).toEqual({ min: '2', max: '5' })
    expect(setPower('', { min: '2', max: '' })).toBe('pow>=2')
  })

  it('reads and writes toughness bounds, including via the long alias', () => {
    expect(getToughness('toughness>=3')).toEqual({ min: '3', max: '' })
    expect(setToughness('toughness>=3', { min: '', max: '4' })).toBe('tou<=4')
  })

  it('allows negative bounds', () => {
    expect(setPower('', { min: '-1', max: '' })).toBe('pow>=-1')
    expect(getPower('pow>=-1')).toEqual({ min: '-1', max: '' })
  })
})

describe('free-text filters (oracle text, set, artist)', () => {
  it('round-trips a single-word value unquoted', () => {
    expect(setOracleText('', 'draw')).toBe('o:draw')
    expect(getOracleText('o:draw')).toBe('draw')
  })

  it('quotes a multi-word value and strips the quotes back off on read', () => {
    expect(setOracleText('bolt', 'draw a card')).toBe('bolt o:"draw a card"')
    expect(getOracleText('o:"draw a card"')).toBe('draw a card')
  })

  it('preserves inner whitespace mid-typing (no trimming on write)', () => {
    // A trailing space must survive the get/set cycle or the user can't type a phrase.
    const query = setOracleText('', 'draw ')
    expect(getOracleText(query)).toBe('draw ')
  })

  it('drops quote characters rather than severing the token', () => {
    expect(setOracleText('', 'draw" a')).toBe('o:"draw a"')
  })

  it('treats a whitespace-only value as empty and clears the filter', () => {
    expect(setOracleText('o:draw', '  ')).toBe('')
  })

  it('reads the oracle alias and writes back the canonical key', () => {
    expect(getOracleText('oracle:flying')).toBe('flying')
    expect(setOracleText('oracle:flying', 'haste')).toBe('o:haste')
  })

  it('round-trips the set code across its aliases', () => {
    expect(getSetCode('e:mh3')).toBe('mh3')
    expect(setSetCode('edition:neo', 'mh3')).toBe('s:mh3')
    expect(setSetCode('s:mh3', '')).toBe('')
  })

  it('round-trips the artist without touching the artists count key', () => {
    expect(setArtist('artists>1', 'rebecca guay')).toBe('artists>1 a:"rebecca guay"')
    expect(getArtist('a:"rebecca guay" artists>1')).toBe('rebecca guay')
  })
})

describe('card flags (is: toggles)', () => {
  it('offers each flag once', () => {
    const values = CARD_FLAG_OPTIONS.map((o) => o.value)
    expect(new Set(values).size).toBe(values.length)
  })

  it('reads the offered flags present in the query', () => {
    expect(getCardFlags('')).toEqual([])
    expect(getCardFlags('is:foil bolt is:reprint')).toEqual(['foil', 'reprint'])
  })

  it('turns a single flag on and off without touching other is: values', () => {
    let query = setCardFlag('is:mdfc', 'foil', true)
    expect(query).toBe('is:mdfc is:foil')
    query = setCardFlag(query, 'foil', false)
    expect(query).toBe('is:mdfc')
  })

  it('leaves a negated flag alone', () => {
    expect(setCardFlag('-is:foil', 'foil', true)).toBe('-is:foil is:foil')
  })
})

describe('activeFilterCount', () => {
  it('is zero for an empty or plain-text query', () => {
    expect(activeFilterCount('')).toBe(0)
    expect(activeFilterCount('lightning bolt')).toBe(0)
  })

  it('counts each active control once', () => {
    expect(activeFilterCount('c:r t:creature r:rare mv>=2 f:modern usd<=5')).toBe(6)
    expect(activeFilterCount('o:draw s:mh3 a:guay pow>=2 tou<=4 is:foil')).toBe(6)
  })

  it('counts a range control once even with both bounds set', () => {
    expect(activeFilterCount('mv>=2 mv<=5')).toBe(1)
  })

  it('counts the flags group once even with several flags set', () => {
    expect(activeFilterCount('is:foil is:promo is:reprint')).toBe(1)
  })

  it('counts a colourless selection as an active colour filter', () => {
    expect(activeFilterCount('c:c')).toBe(1)
  })

  it('counts a builder-owned token the controls cannot reflect, matching Clear', () => {
    // A hand-typed value the pips/selects can't show is still builder-owned, so it must
    // count (badge shown, Clear enabled) since clearBuilderFilters would remove it.
    expect(activeFilterCount('c:golgari')).toBe(1)
    expect(activeFilterCount('r:special')).toBe(1)
    expect(activeFilterCount('c:3 t:creature')).toBe(2)
  })
})

describe('clearBuilderFilters', () => {
  it('strips every builder-owned filter but keeps free text and unrelated tokens', () => {
    expect(clearBuilderFilters('bolt c:r t:goblin -o:flying foo:bar')).toBe(
      'bolt -o:flying foo:bar',
    )
  })

  it('clears every control key group', () => {
    expect(clearBuilderFilters('c:r t:creature r:rare mv>=2 f:modern usd<=5')).toBe('')
    expect(clearBuilderFilters('o:draw s:mh3 a:guay pow>=2 tou<=4 is:foil')).toBe('')
  })

  it('clears a builder-owned is: value the toggles cannot show', () => {
    // Same philosophy as `r:special`: the group owns the key, so count and Clear agree.
    expect(clearBuilderFilters('bolt is:mdfc')).toBe('bolt')
  })

  it('leaves a query with no builder filters unchanged', () => {
    expect(clearBuilderFilters('lightning bolt kw:flying')).toBe('lightning bolt kw:flying')
  })
})
