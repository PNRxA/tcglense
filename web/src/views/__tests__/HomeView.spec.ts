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
    ],
  })
  await router.push('/')
  await router.isReady()

  return mount(HomeView, { global: { plugins: [pinia, router] } })
}

function scannerRow(wrapper: VueWrapper) {
  const heading = wrapper
    .findAll('h2')
    .find((candidate) => candidate.text() === 'Turn a stack of Magic cards into your collection')
  if (!heading) throw new Error('missing scanner heading')

  const row = heading.element.parentElement?.parentElement
  if (!row) throw new Error('missing scanner feature row')
  return row
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
