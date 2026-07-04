// How a "ghost" card (one the viewer doesn't own / hasn't wishlisted, shown dimmed in the
// collection / wish-list browse views' show-ghosts mode) is desaturated. Both styles dim
// the card so owned cards stand out; they differ only in colour:
//   - 'grayscale' — dim + drain the colour (the original, default treatment)
//   - 'color'     — dim only, keeping the artwork's colour (issue #213)
// A personal display preference (like the card size / theme), so it lives in localStorage
// and applies everywhere ghosts render, not in the per-list URL state.
export type GhostStyle = 'grayscale' | 'color'

export const DEFAULT_GHOST_STYLE: GhostStyle = 'grayscale'

const GHOST_STYLES: readonly GhostStyle[] = ['grayscale', 'color']

// A type guard tolerant of the raw localStorage value (a string or `null`), so a stale or
// hand-edited key falls back to the default rather than yielding an invalid style.
export function isGhostStyle(value: unknown): value is GhostStyle {
  return typeof value === 'string' && (GHOST_STYLES as readonly string[]).includes(value)
}
