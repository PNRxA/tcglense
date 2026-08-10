import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    setAccent: vi.fn<typeof actual.setAccent>(),
  }
})

import { setAccent, type User } from '@/lib/api'
import { useSetAccentMutation } from '@/composables/useAccent'
import type { Accent } from '@/lib/accent'
import { useAccentStore } from '@/stores/accent'
import { useAuthStore } from '@/stores/auth'

function user(accent: string): User {
  return {
    id: 1,
    email: 'accent@example.com',
    created_at: '2026-08-01T00:00:00Z',
    username: null,
    discriminator: null,
    handle: null,
    currency: 'USD',
    accent,
  }
}

// The settings picker's wiring in miniature: the mutation's returned User must replace
// the auth store's user, which is what repaints <html>'s data-accent — the same
// replace-don't-refetch loop useCurrency.spec.ts pins for the currency preference.
const Harness = defineComponent({
  setup() {
    const accentStore = useAccentStore()
    const mutation = useSetAccentMutation()
    const choose = (accent: Accent) => mutation.mutateAsync({ accent })
    return { accentStore, choose }
  },
  template: `
    <div data-accent-value>{{ accentStore.accent }}</div>
    <button type="button" @click="choose('teal')">teal</button>
  `,
})

function mountHarness(accent: string) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()
  auth.accessToken = 'access-token'
  auth.user = user(accent)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Harness, {
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
  return { wrapper, auth }
}

describe('useSetAccentMutation', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    delete document.documentElement.dataset.accent
  })

  it('repaints data-accent after the mutation replaces the cached user', async () => {
    vi.mocked(setAccent).mockResolvedValue(user('teal'))
    const { wrapper, auth } = mountHarness('pink')
    expect(document.documentElement.dataset.accent).toBe('pink')

    await wrapper.get('button').trigger('click')
    await flushPromises()

    expect(setAccent).toHaveBeenCalledWith('access-token', 'teal')
    expect(auth.user?.accent).toBe('teal')
    expect(wrapper.get('[data-accent-value]').text()).toBe('teal')
    expect(document.documentElement.dataset.accent).toBe('teal')
  })
})
