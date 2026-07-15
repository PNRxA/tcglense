import type { DeckCardEntry, DeckSection } from '@/lib/api'

export interface DeckStatItem {
  key: string
  label: string
  count: number
  color?: string
}

export interface DeckCardOdds {
  name: string
  copies: number
}

export interface DeckStats {
  totalCopies: number
  uniqueCards: number
  landCopies: number
  averageManaValue: number | null
  manaCurve: DeckStatItem[]
  colors: DeckStatItem[]
  cardTypes: DeckStatItem[]
  cardOdds: DeckCardOdds[]
}

const COLORS: Record<string, { label: string; color: string }> = {
  W: { label: 'White', color: '#e5e7eb' },
  U: { label: 'Blue', color: '#3b82f6' },
  B: { label: 'Black', color: '#374151' },
  R: { label: 'Red', color: '#ef4444' },
  G: { label: 'Green', color: '#22c55e' },
  C: { label: 'Colorless', color: '#a1a1aa' },
}

const CARD_TYPES = [
  'Creature',
  'Artifact',
  'Enchantment',
  'Instant',
  'Sorcery',
  'Planeswalker',
  'Land',
  'Battle',
] as const

const NON_LIBRARY_SECTION_NAMES = new Set([
  'commander',
  'commanders',
  'sideboard',
  'sideboards',
  'maybeboard',
  'maybe board',
  'considering',
  'companion',
  'companions',
  'command zone',
  'signature spell',
  'signature spells',
])

/** Sections included in draw odds by default. Users can override the selection in the
 * analytics panel, while known command-zone and out-of-game boards start excluded. */
export function defaultDrawSectionIds(sections: Array<Pick<DeckSection, 'id' | 'name'>>): number[] {
  return sections
    .filter((section) => !NON_LIBRARY_SECTION_NAMES.has(section.name.trim().toLowerCase()))
    .map((section) => section.id)
}

function increment(map: Map<string, number>, key: string, copies: number) {
  map.set(key, (map.get(key) ?? 0) + copies)
}

function typeWords(typeLine: string | null): Set<string> {
  const front = typeLine?.split('//')[0] ?? ''
  return new Set(front.split(/[^A-Za-z]+/).filter(Boolean))
}

/** Calculate copy-weighted deck composition from the full deck-detail payload. */
export function calculateDeckStats(entries: DeckCardEntry[]): DeckStats {
  const uniqueIds = new Set<string>()
  const colors = new Map<string, number>()
  const types = new Map<string, number>()
  const odds = new Map<string, number>()
  const curve = Array.from({ length: 8 }, () => 0)
  let totalCopies = 0
  let landCopies = 0
  let manaValueCopies = 0
  let manaValueTotal = 0

  for (const entry of entries) {
    const copies = Math.max(0, entry.quantity + entry.foil_quantity)
    if (copies === 0) continue
    totalCopies += copies
    uniqueIds.add(entry.card.id)
    increment(odds, entry.card.name, copies)

    const identity = [...new Set(entry.card.color_identity)]
    if (identity.length === 0) increment(colors, 'C', copies)
    for (const color of identity) increment(colors, color, copies)

    const words = typeWords(entry.card.type_line)
    const matchedTypes = CARD_TYPES.filter((type) => words.has(type))
    if (matchedTypes.length === 0) increment(types, 'Other', copies)
    for (const type of matchedTypes) increment(types, type, copies)

    const isLand = words.has('Land')
    if (isLand) landCopies += copies
    const manaValue = entry.card.cmc
    if (!isLand && manaValue != null && Number.isFinite(manaValue)) {
      manaValueCopies += copies
      manaValueTotal += manaValue * copies
      const bucket = Math.min(7, Math.max(0, Math.floor(manaValue)))
      curve[bucket] = (curve[bucket] ?? 0) + copies
    }
  }

  return {
    totalCopies,
    uniqueCards: uniqueIds.size,
    landCopies,
    averageManaValue: manaValueCopies > 0 ? manaValueTotal / manaValueCopies : null,
    manaCurve: curve.map((count, index) => ({
      key: String(index),
      label: index === 7 ? '7+' : String(index),
      count,
    })),
    colors: Object.entries(COLORS)
      .map(([key, meta]) => ({ key, ...meta, count: colors.get(key) ?? 0 }))
      .filter((item) => item.count > 0),
    cardTypes: [...CARD_TYPES, 'Other']
      .map((type) => ({ key: type, label: type, count: types.get(type) ?? 0 }))
      .filter((item) => item.count > 0),
    cardOdds: [...odds.entries()]
      .map(([name, copies]) => ({ name, copies }))
      .sort((left, right) => right.copies - left.copies || left.name.localeCompare(right.name)),
  }
}

/** Hypergeometric probability of seeing at least one of `copies` in `cardsSeen` draws. */
export function drawProbability(deckSize: number, copies: number, cardsSeen: number): number {
  if (deckSize <= 0 || copies <= 0 || cardsSeen <= 0) return 0
  const boundedCopies = Math.min(deckSize, copies)
  const draws = Math.min(deckSize, cardsSeen)
  let miss = 1
  for (let draw = 0; draw < draws; draw += 1) {
    const missesLeft = deckSize - boundedCopies - draw
    if (missesLeft <= 0) return 1
    miss *= missesLeft / (deckSize - draw)
  }
  return 1 - miss
}
