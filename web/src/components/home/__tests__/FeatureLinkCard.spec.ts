import { describe, expect, it } from 'vitest'
import { mount, RouterLinkStub } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import FeatureLinkCard from '../FeatureLinkCard.vue'

// A stand-in for a lucide icon: a bare <svg> so the assertions can find it.
const Icon = defineComponent({ render: () => h('svg', { 'data-test': 'icon' }) })

function mountCard(feature: Record<string, unknown>, variant?: 'card' | 'row') {
  return mount(FeatureLinkCard, {
    props: {
      feature: { icon: Icon, title: 'Wish lists', description: 'The cards', ...feature },
      variant,
    },
    global: { stubs: { RouterLink: RouterLinkStub } },
  })
}

describe('FeatureLinkCard', () => {
  it('renders an in-app feature as a RouterLink, with the title and description', () => {
    const wrapper = mountCard({ to: '/wishlist' })
    const link = wrapper.getComponent(RouterLinkStub)
    expect(link.props('to')).toBe('/wishlist')
    // The stub renders an <a> of its own; what matters is that it is the router's, not a
    // new-tab anchor.
    expect(link.attributes('target')).toBeUndefined()
    expect(wrapper.text()).toContain('Wish lists')
    expect(wrapper.text()).toContain('The cards')
    expect(wrapper.find('[data-test="icon"]').exists()).toBe(true)
  })

  it('renders an external feature as a new-tab anchor, never a RouterLink', () => {
    const wrapper = mountCard({ href: 'https://github.com/PNRxA/tcglense-cli' })
    const anchor = wrapper.get('a')
    expect(anchor.attributes('href')).toBe('https://github.com/PNRxA/tcglense-cli')
    expect(anchor.attributes('target')).toBe('_blank')
    expect(anchor.attributes('rel')).toBe('noopener noreferrer')
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(0)
  })

  it('keeps every decorative icon hidden from assistive tech in both weights', () => {
    for (const variant of ['card', 'row'] as const) {
      const wrapper = mountCard({ to: '/wishlist' }, variant)
      const svgs = wrapper.findAll('svg')
      expect(svgs.length, `${variant}: the feature icon and the affordance`).toBeGreaterThan(0)
      // Only the root is a link — nothing nested is focusable on its own.
      expect(wrapper.element.querySelectorAll('a, button, [tabindex]')).toHaveLength(0)
      wrapper.unmount()
    }
  })
})
