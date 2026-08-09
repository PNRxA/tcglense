import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { onPageHidden } from '../usePageHidden'

/** jsdom's visibilityState is read-only, so drive it through the prototype like the browser. */
function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: state })
  document.dispatchEvent(new Event('visibilitychange'))
}

function mountWithHandler() {
  const handler = vi.fn<() => void>()
  const wrapper = mount(
    defineComponent({
      setup() {
        onPageHidden(handler)
        return () => null
      },
    }),
  )
  return { handler, wrapper }
}

afterEach(() => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
})

describe('onPageHidden', () => {
  it('fires when the page becomes hidden, and not when it becomes visible again', () => {
    const { handler, wrapper } = mountWithHandler()

    setVisibility('hidden')
    expect(handler).toHaveBeenCalledOnce()

    setVisibility('visible')
    expect(handler).toHaveBeenCalledOnce()

    wrapper.unmount()
  })

  it('fires once when visibilitychange and pagehide overlap, as mobile backgrounding does', () => {
    const { handler, wrapper } = mountWithHandler()

    setVisibility('hidden')
    window.dispatchEvent(new Event('pagehide'))

    // Both events describe one departure. A second call would double-release the camera and
    // double-submit the tentative card.
    expect(handler).toHaveBeenCalledOnce()

    wrapper.unmount()
  })

  it('fires on pagehide alone, then re-arms on the bfcache pageshow restore', () => {
    const { handler, wrapper } = mountWithHandler()

    window.dispatchEvent(new Event('pagehide'))
    expect(handler).toHaveBeenCalledOnce()

    // A page restored from the back/forward cache is visible again without a
    // visibilitychange, so pageshow is what re-arms the latch for the next departure.
    window.dispatchEvent(new Event('pageshow'))
    window.dispatchEvent(new Event('pagehide'))
    expect(handler).toHaveBeenCalledTimes(2)

    wrapper.unmount()
  })

  it('stops listening once the owning scope is disposed', () => {
    const { handler, wrapper } = mountWithHandler()

    wrapper.unmount()
    setVisibility('hidden')
    window.dispatchEvent(new Event('pagehide'))

    expect(handler).not.toHaveBeenCalled()
  })
})
