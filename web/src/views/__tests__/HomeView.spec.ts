import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import HomeView from '../HomeView.vue'

vi.mock('@/composables/useCatalog', async () => {
  const { ref } = await import('vue')
  return {
    useGamesQuery: () => ({ data: ref({ data: [] }) }),
  }
})

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

// The hero's universal search box has its own spec (and its own queries); here it is a
// stub that records the games it was handed, so the view's job — mounting it in the hero
// with the registry — is what's asserted.
const SearchBoxStub = {
  name: 'UniversalSearchBox',
  props: ['games'],
  template: '<div data-test="universal-search" :data-games="games.length" />',
}

type AuthState = 'authenticated' | 'guest' | 'unresolved'

async function mountHome(authState: AuthState) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()

  if (authState === 'authenticated') {
    auth.accessToken = 'token'
    auth.sessionResolved = true
  } else if (authState === 'guest') {
    auth.sessionResolved = true
  }

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: HomeView },
      { path: '/scan', component: { template: '<div />' } },
      { path: '/register', component: { template: '<div />' } },
      { path: '/cards', component: { template: '<div />' } },
      { path: '/collection', component: { template: '<div />' } },
      { path: '/wishlist', component: { template: '<div />' } },
      { path: '/alerts', component: { template: '<div />' } },
      { path: '/sealed', component: { template: '<div />' } },
      { path: '/docs', component: { template: '<div />' } },
      { path: '/login', component: { template: '<div />' } },
      { path: '/decks', component: { template: '<div />' } },
      { path: '/precons', component: { template: '<div />' } },
      { path: '/tools', component: { template: '<div />' } },
      { path: '/releases', component: { template: '<div />' } },
      { path: '/keywords', component: { template: '<div />' } },
    ],
  })
  await router.push('/')
  await router.isReady()

  return mount(HomeView, {
    global: { plugins: [pinia, router], stubs: { UniversalSearchBox: SearchBoxStub } },
  })
}

/** A feature demo row, found by its heading: the grid that pairs the text column with the
 *  decorative panel. */
function featureRow(wrapper: VueWrapper, headingText: string) {
  const heading = wrapper.findAll('h2').find((candidate) => candidate.text() === headingText)
  if (!heading) throw new Error(`missing feature heading: ${headingText}`)

  const row = heading.element.parentElement?.parentElement
  if (!row) throw new Error(`missing feature row: ${headingText}`)
  return row
}

function scannerRow(wrapper: VueWrapper) {
  return featureRow(wrapper, 'Turn a stack of Magic cards into your collection')
}

function rowLink(row: Element, text: string) {
  return Array.from(row.querySelectorAll('a')).find(
    (candidate) => candidate.textContent?.trim() === text,
  )
}

