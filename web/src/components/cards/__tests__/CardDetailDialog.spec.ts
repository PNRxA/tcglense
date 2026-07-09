import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import CardDetailDialog from '../CardDetailDialog.vue'
import { useCardNavStore } from '@/stores/cardNav'

let wrapper: VueWrapper

// Mount the dialog over a page whose registered grid holds `ids`, opened on `card`. The card
// body is stubbed — this suite is about the header's prev/next + the arrow keys, which live in
// the dialog itself, not in CardDetailContent.
async function open(card: string, ids: string[] = ['a', 'b', 'c']) {
  const router: Router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/cards/:game', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  const pinia: Pinia = createPinia()
  setActivePinia(pinia)
  useCardNavStore().register({ game: 'mtg', ids })

  await router.push(`/cards/mtg?card=${card}`)
  await router.isReady()

  wrapper = mount(CardDetailDialog, {
    attachTo: document.body,
    global: {
      plugins: [router, pinia],
      stubs: { CardDetailContent: true },
    },
  })
  await flushPromises()
  return router
}

// reka teleports the dialog to <body>, so reach controls through the document, not the wrapper.
function byLabel(label: string): HTMLButtonElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

function dialogEl(): HTMLElement {
  const el = document.body.querySelector('[role="dialog"]')
  if (!el) throw new Error('dialog is not open')
  return el as HTMLElement
}

// The key handler lives on the dialog's own content (not window), so a keydown must originate
// inside it to be seen — mirroring how a real keypress only reaches it while the modal is focused.
function pressArrow(key: 'ArrowLeft' | 'ArrowRight', init: KeyboardEventInit = {}) {
  dialogEl().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }))
}

describe('CardDetailDialog card navigation (issue #275)', () => {
  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('shows prev/next and a position counter for a card mid-list', async () => {
    await open('b')
    expect(byLabel('Previous card')).not.toBeNull()
    expect(byLabel('Next card')).not.toBeNull()
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain('2 / 3')
  })

  it('advances the card via the next button (rewrites ?card=)', async () => {
    const router = await open('b')
    byLabel('Next card')!.click()
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('c')
  })

  it('steps with router.replace, not push, so Back still exits the modal in one press', async () => {
    const router = await open('b')
    const pushSpy = vi.spyOn(router, 'push')
    const replaceSpy = vi.spyOn(router, 'replace')
    byLabel('Next card')!.click()
    await flushPromises()
    expect(replaceSpy).toHaveBeenCalledTimes(1)
    expect(pushSpy).not.toHaveBeenCalled()
    expect(router.currentRoute.value.query.card).toBe('c')
  })

  it('goes back via the prev button', async () => {
    const router = await open('b')
    byLabel('Previous card')!.click()
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('a')
  })

  it('disables prev on the first card and next on the last', async () => {
    await open('a')
    expect(byLabel('Previous card')!.disabled).toBe(true)
    expect(byLabel('Next card')!.disabled).toBe(false)

    wrapper.unmount()
    document.body.innerHTML = ''

    await open('c')
    expect(byLabel('Previous card')!.disabled).toBe(false)
    expect(byLabel('Next card')!.disabled).toBe(true)
  })

  it('steps forward on ArrowRight and back on ArrowLeft', async () => {
    const router = await open('b')

    pressArrow('ArrowRight')
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('c')

    pressArrow('ArrowLeft')
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('b')
  })

  it('ignores an arrow with a modifier held (leaves browser shortcuts alone)', async () => {
    const router = await open('b')
    pressArrow('ArrowRight', { metaKey: true })
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('b')
  })

  it('does not hijack arrows while typing in one of the modal’s inputs', async () => {
    const router = await open('b')
    // A field inside the dialog (a quantity input) — arrows there must move the cursor, not cards.
    const input = document.createElement('input')
    dialogEl().appendChild(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('b')
  })

  it('ignores arrows from outside its own content (a nested overlay / the page behind)', async () => {
    const router = await open('b')
    // A keydown that doesn't originate inside the modal's content — e.g. the image-zoom lightbox
    // stacked on top (teleported as a sibling) — must not step the underlying card.
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('b')
  })

  it('offers no nav when the open card is on no registered grid (a deep link)', async () => {
    await open('z')
    expect(byLabel('Previous card')).toBeNull()
    expect(byLabel('Next card')).toBeNull()
  })

  it('reveals nav reactively when a grid registers after the modal opens', async () => {
    // A cold deep link: the modal is already up before the page's grid has finished loading,
    // so there's no nav yet.
    await open('b', [])
    expect(byLabel('Next card')).toBeNull()

    // The page's grid loads and publishes its cards — the modal must pick it up live.
    useCardNavStore().register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    await nextTick()
    expect(byLabel('Next card')).not.toBeNull()
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain('2 / 3')
  })

  it('does not advance past the last card on ArrowRight', async () => {
    const router = await open('c')
    pressArrow('ArrowRight')
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBe('c')
  })
})
