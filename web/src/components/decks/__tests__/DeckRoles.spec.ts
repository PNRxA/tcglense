import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckRoles from '@/components/decks/DeckRoles.vue'
import type { DeckRole, DeckRoleGroup, DeckRoles as DeckRolesPayload } from '@/lib/api'

// The roles themselves are the server's grammar over rules text, so what's under test here
// is that the panel is *honest about what it counted*: it shows all eight buckets including
// the empty ones, it never presents them as a partition of the deck, the bars double as the
// card list's filter (which is the only reason the panel is interactive at all), and the
// details list the names a count was made of.

const ROLE_KEYS: DeckRole[] = [
  'ramp',
  'card_draw',
  'removal',
  'board_wipe',
  'counterspell',
  'tutor',
  'recursion',
  'protection',
]

const LABELS: Record<DeckRole, string> = {
  ramp: 'Ramp',
  card_draw: 'Card draw',
  removal: 'Removal',
  board_wipe: 'Board wipes',
  counterspell: 'Counterspells',
  tutor: 'Tutors',
  recursion: 'Recursion',
  protection: 'Protection',
}

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

function group(role: DeckRole, over: Partial<DeckRoleGroup> = {}): DeckRoleGroup {
  return {
    role,
    label: LABELS[role],
    description: `What ${LABELS[role]} counts.`,
    count: 0,
    copies: 0,
    cards: [],
    ...over,
  }
}

function makeRoles(over: Partial<DeckRolesPayload> = {}): DeckRolesPayload {
  return {
    roles: ROLE_KEYS.map((role) =>
      role === 'ramp'
        ? group(role, {
            count: 2,
            copies: 3,
            cards: [
              { card_id: 'sol', name: 'Sol Ring', quantity: 1 },
              { card_id: 'arcane', name: 'Arcane Signet', quantity: 2 },
            ],
          })
        : role === 'removal'
          ? group(role, {
              count: 1,
              copies: 1,
              cards: [{ card_id: 'swords', name: 'Swords to Plowshares', quantity: 1 }],
            })
          : group(role),
    ),
    card_roles: { sol: ['ramp'], arcane: ['ramp'], swords: ['removal'] },
    card_count: 99,
    unclassified_count: 60,
    ...over,
  }
}

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(DeckRoles, {
    props: { game: 'mtg', roles: makeRoles(), pending: false, failed: false, ...props },
  })
}

async function mountExpanded(props: Record<string, unknown> = {}) {
  const wrapper = mountPanel(props)
  await wrapper.get('button[aria-expanded]').trigger('click')
  return wrapper
}

describe('DeckRoles', () => {
  it('draws every role, the empty ones included', () => {
    const wrapper = mountPanel()
    const bars = wrapper.findAll('[role="img"]')

    expect(bars).toHaveLength(8)
    // A role the deck holds none of is a bar reading zero, not a bar that isn't there — an
    // absent "Board wipes" would have to be read as "not counted".
    expect(bars.map((bar) => bar.attributes('aria-label'))).toEqual([
      'Ramp: 3 copies',
      'Card draw: 0 copies',
      'Removal: 1 copies',
      'Board wipes: 0 copies',
      'Counterspells: 0 copies',
      'Tutors: 0 copies',
      'Recursion: 0 copies',
      'Protection: 0 copies',
    ])
  })

  it('says how much of the deck fills a role at all, rather than letting the bars be added up', () => {
    // The roles are not a partition — most creatures and every land fill none — so the panel
    // states the covered share instead of leaving 60 cards unaccounted for.
    expect(mountPanel().text()).toContain('39 of 99 cards fill at least one role')
  })

  it('selects a role on click and clears it on a second click', async () => {
    const wrapper = mountPanel()
    const ramp = wrapper.findAll('button[aria-pressed]')[0]!

    expect(ramp.attributes('aria-pressed')).toBe('false')
    await ramp.trigger('click')
    expect(wrapper.emitted('update:role')).toEqual([['ramp']])

    await wrapper.setProps({ role: 'ramp' })
    expect(wrapper.findAll('button[aria-pressed]')[0]!.attributes('aria-pressed')).toBe('true')

    await wrapper.findAll('button[aria-pressed]')[0]!.trigger('click')
    expect(wrapper.emitted('update:role')).toEqual([['ramp'], [null]])
  })

  it('keeps the evidence behind a disclosure', async () => {
    const wrapper = mountPanel()
    const toggle = wrapper.get('button[aria-expanded]')

    expect(toggle.attributes('aria-expanded')).toBe('false')
    // The body is unmounted while collapsed, so the button must not name a region that isn't
    // in the accessibility tree.
    expect(toggle.attributes('aria-controls')).toBeUndefined()
    expect(wrapper.text()).not.toContain('Sol Ring')

    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    expect(wrapper.find(`#${toggle.attributes('aria-controls')}`).exists()).toBe(true)
  })

  it('lists what each role counts and the cards it counted', async () => {
    const wrapper = await mountExpanded()
    const text = wrapper.text()

    for (const label of Object.values(LABELS)) expect(text).toContain(label)
    expect(text).toContain('What Ramp counts.')

    const links = wrapper.findAll('a')
    expect(links).toHaveLength(3)
    expect(links[0]!.text()).toBe('Sol Ring')
    // Copies only where there's more than one — a "×1" on every chip is noise.
    expect(links[1]!.text()).toBe('Arcane Signet ×2')
    expect(links[2]!.attributes('href')).toContain('swords')
  })

  it('says how many names a capped list left out', async () => {
    const roles = makeRoles()
    roles.roles[0]!.count = 9
    expect((await mountExpanded({ roles })).text()).toContain('…and 7 more')
  })

  it('never words a role count as a share of the deck', () => {
    const text = mountPanel().text()
    // The bars are copies of a role, not slices of a whole: no percentage, and nothing that
    // claims the eight buckets add up to the deck.
    expect(text).not.toMatch(/%/)
    expect(text).not.toMatch(/of the deck/i)
    // The blurb says a card can be in several, which is why they can't be added up.
    expect(text).toContain('a card can fill several roles')
  })

  it('shows a cue while the read is in flight, and no bars to reflow', () => {
    const wrapper = mountPanel({ roles: undefined, pending: true })

    expect(wrapper.text()).toContain('Reading the deck…')
    expect(wrapper.findAll('[role="img"]')).toHaveLength(0)
    // The disclosure is there (the card keeps its shape) but can't be opened onto nothing.
    expect(wrapper.get('button[aria-expanded]').attributes('disabled')).toBeDefined()
  })

  it('says so when the read failed, instead of drawing eight empty bars', () => {
    const wrapper = mountPanel({ roles: undefined, pending: false, failed: true })

    expect(wrapper.find('.text-destructive').exists()).toBe(true)
    expect(wrapper.findAll('[role="img"]')).toHaveLength(0)
    expect(wrapper.find('button[aria-expanded]').exists()).toBe(false)
  })

  it('renders nothing for a deck with no cards in it', () => {
    const wrapper = mountPanel({ roles: makeRoles({ card_count: 0, unclassified_count: 0 }) })
    expect(wrapper.text()).toBe('')
  })
})
