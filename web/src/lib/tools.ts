import type { Component } from 'vue'
import { HeartPulse, Swords } from '@lucide/vue'

/**
 * The tools registry — the play aids that sit beside the catalog rather than inside it.
 *
 * A registry rather than hand-written pages, for the same reason `lib/deckFormats.ts` is one:
 * the `/tools` hub, each game's `/tools/{game}` index, the nav dropdown and the prefetch warm
 * list all need the same list, and a second tool (or a second game's tools) should be a data
 * entry rather than four edits.
 *
 * Keyed by game slug because a tool is only meaningful for the games it applies to — a life
 * counter belongs to MTG's rules, not to every catalog TCGLense might carry.
 */
export interface ToolEntry {
  /** URL slug under `/tools/{game}/`. */
  slug: string
  name: string
  /** One line: what it does for you, on the tile and in the nav. */
  blurb: string
  icon: Component
}

export const TOOLS: Readonly<Record<string, ToolEntry[]>> = {
  mtg: [
    {
      slug: 'life',
      name: 'Life counter',
      blurb:
        'Count life for a table of up to six, keep the gain/loss history, and build a ' +
        'win record for your decks.',
      icon: HeartPulse,
    },
    {
      slug: 'play',
      name: 'Play online',
      blurb:
        'Open a table, share the link, and play a manual game with friends — your decks, ' +
        'any precon, or a pasted list. Guests need no account.',
      icon: Swords,
    },
  ],
}

/** The tools available for a game (empty for a game with none — the index says so). */
export function toolsFor(game: string): ToolEntry[] {
  return TOOLS[game] ?? []
}

export function toolsPath(game: string): string {
  return `/tools/${encodeURIComponent(game)}`
}

export function toolPath(game: string, slug: string): string {
  return `${toolsPath(game)}/${slug}`
}

/** The life counter's own paths, spelled once so the views and nav can't drift. */
export const lifePath = (game: string): string => toolPath(game, 'life')
export const lifeSessionPath = (game: string, sessionId: number): string =>
  `${lifePath(game)}/${sessionId}`
export const lifeDeckStatsPath = (game: string): string => `${lifePath(game)}/decks`

/**
 * The play table's paths. The room code is the invite: it is the one thing a player is given
 * (pasted into the hub's join box, or followed as a link), so it is the route's own param
 * rather than a query string.
 */
export const playPath = (game: string): string => toolPath(game, 'play')
export const playRoomPath = (game: string, code: string): string =>
  `${playPath(game)}/${encodeURIComponent(code.toUpperCase())}`
