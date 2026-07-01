// Domain layer for the advanced-search builder: the option lists the panel offers and
// the get/set functions that map each control to the Scryfall tokens the backend parser
// understands (api/src/scryfall/search/). Everything here is pure and string-level — it
// reads/writes a query string via the token helpers in searchQuery.ts — so the panel
// (AdvancedSearchPanel.vue) stays presentation-only and the mapping is unit-testable.
//
// Each control owns a small set of keys and only touches its own tokens, so hand-typed
// free text, quoted phrases, regexes and unrelated filters survive untouched. A numeric
// *range* control (mana value, price) owns its key outright: setting either bound
// rewrites the key's `>=`/`<=` tokens, so an unusual hand-typed form for that same key
// (`mv:even`, `mv=3`) is replaced once the range control is used.

import { readFilter, readRange, removeFilter, setRange, upsertFilter } from './searchQuery'

// --- Option lists (value is the token value; '' means "Any", i.e. no filter) ---------

/** The five WUBRG colour pips, in canonical order. */
export const COLOR_PIPS = [
  { letter: 'w', label: 'White' },
  { letter: 'u', label: 'Blue' },
  { letter: 'b', label: 'Black' },
  { letter: 'r', label: 'Red' },
  { letter: 'g', label: 'Green' },
] as const

/** How the chosen colours are compared (maps to the `c:`/`c=`/`c<=` operators). */
export type ColorMode = 'including' | 'exactly' | 'atMost'

export const COLOR_MODES: readonly { value: ColorMode; label: string }[] = [
  { value: 'including', label: 'Including' },
  { value: 'exactly', label: 'Exactly' },
  { value: 'atMost', label: 'At most' },
]

export const TYPE_OPTIONS: readonly { value: string; label: string }[] = [
  { value: '', label: 'Any type' },
  { value: 'creature', label: 'Creature' },
  { value: 'instant', label: 'Instant' },
  { value: 'sorcery', label: 'Sorcery' },
  { value: 'artifact', label: 'Artifact' },
  { value: 'enchantment', label: 'Enchantment' },
  { value: 'planeswalker', label: 'Planeswalker' },
  { value: 'land', label: 'Land' },
  { value: 'battle', label: 'Battle' },
]

export const RARITY_OPTIONS: readonly { value: string; label: string }[] = [
  { value: '', label: 'Any' },
  { value: 'common', label: 'Common' },
  { value: 'uncommon', label: 'Uncommon' },
  { value: 'rare', label: 'Rare' },
  { value: 'mythic', label: 'Mythic' },
]

export const FORMAT_OPTIONS: readonly { value: string; label: string }[] = [
  { value: '', label: 'Any format' },
  { value: 'standard', label: 'Standard' },
  { value: 'pioneer', label: 'Pioneer' },
  { value: 'modern', label: 'Modern' },
  { value: 'legacy', label: 'Legacy' },
  { value: 'vintage', label: 'Vintage' },
  { value: 'pauper', label: 'Pauper' },
  { value: 'commander', label: 'Commander' },
  { value: 'brawl', label: 'Brawl' },
  { value: 'historic', label: 'Historic' },
  { value: 'premodern', label: 'Premodern' },
  { value: 'oldschool', label: 'Old School' },
]

// --- Key groups each control owns (aliases the backend accepts) ----------------------

const COLOR_KEYS = ['c', 'color', 'colors'] as const
const TYPE_KEYS = ['t', 'type'] as const
const RARITY_KEYS = ['r', 'rarity'] as const
const MV_KEYS = ['mv', 'cmc', 'manavalue'] as const
const FORMAT_KEYS = ['f', 'format', 'legal'] as const
const USD_KEYS = ['usd'] as const

const WUBRG = ['w', 'u', 'b', 'r', 'g']

/** Keep colour letters in WUBRG order and drop anything that isn't a colour. */
function orderColors(letters: readonly string[]): string[] {
  return WUBRG.filter((c) => letters.includes(c))
}

// --- Colours -------------------------------------------------------------------------

export interface ColorSelection {
  letters: string[]
  colorless: boolean
  mode: ColorMode
}

const EMPTY_COLORS: ColorSelection = { letters: [], colorless: false, mode: 'including' }

