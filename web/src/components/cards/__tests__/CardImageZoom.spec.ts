import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import CardImageZoom from '../CardImageZoom.vue'

const baseProps = { game: 'mtg', id: 'abc-123', name: 'Black Lotus' }

let wrapper: VueWrapper

function mountZoom(props: Record<string, unknown> = {}) {
  wrapper = mount(CardImageZoom, {
    props: { ...baseProps, ...props },
    attachTo: document.body,
  })
  return wrapper
}

describe('CardImageZoom', () => {
  afterEach(() => {
    // Unmount so reka-ui's Dialog reverts its body scroll-lock / portal between tests.
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('renders an accessible enlarge trigger over the thumbnail', () => {
    mountZoom()
    const trigger = wrapper.get('button')
    expect(trigger.attributes('aria-label')).toBe('Enlarge image of Black Lotus')

    // The inline thumbnail uses the default `large` size, not the enlarged one.
    const img = wrapper.get('img')
    expect(img.attributes('src')).toContain('/api/games/mtg/cards/abc-123/image?size=large')
  })

  it('does not render a zoom trigger when there is no image', () => {
    mountZoom({ hasImage: false })
    expect(wrapper.find('button').exists()).toBe(false)
    // Falls back to CardImage's no-image placeholder (the alt/name text).
    expect(wrapper.text()).toContain('Black Lotus')
  })

  it('degrades to the plain placeholder when the thumbnail fails to load', async () => {
    mountZoom()
    expect(wrapper.find('button').exists()).toBe(true)

    // Simulate the image 404ing at runtime though the card claimed has_image.
    await wrapper.get('img').trigger('error')

    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.text()).toContain('Black Lotus')
  })

  it('opens a dialog with the high-res image and a close control', async () => {
    mountZoom()
    await wrapper.get('button').trigger('click')
    await flushPromises()

    const dialog = document.body.querySelector('[role="dialog"]')
    expect(dialog).not.toBeNull()

    // The enlarged image requests the `png` (highest-res) size.
    const enlarged = dialog?.querySelector('img')
    expect(enlarged?.getAttribute('src')).toContain('/api/games/mtg/cards/abc-123/image?size=png')
    // A labelled close button is present for dismissal.
    expect(document.body.querySelector('[aria-label="Close"]')).not.toBeNull()
  })

  it('passes the face index through to both the thumbnail and the enlarged image', async () => {
    mountZoom({ face: 1 })
    expect(wrapper.get('img').attributes('src')).toContain('size=large&face=1')

    await wrapper.get('button').trigger('click')
    await flushPromises()
    const enlarged = document.body.querySelector('[role="dialog"] img')
    expect(enlarged?.getAttribute('src')).toContain('size=png&face=1')
  })
})
