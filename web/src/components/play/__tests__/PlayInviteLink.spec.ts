import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import PlayInviteLink from '../PlayInviteLink.vue'

// The invite link is how anybody else gets to the table, so the two things pinned are that the
// URL it copies is the room's real route (an invite to the wrong page is a dead invite) and
// that the copy button confirms itself — without the "Copied" flip, a silent clipboard write is
// indistinguishable from a broken button.

const writeText = vi.fn<(text: string) => Promise<void>>()

beforeEach(() => {
  vi.useFakeTimers()
  writeText.mockReset().mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
  })
})

afterEach(() => {
  vi.useRealTimers()
})

function mountLink() {
  return mount(PlayInviteLink, { props: { game: 'mtg', code: 'ABC234' } })
}

function copyButton(wrapper: ReturnType<typeof mountLink>) {
  const button = wrapper.findAll('button').find((b) => b.text().includes('Copy'))
  if (!button) throw new Error('no copy button')
  return button
}

describe('PlayInviteLink', () => {
  it('shows the room URL and the code apart, since they are used differently', () => {
    const wrapper = mountLink()
    expect(wrapper.get('[data-testid="play-invite-url"]').text()).toBe(
      `${window.location.origin}/tools/mtg/play/ABC234`,
    )
    // The code is what gets read out loud, so it's on screen as well as inside the link.
    expect(wrapper.text()).toContain('ABC234')
  })

  it('copies the absolute link and confirms it', async () => {
    const wrapper = mountLink()
    await copyButton(wrapper).trigger('click')
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledTimes(1))

    expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/tools/mtg/play/ABC234`)
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('Copied')
  })

  it('returns to its resting label so a second copy is obviously a second copy', async () => {
    const wrapper = mountLink()
    await copyButton(wrapper).trigger('click')
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled())
    await wrapper.vm.$nextTick()

    vi.advanceTimersByTime(2000)
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).not.toContain('Copied')
  })

  it('stays quiet when the clipboard is blocked — the link is still on screen', async () => {
    writeText.mockRejectedValue(new Error('denied'))
    const wrapper = mountLink()
    await copyButton(wrapper).trigger('click')
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled())
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).not.toContain('Copied')
    expect(wrapper.get('[data-testid="play-invite-url"]').text()).toContain('/play/ABC234')
  })
})
