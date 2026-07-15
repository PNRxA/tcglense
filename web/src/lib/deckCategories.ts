import type { Card, DeckSection } from '@/lib/api'

/**
 * Preset type filing used when a deck add targets "Automatic". Keep this order in
 * sync with `api/src/deck_import/categorize.rs`: a multi-type permanent is filed in
 * the bucket that is most useful while building (Artifact Creature -> Creatures), and
 * only the front face of a modal card determines its section.
 */
export function presetDeckSection(card: Pick<Card, 'type_line'>): string | null {
  const front = card.type_line?.split('//')[0] ?? ''
  const words = front.split(/[^A-Za-z]+/).filter(Boolean)
  const hasType = (wanted: string) => words.some((word) => word.toLowerCase() === wanted)

  if (hasType('land')) return 'Lands'
  if (hasType('creature')) return 'Creatures'
  if (hasType('planeswalker')) return 'Planeswalkers'
  if (hasType('instant')) return 'Instants'
  if (hasType('sorcery')) return 'Sorceries'
  if (hasType('enchantment')) return 'Enchantments'
  if (hasType('artifact')) return 'Artifacts'
  return null
}

/** Resolve the automatic bucket against this deck's actual sections, falling back to
 * the first section for imported/custom decks that do not have the preset bucket. */
export function automaticDeckSection(
  card: Pick<Card, 'type_line'>,
  sections: DeckSection[],
): DeckSection | undefined {
  const preset = presetDeckSection(card)
  const matched = preset
    ? sections.find((section) => section.name.toLowerCase() === preset.toLowerCase())
    : undefined
  return matched ?? sections[0]
}
