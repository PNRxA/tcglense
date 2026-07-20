import { describe, it, expect, beforeEach, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { AlertFinish } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

// A stable create-mutation mock so tests can assert the submitted payload. The composable is
// mocked so the dialog mounts without the authed-mutation/query wiring — these tests are about
// which branch renders (nudge vs form), the finish picker's visibility, and the finish the
// hidden-picker path submits, not the network layer.
const mocks = vi.hoisted(() => ({ mutateAsync: vi.fn<(body: unknown) => Promise<unknown>>() }))
vi.mock('@/composables/useAlerts', () => ({
  useCreateAlertMutation: () => ({ mutateAsync: mocks.mutateAsync, isPending: ref(false) }),
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

// A query-bearing route so `route.fullPath` (what the nudge links preserve) differs from
// `route.path` — a bare path would let a fullPath->path regression pass unnoticed. This mirrors
// the real modal case: the detail overlay lives on `?card=<id>` over the browse grid.
const NAV_PATH = '/cards/mtg/cards?card=sol-ring'

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
      { path: '/cards/:game/cards', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(NAV_PATH)
  await router.isReady()

  const wrapper = mount(CreateAlertDialog, {
    props: {
      // Mount closed, then open, so the `watch(open)` reset actually fires on the false->true
      // edge (it has no `immediate`) — that's the path that flips `finish` to the single
      // available finish when the picker is hidden.
      open: false,
      game: 'mtg',
      targetKind: opts.targetKind ?? 'card',
      externalId: 'sol-ring',
      name: 'Sol Ring',
      finishes: opts.finishes ?? ['nonfoil', 'foil'],
    },
    global: { plugins: [pinia, router], stubs: dialogStubs },
  })
  await wrapper.setProps({ open: true })
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
  beforeEach(() => {
    mocks.mutateAsync.mockReset()
    mocks.mutateAsync.mockResolvedValue({})
  })

  it('nudges a signed-out visitor to create an account instead of showing the form', async () => {
    const wrapper = await mountDialog({ authed: false })
    expect(wrapper.text()).toContain('Create free account')
    expect(wrapper.text()).toContain('Sign in')
    // The threshold form is not offered to someone who can't submit it.
    expect(wrapper.text()).not.toContain('Threshold')
    wrapper.unmount()
  })

  it('sends BOTH nudge links back to the current full path so the detail reopens after auth', async () => {
    const wrapper = await mountDialog({ authed: false })
    const links = wrapper.findAllComponents({ name: 'RouterLink' })
    const signIn = links.find((l) => (l.text() ?? '').includes('Sign in'))
    const register = links.find((l) => (l.text() ?? '').includes('Create free account'))
    expect(signIn).toBeTruthy()
    expect(register).toBeTruthy()
    // Both carry the query-bearing full path (not the bare path) so the modal param survives.
    expect(signIn!.props('to')).toEqual({ path: '/login', query: { redirect: NAV_PATH } })
    expect(register!.props('to')).toEqual({ path: '/register', query: { redirect: NAV_PATH } })
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
  beforeEach(() => {
    mocks.mutateAsync.mockReset()
    mocks.mutateAsync.mockResolvedValue({})
  })

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

  it('submits the single available finish implicitly when the picker is hidden (foil-only)', async () => {
    // The hidden-picker contract: `watch(open)` flips `finish` to the one available finish, so a
    // foil-only target arms a foil alert even though the user never touched a picker. Guards
    // against line 84 regressing to a hardcoded 'nonfoil'.
    const wrapper = await mountDialog({ authed: true, finishes: ['foil'] })
    expect(wrapper.text()).not.toContain('Finish')
    await wrapper.find('#alert-threshold').setValue('5.00')
    await wrapper.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.mutateAsync).toHaveBeenCalledTimes(1)
    expect(mocks.mutateAsync).toHaveBeenCalledWith(
      expect.objectContaining({ target_kind: 'card', finish: 'foil', threshold: '5.00' }),
    )
    wrapper.unmount()
  })
})