/** Read the current colour filter, or an empty selection when none/unrecognised. */
export function getColors(query: string): ColorSelection {
  const token = readFilter(query, COLOR_KEYS)
  if (!token) return { ...EMPTY_COLORS }
  const value = token.value.toLowerCase()
  if (value === 'c' || value === 'colorless') {
    return { letters: [], colorless: true, mode: 'including' }
  }
  // Only reflect a plain colour-letter value (wubrg); leave nicknames, `m`, and colour
  // counts for the user to edit as text rather than misreading them into pips.
  if (!/^[wubrg]+$/.test(value)) return { ...EMPTY_COLORS }
  // Only reflect an operator the pips can round-trip losslessly (`:`/`>=` = including,
  // `=` = exactly, `<=` = at most). `!=`/`<`/`>` mean something the three modes can't
  // express, so leave them as raw text rather than misrepresenting (and, on edit,
  // inverting) the filter.
  let mode: ColorMode
  if (token.op === '=') mode = 'exactly'
  else if (token.op === '<=') mode = 'atMost'
  else if (token.op === ':' || token.op === '>=') mode = 'including'
  else return { ...EMPTY_COLORS }
  return { letters: orderColors([...value]), colorless: false, mode }
}

/** Write a colour selection back into the query (empty selection clears it). */
export function setColors(query: string, selection: ColorSelection): string {
  if (selection.colorless) return upsertFilter(query, COLOR_KEYS, 'c', ':', 'c')
  const letters = orderColors(selection.letters)
  if (!letters.length) return removeFilter(query, COLOR_KEYS)
  const op = selection.mode === 'exactly' ? '=' : selection.mode === 'atMost' ? '<=' : ':'
  return upsertFilter(query, COLOR_KEYS, 'c', op, letters.join(''))
}

// --- Type ----------------------------------------------------------------------------

export function getType(query: string): string {
  const value = readFilter(query, TYPE_KEYS)?.value.toLowerCase() ?? ''
  return TYPE_OPTIONS.some((o) => o.value === value) ? value : ''
}

export function setType(query: string, value: string): string {
  return upsertFilter(query, TYPE_KEYS, 't', ':', value)
}

// --- Rarity --------------------------------------------------------------------------

export interface RaritySelection {
  value: string
  orHigher: boolean
}

export function getRarity(query: string): RaritySelection {
  const token = readFilter(query, RARITY_KEYS)
  const value = token?.value.toLowerCase() ?? ''
  if (!RARITY_OPTIONS.some((o) => o.value === value) || !value)
    return { value: '', orHigher: false }
  return { value, orHigher: token!.op === '>=' }
}

export function setRarity(query: string, selection: RaritySelection): string {
  const op = selection.orHigher ? '>=' : ':'
  return upsertFilter(query, RARITY_KEYS, 'r', op, selection.value)
}

// --- Mana value (range) --------------------------------------------------------------

export interface RangeSelection {
  min: string
  max: string
}

export function getManaValue(query: string): RangeSelection {
  return readRange(query, MV_KEYS)
}

export function setManaValue(query: string, range: RangeSelection): string {
  return setRange(query, MV_KEYS, 'mv', range.min, range.max)
}

// --- Format legality -----------------------------------------------------------------

export function getFormat(query: string): string {
  const value = readFilter(query, FORMAT_KEYS)?.value.toLowerCase() ?? ''
  return FORMAT_OPTIONS.some((o) => o.value === value) ? value : ''
}

export function setFormat(query: string, value: string): string {
  return upsertFilter(query, FORMAT_KEYS, 'f', ':', value)
}

// --- Price (USD, range) --------------------------------------------------------------

export function getUsd(query: string): RangeSelection {
  return readRange(query, USD_KEYS)
}

export function setUsd(query: string, range: RangeSelection): string {
  return setRange(query, USD_KEYS, 'usd', range.min, range.max)
}

// --- Aggregate helpers ---------------------------------------------------------------

// The six key-groups the builder owns — the single source of truth for both the active
// count and Clear, so the two never disagree about what counts as builder-owned.
const BUILDER_KEY_GROUPS = [
  COLOR_KEYS,
  TYPE_KEYS,
  RARITY_KEYS,
  MV_KEYS,
  FORMAT_KEYS,
  USD_KEYS,
] as const

/**
 * How many builder-owned filter groups are present — drives the trigger's count badge and
 * whether Clear is enabled. Counts by token *presence* (not by whether a control can
 * reflect it), so a hand-typed value the pips/selects can't show (`c:golgari`, `c:3`,
 * `r:special`) still counts and can be cleared, matching clearBuilderFilters exactly.
 */
export function activeFilterCount(query: string): number {
  return BUILDER_KEY_GROUPS.filter((keys) => readFilter(query, keys) !== null).length
}

/** Strip every builder-owned filter, leaving free text and unrelated tokens intact. */
export function clearBuilderFilters(query: string): string {
  let next = query
  for (const keys of BUILDER_KEY_GROUPS) {
    next = removeFilter(next, keys)
  }
  return next
}
