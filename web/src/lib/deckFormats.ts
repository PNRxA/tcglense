// Suggested deck-format presets per game (issue #391). `deck.format` is a free-form
// `string | null` on the wire, so these are purely a UI affordance: the New-deck dialog
// binds them to a native <datalist> so the user can pick a common format OR type their own.
// Keyed by game slug so a future TCG can add its own list without touching the dialog.

export const DECK_FORMAT_PRESETS: Record<string, string[]> = {
  mtg: [
    'Commander',
    'Standard',
    'Pioneer',
    'Modern',
    'Legacy',
    'Vintage',
    'Pauper',
    'Pauper Commander',
    'Brawl',
    'Historic',
    'Alchemy',
    'Oathbreaker',
    'Limited',
    'Cube',
    'Casual',
  ],
}

/** The suggested format presets for a game, or an empty list when none are curated. */
export function formatPresetsFor(game: string): string[] {
  return DECK_FORMAT_PRESETS[game] ?? []
}
