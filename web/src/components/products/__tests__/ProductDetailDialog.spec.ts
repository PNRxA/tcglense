import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import ProductDetailDialog from '../ProductDetailDialog.vue'
import { useProductNavStore } from '@/stores/productNav'

let wrapper: VueWrapper

async function open(product: string, ids: string[] = ['a', 'b', 'c']) {
  const router: Router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  useProductNavStore().register({ game: 'mtg', ids })
  await router.push(`/sealed/mtg?sort=name&product=${product}`)
  await router.isReady()

  wrapper = mount(ProductDetailDialog, {
    attachTo: document.body,
    global: {
      plugins: [router, pinia],
      stubs: { ProductDetailContent: true },
    },
  })
  await flushPromises()
  return router
}

function byLabel(label: string): HTMLButtonElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

function dialogEl(): HTMLElement {
  const el = document.body.querySelector('[role="dialog"]')
  if (!el) throw new Error('dialog is not open')
  return el as HTMLElement
}

describe('ProductDetailDialog', () => {
  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('opens from ?product and links to the canonical full page', async () => {
    await open('b')
    expect(dialogEl()).not.toBeNull()
    expect(document.body.querySelector('a[href="/sealed/mtg/b"]')?.textContent).toContain(
      'Open full page',
    )
  })

  it('steps through the underlying product grid with replace', async () => {
    const router = await open('b')
    const replace = vi.spyOn(router, 'replace')
    byLabel('Next sealed product')!.click()
    await flushPromises()

    expect(replace).toHaveBeenCalledTimes(1)
    expect(router.currentRoute.value.query.product).toBe('c')
    expect(dialogEl().textContent).toContain('3 / 3')
  })

  it('steps with arrow keys without hijacking quantity inputs', async () => {
    const router = await open('b')
    dialogEl().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('a')

    const input = document.createElement('input')
    dialogEl().appendChild(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('a')
  })

  it('closes by removing only modal state', async () => {
    const router = await open('b')
    byLabel('Close')!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ sort: 'name' })
  })

  it('hides navigation for a deep-linked product outside the registered grid', async () => {
    await open('z')
    expect(byLabel('Previous sealed product')).toBeNull()
    expect(byLabel('Next sealed product')).toBeNull()
  })
})
