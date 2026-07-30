import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import SeatLinkField from '../SeatLinkField.vue'

// The seat's "what were they playing" control enforces one rule the server also enforces: a seat
// links to a deck OR a commander, never both. Getting that wrong wouldn't error — it would quietly
// produce a seat whose per-deck record and displayed commander disagree — so it's pinned here.

// Both pickers fetch (the caller's decks / card-name hints); stub them so this spec is about the
// mode logic, not the network.
vi.mock('@/composables/useDecks', () => ({
  useDecksQuery: () => ({ data: { value: { data: [] } }, isPending: { value: false } }),
}))
vi.mock('@/composables/useQuickAdd', () => ({
  QUICK_ADD_MIN_CHARS: 2,
  useCardNameSuggestions: () => ({ data: { value: { data: [] } }, isFetching: { value: false } }),
}))

function mountField(props: Record<string, unknown> = {}) {
  return mount(SeatLinkField, {
    props: { game: 'mtg', deckId: null, commanderCardId: null, ...props },
    global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] },
  })
}

/** The mode toggle's three options, by the label a user reads. */
function modeButton(wrapper: ReturnType<typeof mountField>, label: string) {
  const button = wrapper.findAll('button').find((b) => b.text() === label)
  if (!button) throw new Error(`no "${label}" mode button`)
  return button
}

describe('SeatLinkField', () => {
  it('opens on whichever link the seat already has', () => {
    // A deck-linked seat shows the deck picker...
    const deckSeat = mountField({ deckId: 7 })
    expect(deckSeat.findComponent({ name: 'DeckPickerField' }).exists()).toBe(true)
    expect(deckSeat.findComponent({ name: 'CommanderPickerField' }).exists()).toBe(false)

    // ...and a commander-linked one shows the commander box.
    const commanderSeat = mountField({ commanderCardId: 'abc', commanderName: 'Atraxa' })
    expect(commanderSeat.findComponent({ name: 'CommanderPickerField' }).exists()).toBe(true)
    expect(commanderSeat.findComponent({ name: 'DeckPickerField' }).exists()).toBe(false)
  })

  it('explains itself rather than showing an empty control when nothing is linked', () => {
    const wrapper = mountField()
    expect(wrapper.findComponent({ name: 'DeckPickerField' }).exists()).toBe(false)
    expect(wrapper.findComponent({ name: 'CommanderPickerField' }).exists()).toBe(false)
    expect(wrapper.text()).toContain('Counts life only')
  })

  it('clears the deck when switching to a commander, so the pair is never both set', async () => {
    const wrapper = mountField({ deckId: 7 })
    await modeButton(wrapper, 'Commander').trigger('click')
    // The deck link is dropped in the same breath as the mode change — the server would answer
    // 422 for a seat carrying both, so the client never builds one.
    expect(wrapper.emitted('update:deckId')).toEqual([[null]])
    expect(wrapper.findComponent({ name: 'CommanderPickerField' }).exists()).toBe(true)
  })

  it('clears the commander when switching to a deck', async () => {
    const wrapper = mountField({ commanderCardId: 'abc', commanderName: 'Atraxa' })
    await modeButton(wrapper, 'My deck').trigger('click')
    expect(wrapper.emitted('update:commanderCardId')).toEqual([[null]])
    expect(wrapper.findComponent({ name: 'DeckPickerField' }).exists()).toBe(true)
  })

  it('clears both when the seat is set to link nothing', async () => {
    const wrapper = mountField({ deckId: 7 })
    await modeButton(wrapper, 'Neither').trigger('click')
    expect(wrapper.emitted('update:deckId')).toEqual([[null]])
    expect(wrapper.emitted('update:commanderCardId')).toEqual([[null]])
  })

  // The specs above assert what the field *emits*; a real parent then applies that emit and feeds
  // it straight back down as props. That round trip is where the mode used to be lost: the clear
  // for the other link arrives as a prop change with both links null, which re-derives "Neither"
  // and throws away the choice just made. So these drive the props the way the dialog does.
  it('keeps the chosen mode when the parent applies the clear it just emitted', async () => {
    const wrapper = mountField({ deckId: 7 })
    await modeButton(wrapper, 'Commander').trigger('click')
    // The parent handles `update:deckId` by writing null back into the prop.
    await wrapper.setProps({ deckId: null })

    expect(wrapper.findComponent({ name: 'CommanderPickerField' }).exists()).toBe(true)
    expect(wrapper.text()).not.toContain('Counts life only')
  })

  it('keeps "My deck" when the parent applies the commander clear', async () => {
    const wrapper = mountField({ commanderCardId: 'abc', commanderName: 'Atraxa' })
    await modeButton(wrapper, 'My deck').trigger('click')
    await wrapper.setProps({ commanderCardId: null })

    expect(wrapper.findComponent({ name: 'DeckPickerField' }).exists()).toBe(true)
    expect(wrapper.text()).not.toContain('Counts life only')
  })

  it('still follows a link that arrives from outside', async () => {
    // The guard above must not make the field ignore its props altogether: a seat that gains a
    // link elsewhere still opens on it.
    const wrapper = mountField()
    await wrapper.setProps({ deckId: 12 })
    expect(wrapper.findComponent({ name: 'DeckPickerField' }).exists()).toBe(true)
  })

  it('labels each control with the seat, so a table of six is navigable by screen reader', () => {
    const wrapper = mountField({ seatLabel: 'Priya' })
    const labels = wrapper.findAll('button').map((b) => b.attributes('aria-label'))
    expect(labels).toContain('Link a deck for Priya')
    expect(labels).toContain('Name a commander for Priya')
  })
})
