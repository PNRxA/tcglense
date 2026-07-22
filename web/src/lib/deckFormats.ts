// Deck-format select options per game (issues #391/#557). `deck.format` is still a
// free-form `string | null` on the wire: picking an option stores its display label,
// and the format field's "Custom…" escape hatch still lets the user type anything.
// MTG's legality-tracked formats come from lib/legality's MTG_FORMATS, so the select,
// the card-page legality panel, and the deck breach check can never drift apart.
// Keyed by game slug so a future TCG can add its own list without touching the field.

import { MTG_FORMATS, type MtgFormat } from '@/lib/legality'

export interface DeckFormatGroup {
  label: string
  options: string[]
}

const MTG_GROUP_ORDER: MtgFormat['group'][] = ['Constructed', 'Commander', 'Arena', 'Other']

export const DECK_FORMAT_GROUPS: Record<string, DeckFormatGroup[]> = {
  mtg: [
    ...MTG_GROUP_ORDER.map((label) => ({
      label: label as string,
      options: MTG_FORMATS.filter((format) => format.group === label).map((format) => format.label),
    })),
    // Common ways to play that aren't sanctioned constructed formats — no legality
    // checking applies (normalizeFormatKey maps them to null).
    { label: 'Casual', options: ['Limited', 'Cube', 'Casual'] },
  ],
}

/** The grouped format options for a game, or an empty list when none are curated. */
export function formatGroupsFor(game: string): DeckFormatGroup[] {
  return DECK_FORMAT_GROUPS[game] ?? []
}
