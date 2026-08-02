import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import LifeEventList from '../LifeEventList.vue'
import LifeHistoryChart from '../LifeHistoryChart.vue'
import type { LifeEvent, LifeSeat } from '@/lib/api'
import { lifeLines } from '@/lib/lifeSeries'

const START = '2026-07-30T12:00:00.000Z'

function seat(over: Partial<LifeSeat> & { id: number; position: number }): LifeSeat {
  return {
    name: `Player ${over.position + 1}`,
    deck_id: null,
    deck_name: null,
    starting_life: 40,
    life: 40,
    rotation: 0,
    result: 'none',
    ...over,
  } as LifeSeat
}

function event(over: Partial<LifeEvent> & { id: number; player_id: number }): LifeEvent {
  return {
    delta: -1,
    life_after: 39,
    kind: 'adjust',
    counter: 'life',
    source_player_id: null,
    created_at: START,
    ...over,
  } as LifeEvent
}

// The unovis body is stubbed: the wrapper's job is the fold into steps and the nothing-to-draw
// branch, and mounting a real chart in jsdom would test unovis rather than us.
async function mountChart(seats: LifeSeat[], events: LifeEvent[]) {
  const wrapper = mount(LifeHistoryChart, {
    props: { lines: lifeLines(seats, events, START) },
    global: {
      stubs: {
        LifeHistoryChartInner: {
          name: 'LifeHistoryChartInner',
          props: ['lines', 'steps'],
          template: '<div class="chart-inner-stub" />',
        },
      },
    },
  })
  await flushPromises()
  return wrapper
}

describe('LifeHistoryChart', () => {
  it('hands the body one evenly-spaced column per change', async () => {
    const wrapper = await mountChart(
      [seat({ id: 1, position: 0 }), seat({ id: 2, position: 1 })],
      [
        event({ id: 10, player_id: 1, life_after: 37 }),
        event({ id: 11, player_id: 2, life_after: 35 }),
      ],
    )
    const inner = wrapper.findComponent({ name: 'LifeHistoryChartInner' })
    expect(inner.exists()).toBe(true)
    // Two changes plus the starting column, and the x value is the index — not the timestamp.
    expect(inner.props('steps').map((s: { step: number }) => s.step)).toEqual([0, 1, 2])
  })

  it('says so instead of drawing a one-column frame', async () => {
    // Every recorded change belongs to a seat that has since been removed, so `lifeLines` drops
    // them all and there is nothing but the starting column left.
    const wrapper = await mountChart(
      [seat({ id: 1, position: 0 })],
      [event({ id: 1, player_id: 99 })],
    )
    expect(wrapper.findComponent({ name: 'LifeHistoryChartInner' }).exists()).toBe(false)
    expect(wrapper.text()).toContain('No changes left to chart')
  })
})

describe('LifeEventList', () => {
  it('keeps a real gap between the player and what they did', () => {
    // Regression: the space lived in a whitespace-only text node, which the template compiler
    // drops at the start of an element — the row read "Player 1lost 3". The fix puts a
    // non-breaking space in the text itself (the row is one truncating line, so it never wraps),
    // which is why this asserts on an escaped \u00a0 rather than a plain space.
    const wrapper = mount(LifeEventList, {
      props: {
        events: [event({ id: 1, player_id: 1, delta: -3, life_after: 37 })],
        seats: [seat({ id: 1, position: 0 })],
        startedAt: START,
        undoable: false,
      },
    })
    expect(wrapper.text()).toContain('Player 1\u00a0lost 3')
  })
})
