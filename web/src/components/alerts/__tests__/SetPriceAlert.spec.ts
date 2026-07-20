import { describe, it, expect, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { AlertFinish } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

// The create mutation is mocked so the dialog can mount without the authed-mutation/query
// wiring — these tests are about which branch renders (nudge vs form) and the finish picker's
// visibility, not the network layer.
vi.mock('@/composables/useAlerts', () => ({
  useCreateAlertMutation: () => ({
    mutateAsync: vi.fn<() => Promise<unknown>>().mockResolvedValue({}),
    isPending: ref(false),
  }),
}))

import CreateAlertDialog from '../CreateAlertDialog.vue'
import SetPriceAlertButton from '../SetPriceAlertButton.vue'

// Pass-through stubs for the reka dialog + select chrome so the body renders inline (not
// teleported) and its text/labels are assertable. Real Button/Input/Label are kept.
const PassThrough = { template: '<div><slot /></div>' }
const dialogStubs = {
  Dialog: PassThrough,
  DialogContent: PassThrough,
  DialogTitle: PassThrough,
  DialogDescription: PassThrough,
  DialogClose: { template: '<button><slot /></button>' },
  Select: PassThrough,
  SelectTrigger: PassThrough,
  SelectContent: PassThrough,
  SelectItem: PassThrough,
  SelectValue: PassThrough,
}

async function mountDialog(opts: {
  authed: boolean
  resolved?: boolean
  finishes?: AlertFinish[]
  targetKind?: 'card' | 'product'
}) {
  const pinia: Pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()
  auth.sessionResolved = opts.resolved ?? true
  if (opts.authed) auth.accessToken = 'token'

  const router: Router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/login', component: { template: '<div />' } },
      { path: '/register', component: { template: '<div />' } },
      { path: '/alerts', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push('/cards/mtg/cards/sol-ring')
  await router.isReady()

  const wrapper = mount(CreateAlertDialog, {
    props: {
      open: true,
      game: 'mtg',
      targetKind: opts.targetKind ?? 'card',
      externalId: 'sol-ring',
      name: 'Sol Ring',
      finishes: opts.finishes ?? ['nonfoil', 'foil'],
    },
    global: { plugins: [pinia, router], stubs: dialogStubs },
  })
  await flushPromises()
  return wrapper
}

describe('SetPriceAlertButton', () => {
  it('renders the trigger for everyone — including signed-out visitors', () => {
    // The whole point of the fix: the button is no longer auth-gated, so a signed-out visitor
    // sees it (and the dialog it opens nudges them to make an account).
    const wrapper = mount(SetPriceAlertButton, {
      props: {
        game: 'mtg',
        targetKind: 'card',
        externalId: 'x',
        name: 'Sol Ring',
        finishes: ['nonfoil'],
      },
      global: { stubs: { CreateAlertDialog: true } },
    })
    expect(wrapper.text()).toContain('Set price alert')
    wrapper.unmount()
  })

  it('opens the dialog on click', async () => {
    const wrapper = mount(SetPriceAlertButton, {
      props: {
        game: 'mtg',
        targetKind: 'card',
        externalId: 'x',
        name: 'Sol Ring',
        finishes: ['nonfoil'],
      },
      global: { stubs: { CreateAlertDialog: true } },
    })
    const dialog = wrapper.findComponent(CreateAlertDialog)
    expect(dialog.props('open')).toBe(false)
    await wrapper.find('button').trigger('click')
    expect(dialog.props('open')).toBe(true)
    wrapper.unmount()
  })
})

describe('CreateAlertDialog auth branch', () => {
  it('nudges a signed-out visitor to create an account instead of showing the form', async () => {
    const wrapper = await mountDialog({ authed: false })
    expect(wrapper.text()).toContain('Create free account')
    expect(wrapper.text()).toContain('Sign in')
    // The threshold form is not offered to someone who can't submit it.
    expect(wrapper.text()).not.toContain('Threshold')
    wrapper.unmount()
  })

  it('sends the sign-in link back to the current page so the detail reopens post-login', async () => {
    const wrapper = await mountDialog({ authed: false })
    const signIn = wrapper
      .findAllComponents({ name: 'RouterLink' })
      .find((l) => (l.text() ?? '').includes('Sign in'))
    expect(signIn).toBeTruthy()
    expect(signIn!.props('to')).toEqual({
      path: '/login',
      query: { redirect: '/cards/mtg/cards/sol-ring' },
    })
    wrapper.unmount()
  })

  it('shows the alert form to a signed-in user', async () => {
    const wrapper = await mountDialog({ authed: true })
    expect(wrapper.text()).toContain('Threshold')
    expect(wrapper.text()).not.toContain('Create free account')
    wrapper.unmount()
  })

  it('waits out an unresolved session rather than flashing the nudge', async () => {
    const wrapper = await mountDialog({ authed: false, resolved: false })
    expect(wrapper.text()).not.toContain('Create free account')
    expect(wrapper.text()).not.toContain('Threshold')
    wrapper.unmount()
  })
})

describe('CreateAlertDialog finish picker', () => {
  it('hides the finish picker when only one finish is available', async () => {
    const wrapper = await mountDialog({ authed: true, finishes: ['nonfoil'] })
    expect(wrapper.text()).not.toContain('Finish')
    wrapper.unmount()
  })

  it('shows the finish picker when both regular and foil are priced', async () => {
    const wrapper = await mountDialog({ authed: true, finishes: ['nonfoil', 'foil'] })
    expect(wrapper.text()).toContain('Finish')
    wrapper.unmount()
  })

  it('treats a finish-less sealed product as a single implicit finish (no picker)', async () => {
    const wrapper = await mountDialog({
      authed: true,
      targetKind: 'product',
      finishes: ['nonfoil'],
    })
    expect(wrapper.text()).not.toContain('Finish')
    wrapper.unmount()
  })
})
