import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { Quad } from '@/lib/scan/detect'
import ScanCameraSurface from '../ScanCameraSurface.vue'

const lockedQuad: Quad = [
  { x: 0.1, y: 0.2 },
  { x: 0.9, y: 0.2 },
  { x: 0.9, y: 0.8 },
  { x: 0.1, y: 0.8 },
]

function mountSurface(overrides: Partial<InstanceType<typeof ScanCameraSurface>['$props']> = {}) {
  return mount(ScanCameraSurface, {
    props: {
      video: null,
      aspect: 0.75,
      status: 'idle',
      errorMessage: null,
      ocrLoading: false,
      cvStatus: 'ready',
      detectedQuad: null,
      captureEnabled: false,
      captureLabel: 'Scan card',
      processing: false,
      activityLabel: 'Scanning the card…',
      statusHint: 'Fit one card inside the guide.',
      ...overrides,
    },
  })
}

describe('ScanCameraSurface', () => {
  it('keeps the idle camera surface noninteractive around the real start button', async () => {
    const wrapper = mountSurface()

    expect(wrapper.get('[data-testid="scan-camera"]').attributes('role')).toBeUndefined()
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('start')).toHaveLength(1)
  })

  it('shows a guide before lock and keeps the camera capture shortcut stable', async () => {
    const wrapper = mountSurface({ status: 'ready' })
    const shortcut = wrapper.get('button[aria-label="Camera preview — Scan card"]')
    expect(wrapper.get('[data-testid="scan-guide"]')).toBeTruthy()
    expect(shortcut.attributes('aria-disabled')).toBe('true')

    await wrapper.setProps({ detectedQuad: lockedQuad, captureEnabled: true })
    expect(wrapper.find('[data-testid="scan-guide"]').exists()).toBe(false)
    // The outline is a rounded-corner path (~6% of each corner's shorter edge) so the
    // lock-on frame reads like a card, not a hard rectangle. The 80×60 test quad has
    // 60-long side edges, so every corner curves over 3.6 units.
    expect(wrapper.get('[data-testid="scan-outline"] path').attributes('d')).toBe(
      'M 10.00 23.60 Q 10.00 20.00 13.60 20.00 L 86.40 20.00 Q 90.00 20.00 90.00 23.60 ' +
        'L 90.00 76.40 Q 90.00 80.00 86.40 80.00 L 13.60 80.00 Q 10.00 80.00 10.00 76.40 Z',
    )

    const enabledShortcut = wrapper.get('button[aria-label="Camera preview — Scan card"]')
    expect(enabledShortcut.element).toBe(shortcut.element)
    await enabledShortcut.trigger('click')
    expect(wrapper.emitted('capture')).toHaveLength(1)

    await wrapper.setProps({ captureEnabled: false })
    expect(wrapper.get('button[aria-label="Camera preview — Scan card"]').element).toBe(
      shortcut.element,
    )
  })

  it('distinguishes normal detector warm-up from degraded fallback mode', async () => {
    const wrapper = mountSurface({ status: 'ready', cvStatus: 'loading' })

    expect(wrapper.text()).toContain('Preparing scanner…')
    expect(wrapper.text()).not.toContain('Basic detection')

    await wrapper.setProps({ cvStatus: 'fallback', detectedQuad: lockedQuad })
    expect(wrapper.text()).toContain(
      'Basic detection active — use a plain, contrasting background.',
    )
  })

  it('resynchronizes the camera aspect when mobile video dimensions change', async () => {
    const wrapper = mountSurface({ status: 'ready' })
    const element = wrapper.get('video').element as HTMLVideoElement
    Object.defineProperty(element, 'videoWidth', { configurable: true, value: 1920 })
    Object.defineProperty(element, 'videoHeight', { configurable: true, value: 1080 })
    await wrapper.setProps({ video: element })

    await wrapper.get('video').trigger('resize')

    const updates = wrapper.emitted('update:aspect')
    expect(updates?.[updates.length - 1]).toEqual([1920 / 1080])
  })
})
