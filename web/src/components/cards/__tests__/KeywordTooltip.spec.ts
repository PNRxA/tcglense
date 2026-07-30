import { describe, it, expect, beforeEach, vi } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createRouter, createWebHistory } from 'vue-router'
import type { KeywordEntry } from '@/lib/api'
import ManaSymbols from '../ManaSymbols.vue'

const FLYING: KeywordEntry = {
  name: 'Flying',
  slug: 'flying',
  kind: 'ability',
  text: "This creature can't be blocked except by creatures with flying or reach.",
  parameterized: false,
  match_mode: 'anywhere',
}

const HASTE: KeywordEntry = {
  name: 'Haste',
  slug: 'haste',
  kind: 'ability',
  text: 'This creature can attack and {T} as soon as it comes under your control.',
  parameterized: false,
  match_mode: 'anywhere',
}

const UNEARTH: KeywordEntry = {
  name: 'Unearth',
  slug: 'unearth',
  kind: 'ability',
  text: 'Return this card from your graveyard to the battlefield.',
  parameterized: true,
  match_mode: 'anywhere',
}

const WARD: KeywordEntry = {
  name: 'Ward',
  slug: 'ward',
  kind: 'ability',
  text: 'Whenever this permanent becomes the target of a spell or ability an opponent controls, counter it unless that player pays the ward cost.',
  parameterized: true,
  match_mode: 'anywhere',
}

const VIGILANCE: KeywordEntry = {
  name: 'Vigilance',
  slug: 'vigilance',
  kind: 'ability',
  text: "Attacking doesn't cause this creature to tap.",
  parameterized: false,
  match_mode: 'anywhere',
}

function router() {
  return createRouter({
    history: createWebHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/keywords/:game/:slug', name: 'keyword', component: { template: '<div />' } },
    ],
  })
}

/** Mount `ManaSymbols` with the glossary already in the query cache, so the keyword
 * markers render synchronously (no network in tests). */
async function mountText(text: string, keywords = true, cardName?: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(['keywords', 'mtg'], [FLYING, VIGILANCE, HASTE, UNEARTH, WARD])
  const instance = router()
  await instance.push('/')
  await instance.isReady()
  return mount(ManaSymbols, {
    props: { text, keywords, game: 'mtg', cardName },
    global: { plugins: [[VueQueryPlugin, { queryClient }], instance] },
  })
}

/** Stub `matchMedia` to report whether the device can hover, so a test can pick which
 * trigger branch KeywordTooltip renders. */
function stubPointer(canHover: boolean) {
  vi.stubGlobal('matchMedia', () => ({
    matches: canHover,
    addEventListener: () => {},
    removeEventListener: () => {},
  }))
}

describe('ManaSymbols with keywords', () => {
  beforeEach(() => {
    // jsdom has no matchMedia; the component defaults to the hover branch without it.
    // Each case stubs the answer it needs.
    vi.stubGlobal('matchMedia', undefined)
  })

  it('marks a keyword without altering the rendered text', async () => {
    const wrapper = await mountText('Flying, vigilance')
    // Text is byte-identical to the input: the marker wraps it, never rewrites it.
    expect(wrapper.element.textContent).toBe('Flying, vigilance')
    expect(wrapper.findAll('a')).toHaveLength(2)
  })

  it('keeps the card text spelling, not the glossary name', async () => {
    const wrapper = await mountText('Target creature gains vigilance.')
    expect(wrapper.get('a').text()).toBe('vigilance')
  })

  it('links each marker to that keyword page', async () => {
    const wrapper = await mountText('Flying')
    expect(wrapper.get('a').attributes('href')).toBe('/keywords/mtg/flying')
  })

  it('renders mana symbols and keyword markers together', async () => {
    const wrapper = await mountText('{T}: Target creature gains flying until end of turn.')
    expect(wrapper.findAll('i')).toHaveLength(1)
    expect(wrapper.findAll('a')).toHaveLength(1)
    expect(wrapper.element.textContent).toBe(': Target creature gains flying until end of turn.')
  })

  it('marks nothing without the keywords prop, and makes no glossary subscription', async () => {
    // Mounted with no QueryClient at all — proof the mana-cost call sites (a deck list
    // renders ~100 of them) never touch vue-query.
    const wrapper = mount(ManaSymbols, { props: { text: 'Flying, vigilance' } })
    expect(wrapper.findAll('a')).toHaveLength(0)
    expect(wrapper.element.textContent).toBe('Flying, vigilance')
  })

  it("does not mark the card's own name", async () => {
    const wrapper = await mountText('Flying Men can block.', true, 'Flying Men')
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  // Keywords are matched over the whole string BEFORE the mana-symbol split. Doing it
  // the other way round fragments the text at every `{…}`, which quietly defeats two of
  // the matcher's guards — these are the cases that catches.
  it('keeps the reminder-text guard when the reminder contains a mana symbol', async () => {
    // Dregscape Zombie's shape. Split symbols-first, the "(" and the "haste" inside it
    // land in different runs once `{B}` is cut out, so the guard sees no parenthesis and
    // marks the haste — plus a second Unearth, both inside the reminder.
    const wrapper = await mountText(
      'Unearth {B} ({B}: Return this card from your graveyard to the battlefield. ' +
        'It gains haste and flying. Unearth only as a sorcery.)',
    )
    // Only the real keyword, ahead of the reminder text.
    expect(wrapper.findAll('a').map((a) => a.text())).toEqual(['Unearth'])
  })

  it('marks a keyword once per block even across a mana symbol', async () => {
    const wrapper = await mountText(
      'Flying, ward {2}\nWhenever this creature attacks, target creature gains flying.',
    )
    // The second "flying" is a repeat, so it stays unmarked even though the `{2}` sits
    // between the two mentions.
    expect(wrapper.findAll('a').map((a) => a.text())).toEqual(['Flying', 'ward'])
  })

  it('uses a tap-friendly button trigger when the device cannot hover', async () => {
    stubPointer(false)
    const wrapper = await mountText('Flying')
    // The popover branch: a real button, so a tap opens it and a keyboard reaches it.
    expect(wrapper.findAll('button')).toHaveLength(1)
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  it('uses a hoverable link trigger on a fine pointer', async () => {
    stubPointer(true)
    const wrapper = await mountText('Flying')
    expect(wrapper.findAll('a')).toHaveLength(1)
    expect(wrapper.findAll('button')).toHaveLength(0)
  })
})
