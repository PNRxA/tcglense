import { createPinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AuthResponse,
  CompleteRegistrationPayload,
  PublicConfig,
  RegisterPayload,
  RegisterResponse,
} from '@/lib/api'

const mocks = vi.hoisted(() => ({
  completeRegistration:
    vi.fn<(payload: CompleteRegistrationPayload) => Promise<AuthResponse>>(),
  execute: vi.fn<() => Promise<string | null>>(),
  publicConfig: vi.fn<() => Promise<PublicConfig>>(),
  register: vi.fn<(payload: RegisterPayload) => Promise<RegisterResponse>>(),
}))

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  completeRegistration: mocks.completeRegistration,
  register: mocks.register,
}))

vi.mock('@/lib/config', () => ({ publicConfig: mocks.publicConfig }))

vi.mock('@/composables/useTurnstile', () => ({
  useTurnstile: () => ({ execute: mocks.execute }),
}))

import CompleteRegistrationView from '../CompleteRegistrationView.vue'
import LoginView from '../LoginView.vue'
import RegisterView from '../RegisterView.vue'

const USER = {
  id: 7,
  email: 'new@example.com',
  created_at: '2026-07-15T00:00:00Z',
  username: null,
  discriminator: null,
  handle: null,
  currency: 'USD',
}

function makeRouter() {
  const page = { template: '<div />' }
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: page },
      { path: '/register', component: page },
      { path: '/complete-registration', component: page },
      { path: '/collection/:game', component: page },
      { path: '/terms', component: page },
      { path: '/privacy', component: page },
      { path: '/login', component: page },
    ],
  })
}

beforeEach(() => {
  mocks.completeRegistration.mockReset()
  mocks.execute.mockReset().mockResolvedValue(null)
  mocks.publicConfig.mockReset().mockResolvedValue({
    turnstile_site_key: null,
    signups_enabled: true,
    signups_disabled_message: null,
  })
  mocks.register.mockReset()
})

describe('registration redirect', () => {
  it('preserves a safe redirect in the login/register cross-links', async () => {
    const router = makeRouter()
    await router.push('/login?redirect=/collection/mtg')
    const login = mount(LoginView, {
      global: { plugins: [createPinia(), router] },
    })
    const createAccount = login.findAll('a').find((link) => link.text().includes('Create one'))
    expect(createAccount?.attributes('href')).toBe('/register?redirect=/collection/mtg')
    login.unmount()

    await router.push('/register?redirect=/collection/mtg')
    const register = mount(RegisterView, { global: { plugins: [router] } })
    await flushPromises()
    const signIn = register.findAll('a').find((link) => link.text().includes('Sign in'))
    expect(signIn?.attributes('href')).toBe('/login?redirect=/collection/mtg')
    register.unmount()

    await router.push('/login?redirect=//evil.example')
    const unsafeLogin = mount(LoginView, {
      global: { plugins: [createPinia(), router] },
    })
    const safeCreateAccount = unsafeLogin
      .findAll('a')
      .find((link) => link.text().includes('Create one'))
    expect(safeCreateAccount?.attributes('href')).toBe('/register')
    unsafeLogin.unmount()
  })

  it('sends the safe redirect and preserves it through the dev completion bypass', async () => {
    mocks.register.mockResolvedValue({ completion_token: 'dev-token' })
    const router = makeRouter()
    await router.push('/register?redirect=/collection/mtg')
    const wrapper = mount(RegisterView, { global: { plugins: [router] } })
    await flushPromises()

    await wrapper.get('#email').setValue('New@Example.com')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(mocks.register).toHaveBeenCalledWith({
      email: 'New@Example.com',
      redirect: '/collection/mtg',
      captcha_token: undefined,
    })
    expect(router.currentRoute.value.path).toBe('/complete-registration')
    expect(router.currentRoute.value.query).toEqual({
      token: 'dev-token',
      redirect: '/collection/mtg',
    })
    wrapper.unmount()
  })

  it('scrubs the completion token and returns to the preserved redirect', async () => {
    mocks.completeRegistration.mockResolvedValue({ access_token: 'access-token', user: USER })
    const router = makeRouter()
    await router.push('/complete-registration?token=email-secret&redirect=/collection/mtg')
    const pinia = createPinia()
    const wrapper = mount(CompleteRegistrationView, {
      global: { plugins: [pinia, router] },
    })
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ redirect: '/collection/mtg' })
    const terms = wrapper.get('a[href="/terms"]')
    const privacy = wrapper.get('a[href="/privacy"]')
    expect(terms.attributes()).toMatchObject({ target: '_blank', rel: 'noopener noreferrer' })
    expect(privacy.attributes()).toMatchObject({ target: '_blank', rel: 'noopener noreferrer' })
    await terms.trigger('click')
    expect(router.currentRoute.value.path).toBe('/complete-registration')
    await wrapper.get('#password').setValue('correct horse battery staple')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(mocks.completeRegistration).toHaveBeenCalledWith({
      token: 'email-secret',
      password: 'correct horse battery staple',
      username: null,
      captcha_token: undefined,
    })
    expect(router.currentRoute.value.fullPath).toBe('/collection/mtg')
    wrapper.unmount()
  })
})
