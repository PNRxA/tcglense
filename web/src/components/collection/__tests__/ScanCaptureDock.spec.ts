import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import ScanCaptureDock from '../ScanCaptureDock.vue'

function mountDock(overrides: Partial<InstanceType<typeof ScanCaptureDock>['$props']> = {}) {
  return mount(ScanCaptureDock, {
    props: {
      statusHint: 'Card locked on — ready to scan.',
      captureLabel: 'Scan card',
      captureDisabled: false,
      controlsDisabled: false,
      stopDisabled: false,
      stopping: false,
      matchName: null,
      addedCount: 0,
      ...overrides,
    },
  })
}

describe('ScanCaptureDock', () => {
  it('exposes one large primary action and forwards scanner controls', async () => {
    const wrapper = mountDock()
    const capture = wrapper.findAll('button').find((button) => button.text() === 'Scan card')
    expect(capture).toBeDefined()

    expect(capture!.classes()).toContain('h-12')
    expect(wrapper.get('[data-testid="scan-capture-dock"]').classes()).toContain('fixed')

    await wrapper.get('button[aria-label="Switch camera"]').trigger('click')
    await capture!.trigger('click')
    await wrapper.get('button[aria-label="Stop scanning"]').trigger('click')

    expect(wrapper.emitted('switchCamera')).toHaveLength(1)
    expect(wrapper.emitted('capture')).toHaveLength(1)
    expect(wrapper.emitted('stop')).toHaveLength(1)
  })

  it('keeps keyboard focus when capture becomes temporarily unavailable', async () => {
    const wrapper = mount(ScanCaptureDock, {
      attachTo: document.body,
      props: {
        statusHint: 'Card locked on — ready to scan.',
        captureLabel: 'Scan card',
        captureDisabled: false,
        controlsDisabled: false,
        stopDisabled: false,
        stopping: false,
        matchName: null,
        addedCount: 0,
      },
    })
    const capture = wrapper.findAll('button').find((button) => button.text() === 'Scan card')!
    const captureElement = capture.element as HTMLButtonElement
    captureElement.focus()

    await wrapper.setProps({ captureDisabled: true })

    expect(document.activeElement).toBe(captureElement)
    expect(capture.attributes('aria-disabled')).toBe('true')
    await capture.trigger('click')
    expect(wrapper.emitted('capture')).toBeUndefined()
    wrapper.unmount()
  })

  it('surfaces the tentative match and session tally without crowding the primary action', async () => {
    const wrapper = mountDock({
      captureLabel: 'Add & scan next',
      matchName: 'Black Lotus',
      addedCount: 4,
    })

    expect(wrapper.text()).toContain('Add & scan next')
    expect(wrapper.text()).toContain('4 added')
    const review = wrapper.findAll('button').find((button) => button.text() === 'Review')
    expect(review).toBeDefined()
    await review!.trigger('click')
    expect(wrapper.emitted('review')).toHaveLength(1)
  })
})
