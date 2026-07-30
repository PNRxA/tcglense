import type { Component } from 'vue'
import { HeartPulse } from '@lucide/vue'

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
  ],
}

/** The tools available for a game (empty for a game with none — the index says so). */
export function toolsFor(game: string): ToolEntry[] {
  return TOOLS[game] ?? []
}

/** Games that have at least one tool, for the hub's tile list. */
export function gamesWithTools(): string[] {
  return Object.keys(TOOLS).filter((game) => (TOOLS[game]?.length ?? 0) > 0)
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
