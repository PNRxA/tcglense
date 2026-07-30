import { describe, expect, it } from 'vitest'
import type { Game } from '@/lib/api'
import {
  NAV,
  allNavItems,
  itemWarmTargets,
  publicNavItems,
  resolveItem,
  type NavItem,
} from '@/lib/nav'
import router from '@/router'

// The nav registry is the app's IA written down once, so what's worth testing is the two ways
// it can lie: a destination that doesn't exist, and a destination that quietly disappeared.
// Both are checked against something outside the table — the REAL router, and an explicit list
// of what the hand-written navs carried before the consolidation — because a snapshot of the
// table against itself would rubber-stamp either mistake.

const game = (id: string, name: string): Game => ({
  id,
  name,
  publisher: 'Test Publisher',
  data_source: 'Test Source',
})

// Two games: MTG (which has tools) and one that has none, so the per-game expansion is
// exercised in both directions.
const GAMES: readonly Game[] = [game('mtg', 'Magic: The Gathering'), game('pkm', 'Pokémon')]

function itemById(id: string): NavItem {
  const item = allNavItems().find((entry) => entry.id === id)
  if (!item) throw new Error(`no nav item with id "${id}"`)
  return item
}

const warmTargets = (id: string): string[] => itemWarmTargets(resolveItem(itemById(id), GAMES))

describe('NAV destinations', () => {
  it('points every landing and per-game link at a real route', () => {
    for (const item of allNavItems()) {
      for (const to of itemWarmTargets(resolveItem(item, GAMES))) {
        // The catch-all is `name: 'not-found'` (router/index.ts) — a typo'd path resolves to
        // it rather than throwing, so that is what this has to assert against.
        expect(router.resolve(to).name, `${item.id} → ${to}`).not.toBe('not-found')
      }
    }
  })

  // Deliberately an explicit list, not a snapshot: a snapshot of the table against itself
  // would happily record a deletion as the new truth, which is the exact failure mode
  // (Decks, Alerts and the Keyword glossary each reaching some navs and not others) the
  // registry exists to end.
  const PRE_REGISTRY_LANDINGS = [
    '/cards',
    '/sealed',
    '/keywords',
    '/collection',
    '/decks',
    '/wishlist',
    '/scan',
    '/tools',
    '/docs',
  ]

  it('still carries every landing the hand-written navs did', () => {
    const landings = allNavItems().map((item) => item.landing)
    for (const landing of PRE_REGISTRY_LANDINGS) expect(landings).toContain(landing)
  })

  it('leaves /alerts out of the registry', () => {
    // The one deliberate removal in the consolidation (nav-decision §3): /alerts is
    // account-scoped notification *settings*, so it lives in UserMenu, which renders at every
    // width. The registry is primary IA, not account chrome.
    expect(allNavItems().map((item) => item.landing)).not.toContain('/alerts')
  })

  it('keeps `auth` in step with the router', () => {
    // Compared as two id-tagged lists rather than per-item assertions, so a mismatch names
    // the offending item in the diff (expect's message argument is linted away here).
    const declared = allNavItems().map((item) => `${item.id}: ${item.auth ?? false}`)
    const routed = allNavItems().map(
      (item) => `${item.id}: ${router.resolve(item.landing).meta.requiresAuth === true}`,
    )
    expect(declared).toEqual(routed)
  })

  it('gives every item a unique id', () => {
    const ids = allNavItems().map((item) => item.id)
    expect(new Set(ids).size).toBe(ids.length)
  })
})

describe('resolveItem', () => {
  it('expands a per-game item over every game', () => {
    expect(warmTargets('cards')).toEqual(['/cards', '/cards/mtg', '/cards/pkm'])
  })

  it('drops games with no tools but still warms the hub', () => {
    expect(warmTargets('tools')).toEqual(['/tools', '/tools/mtg/life', '/tools/mtg'])
  })

  it('trails each game with an index link to its own tools page', () => {
    const resolved = resolveItem(itemById('tools'), GAMES)
    expect(resolved.perGame).toHaveLength(1)
    expect(resolved.perGame[0]?.game.id).toBe('mtg')
    expect(resolved.perGame[0]?.links.map((link) => [link.label, link.kind])).toEqual([
      ['Life counter', undefined],
      ['All Magic: The Gathering tools', 'index'],
    ])
  })

  it('leaves an item with no expansion as its landing alone', () => {
    expect(warmTargets('scan')).toEqual(['/scan'])
    expect(warmTargets('docs')).toEqual(['/docs'])
  })
})

describe('publicNavItems', () => {
  it('is account-free and excludes the bare API link', () => {
    const landings = publicNavItems().map((item) => item.landing)
    expect(publicNavItems().every((item) => !item.auth)).toBe(true)
    expect(landings).not.toContain('/scan')
    // /docs is a `kind: 'link'` root, so it stays in the footer's Project column.
    expect(landings).not.toContain('/docs')
    expect(landings).toContain('/decks')
  })
})

describe('the consolidation', () => {
  it('puts the catalog and library groups under one Browse root', () => {
    const browse = NAV.find((root) => root.kind === 'menu' && root.id === 'browse')
    if (!browse || browse.kind !== 'menu') throw new Error('no `browse` menu root')
    expect(browse.label).toBe('Browse')
    expect(browse.groups.map((group) => group.id)).toEqual(['catalog', 'library'])
    expect(browse.groups.map((group) => group.label)).toEqual(['Catalog', 'Your library'])
    expect(browse.groups[0]?.items.map((item) => item.id)).toEqual(['cards', 'sealed', 'keywords'])
    expect(browse.groups[1]?.items.map((item) => item.id)).toEqual([
      'collection',
      'decks',
      'wishlist',
      'scan',
    ])
  })

  it('keeps Tools its own root and API a bare link', () => {
    expect(NAV.map((root) => (root.kind === 'menu' ? root.id : root.item.id))).toEqual([
      'browse',
      'tools',
      'docs',
    ])
  })
})
