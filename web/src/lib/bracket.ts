// Commander bracket — the *presentation* half of the estimate.
//
// The estimate itself is the server's: `GET /api/decks/{game}/{deck_id}/bracket` returns the
// bracket, the reasons, the categories with their cards, and the whole 1–5 ladder with its
// labels and descriptions (see `api/src/handlers/decks/analysis/bracket.rs`). Unlike the
// format table in `lib/legality.ts`, the ladder is **not** mirrored here: the panel that
// draws it doesn't exist until the response lands, so a second copy would buy nothing and
// could drift.
//
// What lives here is what a JSON payload shouldn't carry — the colour a rung is drawn in.
// The ramp runs cool to warm with power (teal → rose); it is deliberately not a
// good/bad ramp, because no bracket is the "right" one to be in.

/** Chip background + text per bracket, for the headline badge and a decisive count. */
export const BRACKET_TONE: Record<number, string> = {
  1: 'bg-teal-500/15 text-teal-700 dark:text-teal-300',
  2: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
  3: 'bg-sky-500/15 text-sky-700 dark:text-sky-300',
  4: 'bg-amber-500/15 text-amber-700 dark:text-amber-300',
  5: 'bg-rose-500/15 text-rose-700 dark:text-rose-300',
}

/** Solid fill per bracket, for the ladder strip's segments. */
export const BRACKET_BAR: Record<number, string> = {
  1: 'bg-teal-500',
  2: 'bg-emerald-500',
  3: 'bg-sky-500',
  4: 'bg-amber-500',
  5: 'bg-rose-500',
}

/**
 * The rungs the estimate can actually land on. Brackets 1 (Exhibition) and 5 (cEDH) are
 * claims about intent — the server never estimates them — so the ladder marks them as the
 * player's own call rather than pretending they were ruled out.
 */
export const ESTIMATABLE_BRACKETS: readonly number[] = [2, 3, 4]

/** Tone for a bracket, falling back to the neutral muted chip for anything unexpected. */
export function bracketTone(bracket: number): string {
  return BRACKET_TONE[bracket] ?? 'bg-muted text-muted-foreground'
}

/** Ladder-segment fill for a bracket, falling back to the neutral track. */
export function bracketBar(bracket: number): string {
  return BRACKET_BAR[bracket] ?? 'bg-muted-foreground'
}
