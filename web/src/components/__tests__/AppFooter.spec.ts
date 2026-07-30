import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import AppFooter from '../AppFooter.vue'
import { publicNavItems } from '@/lib/nav'

// A catch-all keeps the spec from having to restate the registry as a route table — the
// point here is which links the footer renders, not whether the router knows them.
function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
}

async function mountFooter() {
  const router = makeRouter()
  router.push('/')
  await router.isReady()
  // No pinia and no query client on purpose: the footer is fully static, so needing
  // either here would itself be the regression.
  return mount(AppFooter, { global: { plugins: [router] } })
}

function column(wrapper: Awaited<ReturnType<typeof mountFooter>>, heading: string) {
  const found = wrapper
    .findAll('nav[aria-label="Footer"] > div')
    .find((col) => col.get('h2').text() === heading)
  if (!found) throw new Error(`no "${heading}" footer column`)
  return found
}

describe('AppFooter', () => {
  it('derives the Product column from the nav registry', async () => {
    const wrapper = await mountFooter()
    const links = column(wrapper, 'Product').findAll('a')

    expect(links.map((link) => link.attributes('href'))).toEqual(
      publicNavItems().map((item) => item.landing),
    )
    expect(links.map((link) => link.text())).toEqual(publicNavItems().map((item) => item.label))
  })

  it('omits the auth-gated Scan link', async () => {
    // The footer has no room to explain a sign-in prompt, so `auth` items stay out —
    // `publicNavItems()` filters them, and this pins that the footer honours it.
    const wrapper = await mountFooter()
    expect(wrapper.findAll('a[href="/scan"]')).toHaveLength(0)
  })

  it('keeps the API reference in Project, exactly once', async () => {
    // `/docs` is a bare top-bar link, not a menu root, so deriving Product must not pull it
    // up out of the Project column beside GitHub and Terms.
    const wrapper = await mountFooter()
    expect(wrapper.findAll('a[href="/docs"]')).toHaveLength(1)
    expect(column(wrapper, 'Project').find('a[href="/docs"]').exists()).toBe(true)
    expect(column(wrapper, 'Product').find('a[href="/docs"]').exists()).toBe(false)
  })
})
