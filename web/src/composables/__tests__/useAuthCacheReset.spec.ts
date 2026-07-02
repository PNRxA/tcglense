import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { clearAuthedQueries, useAuthCacheReset } from '@/composables/useAuthCacheReset'
import { useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'
import type { User } from '@/lib/api'

const USER_A: User = {
  id: 1,
  email: 'a@x.test',
  display_name: 'A',
  created_at: '2026-01-01T00:00:00Z',
}
const USER_B: User = {
  id: 2,
  email: 'b@x.test',
  display_name: 'B',
  created_at: '2026-01-02T00:00:00Z',
}

// Seed a per-user (authed) query and a public one so we can assert the reset drops only
// the former. fetchQuery attaches `meta` to the cached query exactly like useAuthedQuery.
async function seedCache(qc: QueryClient) {
  await qc.fetchQuery({
    queryKey: ['collection', 'mtg'],
    queryFn: () => 'private-collection',
    meta: { authed: true },
  })
  await qc.fetchQuery({ queryKey: ['games'], queryFn: () => 'public-games' })
}

const mounted: Array<ReturnType<typeof mount>> = []

describe('clearAuthedQueries', () => {
  it('removes only queries tagged meta.authed, leaving public catalog data', async () => {
    const qc = new QueryClient()
    await seedCache(qc)
    expect(qc.getQueryData(['collection', 'mtg'])).toBe('private-collection')

    clearAuthedQueries(qc)

    expect(qc.getQueryData(['collection', 'mtg'])).toBeUndefined()
    expect(qc.getQueryData(['games'])).toBe('public-games')
  })

  it('drops setQueryData-seeded per-user entries that carry no meta', () => {
    const qc = new QueryClient()
    // The per-card entry mutations write straight into the cache; in the quick-add flow no
    // entry-query observer is mounted, so this cache entry is built with meta === undefined
    // — it must still be dropped, matched by its key prefix rather than the meta tag.
    qc.setQueryData(['collection-entry', 'mtg', 'card-x'], { quantity: 1, foil_quantity: 0 })
    qc.setQueryData(['wishlist-entry', 'mtg', 'card-y'], { quantity: 2, foil_quantity: 0 })
    qc.setQueryData(['card', 'mtg', 'card-x'], 'public-card')
    expect(qc.getQueryState(['collection-entry', 'mtg', 'card-x'])?.data).toBeDefined()

    clearAuthedQueries(qc)

    expect(qc.getQueryData(['collection-entry', 'mtg', 'card-x'])).toBeUndefined()
    expect(qc.getQueryData(['wishlist-entry', 'mtg', 'card-y'])).toBeUndefined()
    // A public key that isn't per-user must survive.
    expect(qc.getQueryData(['card', 'mtg', 'card-x'])).toBe('public-card')
  })
})

describe('useAuthCacheReset', () => {
  let qc: QueryClient
  let auth: ReturnType<typeof useAuthStore>

  beforeEach(() => {
    setActivePinia(createPinia())
    auth = useAuthStore()
    qc = new QueryClient()
    const host = defineComponent({ setup: () => useAuthCacheReset(), render: () => null })
    mounted.push(mount(host, { global: { plugins: [[VueQueryPlugin, { queryClient: qc }]] } }))
  })

  afterEach(() => {
    mounted.forEach((wrapper) => wrapper.unmount())
    mounted.length = 0
  })

  it('drops per-user data on logout (user → null)', async () => {
    auth.user = USER_A
    await flushPromises()
    await seedCache(qc)

    auth.user = null
    await flushPromises()

    expect(qc.getQueryData(['collection', 'mtg'])).toBeUndefined()
    expect(qc.getQueryData(['games'])).toBe('public-games')
  })

  it('drops the previous account on an account switch (A → B)', async () => {
    auth.user = USER_A
    await flushPromises()
    await seedCache(qc)
    expect(qc.getQueryData(['collection', 'mtg'])).toBe('private-collection')

    auth.user = USER_B
    await flushPromises()

    expect(qc.getQueryData(['collection', 'mtg'])).toBeUndefined()
    expect(qc.getQueryData(['games'])).toBe('public-games')
  })

  it('does not clear on a token-only refresh (same user id)', async () => {
    auth.user = USER_A
    await flushPromises()
    await seedCache(qc)

    // A silent refresh rotates the access token but keeps the same identity; a new user
    // object with the same id must not wipe the cache.
    auth.accessToken = 'rotated-token'
    auth.user = { ...USER_A }
    await flushPromises()

    expect(qc.getQueryData(['collection', 'mtg'])).toBe('private-collection')
  })
})

// The reset only works because `useAuthedQuery` tags every per-user read with
// `meta.authed` — the load-bearing half of the #177 fix. The tests above hand-stamp that
// meta, so they'd stay green if the wrapper stopped tagging. This drives the *real*
// wrapper end-to-end so a regression of the tag (a rename/removal) fails the suite.
describe('useAuthedQuery tags its cached query so clearAuthedQueries catches it', () => {
  it('a real authed query is tagged meta.authed and dropped by the reset', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    // Signed in, so `useAuthedQuery`'s `enabled` gate opens and it actually fetches.
    useAuthStore().accessToken = 'test-token'
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })

    // Built as an intermediate variable (not an inline literal) to sidestep the
    // excess-property check the useAuthed* wrappers trip on — see useCollection.ts.
    const options = {
      queryKey: ['collection', 'mtg'],
      queryFn: (_token: string) => Promise.resolve('private-collection'),
    }
    const host = defineComponent({
      setup: () => useAuthedQuery<string>(options),
      render: () => null,
    })
    mounted.push(
      mount(host, { global: { plugins: [pinia, [VueQueryPlugin, { queryClient: qc }]] } }),
    )

    await flushPromises()
    expect(qc.getQueryData(['collection', 'mtg'])).toBe('private-collection')

    clearAuthedQueries(qc)
    expect(qc.getQueryData(['collection', 'mtg'])).toBeUndefined()
  })
})