/** The decorative panel of a feature row: the one direct child hidden from assistive tech. */
function rowDemo(row: Element) {
  const demo = row.querySelector(':scope > div[aria-hidden="true"]')
  if (!demo) throw new Error('missing demo panel')
  return demo
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('HomeView card scanner feature', () => {
  it('links authenticated users to the scanner and keeps its preview decorative', async () => {
    const getUserMedia = vi.fn<() => Promise<MediaStream>>()
    vi.stubGlobal('navigator', {
      ...navigator,
      mediaDevices: { getUserMedia },
    })
    const wrapper = await mountHome('authenticated')
    const row = scannerRow(wrapper)

    expect(row.textContent).toContain('confirm the exact printing')
    expect(row.textContent).toContain('Photos are processed locally and never uploaded')
    const link = Array.from(row.querySelectorAll('a')).find(
      (candidate) => candidate.textContent?.trim() === 'Scan Magic cards',
    )
    expect(link?.getAttribute('href')).toBe('/scan')

    const demo = row.querySelector(':scope > div[aria-hidden="true"]')
    expect(demo?.textContent).toContain('Artwork matched')
    expect(
      demo?.querySelector('a, button, input, select, textarea, [tabindex], [aria-live]'),
    ).toBeNull()
    expect(getUserMedia).not.toHaveBeenCalled()

    wrapper.unmount()
  })

  it('offers resolved guests an account with a safe scanner redirect', async () => {
    const wrapper = await mountHome('guest')
    const row = scannerRow(wrapper)
    const links = Array.from(row.querySelectorAll('a'))
    const registerLink = links.find(
      (candidate) => candidate.textContent?.trim() === 'Create a free account to scan',
    )

    expect(registerLink?.getAttribute('href')).toBe('/register?redirect=/scan')
    expect(links.some((candidate) => candidate.getAttribute('href') === '/scan')).toBe(false)

    wrapper.unmount()
  })

  it('reserves the scanner CTA while the session is unresolved', async () => {
    const wrapper = await mountHome('unresolved')
    const row = scannerRow(wrapper)

    expect(row.textContent).not.toContain('Scan Magic cards')
    expect(row.textContent).not.toContain('Create a free account to scan')
    expect(row.querySelector('[data-slot="skeleton"]')).not.toBeNull()

    wrapper.unmount()
  })
})

describe('HomeView universal search', () => {
  it('gives the search box its own section, front and centre above the hero, for every visitor', async () => {
    for (const state of ['authenticated', 'guest', 'unresolved'] as const) {
      const wrapper = await mountHome(state)
      // The first section on the page is the search, labelled by its own heading.
      const first = wrapper.find('section')
      expect(first.attributes('aria-labelledby')).toBe('home-search-heading')
      expect(first.find('#home-search-heading').text()).toBe('Search the whole catalog')
      const box = first.find('[data-test="universal-search"]')
      expect(box.exists(), `${state}: the box is in the search section`).toBe(true)
      // It is handed the (here empty) games registry, and it precedes the hero's h1.
      expect(box.attributes('data-games')).toBe('0')
      const heading = wrapper.find('h1').element
      expect(
        heading.compareDocumentPosition(box.element) & Node.DOCUMENT_POSITION_PRECEDING,
        `${state}: the box comes before the hero heading`,
      ).toBeTruthy()
      wrapper.unmount()
    }
  })
})

// The rows added for the deck, precon, booster-odds and life-counter releases: each links
// to where the feature lives (auth-branched where the page is per-user), and each demo
// panel stays a decorative mock — no interactive element a screen reader would find in a
// region hidden from it, and no data fetch behind it.
describe('HomeView recent feature rows', () => {
  const DEMO_HEADINGS = [
    'Build a deck, then let the numbers talk',
    'Every published precon, ready to copy',
    'Know what a pack is worth before you open it',
    'A life counter that remembers the game',
  ]

  it('leads with decks and links a signed-in user to their decks', async () => {
    const wrapper = await mountHome('authenticated')
    const headings = wrapper.findAll('h2').map((candidate) => candidate.text())
    // The decks row is the first feature row: the first h2 after the search section's (the
    // hero carries the page's only h1).
    expect(headings[headings.indexOf('Search the whole catalog') + 1]).toBe(
      'Build a deck, then let the numbers talk',
    )

    const decks = featureRow(wrapper, 'Build a deck, then let the numbers talk')
    expect(decks.textContent).toContain('combos from Commander Spellbook')
    expect(rowLink(decks, 'Open your decks')?.getAttribute('href')).toBe('/decks')
    expect(rowDemo(decks).textContent).toContain('Legal in Commander')

    wrapper.unmount()
  })

  it('offers a guest a first deck, and the public rows to everyone', async () => {
    const wrapper = await mountHome('guest')
    const decks = featureRow(wrapper, 'Build a deck, then let the numbers talk')
    expect(rowLink(decks, 'Start a deck')?.getAttribute('href')).toBe('/decks')

    const precons = featureRow(wrapper, 'Every published precon, ready to copy')
    expect(rowLink(precons, 'Browse preconstructed decks')?.getAttribute('href')).toBe('/precons')

    const odds = featureRow(wrapper, 'Know what a pack is worth before you open it')
    expect(rowLink(odds, 'Browse sealed products')?.getAttribute('href')).toBe('/sealed')

    const life = featureRow(wrapper, 'A life counter that remembers the game')
    expect(rowLink(life, 'Open the play aids')?.getAttribute('href')).toBe('/tools')

    wrapper.unmount()
  })

  it('reserves the decks CTA while the session is unresolved', async () => {
    const wrapper = await mountHome('unresolved')
    const decks = featureRow(wrapper, 'Build a deck, then let the numbers talk')

    expect(decks.textContent).not.toContain('Open your decks')
    expect(decks.textContent).not.toContain('Start a deck')
    expect(decks.querySelector('[data-slot="skeleton"]')).not.toBeNull()

    wrapper.unmount()
  })

  it('keeps every new demo panel decorative', async () => {
    const wrapper = await mountHome('guest')
    for (const heading of DEMO_HEADINGS) {
      const demo = rowDemo(featureRow(wrapper, heading))
      expect(
        demo.querySelector('a, button, input, select, textarea, [tabindex], [aria-live]'),
        `${heading}: nothing focusable inside the hidden panel`,
      ).toBeNull()
    }
    wrapper.unmount()
  })

  it('words the booster-odds mock as an expectation, never as contents', async () => {
    const wrapper = await mountHome('guest')
    const demo = rowDemo(featureRow(wrapper, 'Know what a pack is worth before you open it'))
    const text = demo.textContent ?? ''
    expect(text).toContain('on average')
    expect(text).toContain("of today's price")
    expect(text).not.toMatch(/contains|guaranteed|profit/i)
    wrapper.unmount()
  })

  it('links the compact grid to the release calendar, the glossary and the shopping list', async () => {
    const wrapper = await mountHome('guest')
    const heading = wrapper
      .findAll('h2')
      .find((candidate) => candidate.text() === "Everything else that's live today")
    const grid = heading?.element.parentElement
    if (!grid) throw new Error('missing the compact grid section')
    const hrefs = Array.from(grid.querySelectorAll('a')).map((a) => a.getAttribute('href'))
    expect(hrefs).toContain('/releases')
    expect(hrefs).toContain('/keywords')
    expect(hrefs).toContain('/wishlist')
    wrapper.unmount()
  })
})
