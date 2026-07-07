import { describe, it, expect } from 'vitest'
import { computed, defineComponent, h, inject, nextTick, ref, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createRouter, createWebHistory, useRoute } from 'vue-router'
import type { Card } from '@/lib/api'
import { useCardBackLink } from '../useCardBackLink'

// A minimal card in Bloomburrow (`blb`), enough for the back-link's set fallback/label.
function makeCard(): Card {
  return {
    id: 'dummy-blb-0001',
    name: 'Dummy Card',
    set_code: 'blb',
    set_name: 'Bloomburrow',
    collector_number: '1',
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-08-02',
    mana_cost: '{1}',
    cmc: 1,
    type_line: 'Creature',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: '1.00', usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    faces: [],
  }
}

// The real named routes the back-link resolves against (a card can be opened from any
// list section — catalog, collection, wish list). Each renders a placeholder except the
// card route, which hosts the composable.
const routes = [
  { path: '/', name: 'home', component: { template: '<div />' } },
  { path: '/cards/:game', name: 'game', component: { template: '<div />' } },
  { path: '/cards/:game/cards', name: 'game-cards', component: { template: '<div />' } },
  { path: '/cards/:game/sets/:code', name: 'set', component: { template: '<div />' } },
  { path: '/collection/:game', name: 'game-collection', component: { template: '<div />' } },
  {
    path: '/collection/:game/cards',
    name: 'game-collection-cards',
    component: { template: '<div />' },
  },
  {
    path: '/collection/:game/sets/:code',
    name: 'game-collection-set',
    component: { template: '<div />' },
  },
  { path: '/wishlist/:game', name: 'game-wishlist', component: { template: '<div />' } },
  { path: '/wishlist/:game/cards', name: 'wishlist-cards', component: { template: '<div />' } },
  { path: '/wishlist/:game/sets/:code', name: 'wishlist-set', component: { template: '<div />' } },
  { path: '/sealed/:game/:id', name: 'sealed-product', component: { template: '<div />' } },
  { path: '/cards/:game/cards/:id', name: 'card', component: Host() },
]

// Card-route component: calls the composable and renders the resolved link so the test
// can read its `to`/label. The card object is injected so we don't need route props.
function Host() {
  return defineComponent({
    setup() {
      const route = useRoute()
      const game = computed(() => String(route.params.game))
      const card = inject<Ref<Card | undefined>>('card')!
      const link = useCardBackLink(game, card)
      return () => h('a', { href: link.value.to }, link.value.label)
    },
  })
}

// Navigate through `from` (the referrer, if any) then to the card page, mount the tree,
// and return the rendered back link. Mounting *after* the pushes means the card page's
// `history.state.back` is already the referrer, exactly as on a real "Open full page".
// A neutral `/` base is pushed first so the card page's `state.back` depends only on the
// last two entries — deterministic regardless of test order over the shared jsdom
// history (memory history doesn't track `state.back`, so this uses web history).
async function backLink(from?: string, card: Card | undefined = makeCard()) {
  const router = createRouter({ history: createWebHistory(), routes })
  await router.push('/')
  if (from) await router.push(from)
  await router.push('/cards/mtg/cards/dummy-blb-0001')
  await router.isReady()
  const wrapper = mount(
    { template: '<router-view />' },
    { global: { plugins: [router], provide: { card: ref(card) } } },
  )
  await nextTick()
  const a = wrapper.find('a')
  return { to: a.attributes('href'), label: a.text() }
}

describe('useCardBackLink', () => {
  it('falls back to the card set on a direct load (no referrer)', async () => {
    expect(await backLink()).toEqual({ to: '/cards/mtg/sets/blb', label: 'Bloomburrow' })
  })

  it('points at the catalog set the card was opened from', async () => {
    expect(await backLink('/cards/mtg/sets/blb?card=dummy-blb-0001')).toEqual({
      to: '/cards/mtg/sets/blb',
      label: 'Bloomburrow',
    })
  })

  it('points at the catalog all-cards list, preserving its search', async () => {
    expect(await backLink('/cards/mtg/cards?q=bolt&card=dummy-blb-0001')).toEqual({
      to: '/cards/mtg/cards?q=bolt',
      label: 'All cards',
    })
  })

  it('returns to a collection set the card was opened from', async () => {
    expect(await backLink('/collection/mtg/sets/blb?card=dummy-blb-0001')).toEqual({
      to: '/collection/mtg/sets/blb',
      label: 'Bloomburrow',
    })
  })

  it('returns to the all-owned collection list', async () => {
    expect(await backLink('/collection/mtg/cards?card=dummy-blb-0001')).toEqual({
      to: '/collection/mtg/cards',
      label: 'Collection',
    })
  })

  it('returns to a wish-list set the card was opened from', async () => {
    expect(await backLink('/wishlist/mtg/sets/blb?card=dummy-blb-0001')).toEqual({
      to: '/wishlist/mtg/sets/blb',
      label: 'Bloomburrow',
    })
  })

  it('returns to the wish-list all-cards list', async () => {
    expect(await backLink('/wishlist/mtg/cards?card=dummy-blb-0001')).toEqual({
      to: '/wishlist/mtg/cards',
      label: 'Wish list',
    })
  })

  it('returns to the sealed product the card was opened from (Cards in this product)', async () => {
    expect(await backLink('/sealed/mtg/900003?card=dummy-blb-0001')).toEqual({
      to: '/sealed/mtg/900003',
      label: 'Sealed product',
    })
  })

  it("ignores a referrer for a different game and falls back to the card's set", async () => {
    expect(await backLink('/collection/other/cards?card=dummy-blb-0001')).toEqual({
      to: '/cards/mtg/sets/blb',
      label: 'Bloomburrow',
    })
  })
})
