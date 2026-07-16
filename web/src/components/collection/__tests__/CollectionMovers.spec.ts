import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { mount } from '@vue/test-utils'
import type {
  Card,
  CollectionMover,
  CollectionMoverList,
  CollectionMovers as CollectionMoversResponse,
  CollectionSealedMover,
  CollectionSealedMoverList,
  Product,
} from '@/lib/api'
import CollectionMovers from '../CollectionMovers.vue'

const h = vi.hoisted(() => ({ query: {} as Record<string, unknown> }))
vi.mock('@/composables/useCollection', () => ({
  useCollectionMoversQuery: () => h.query,
}))

const cardMover: CollectionMover = {
  card: { id: 'card-1', name: 'Singles winner' } as Card,
  quantity: 1,
  foil_quantity: 0,
  value_now: '12.00',
  value_prev: '10.00',
  change_usd: '2.00',
  change_pct: 20,
}
const sealedMover: CollectionSealedMover = {
  product: { id: 'product-1', name: 'Sealed winner' } as Product,
  quantity: 1,
  foil_quantity: 0,
  value_now: '60.00',
  value_prev: '50.00',
  change_usd: '10.00',
  change_pct: 20,
}

function cardList(): CollectionMoverList {
  return { gainers: [cardMover], losers: [] }
}
function sealedList(): CollectionSealedMoverList {
  return { gainers: [sealedMover], losers: [] }
}

const movers: CollectionMoversResponse = {
  as_of: '2026-07-10',
  day_as_of: '2026-07-09',
  day: cardList(),
  week: cardList(),
  month: cardList(),
  year: cardList(),
  two_year: cardList(),
  three_year: cardList(),
  all_time: cardList(),
  sealed: {
    as_of: '2026-07-11',
    day_as_of: '2026-07-08',
    day: sealedList(),
    week: sealedList(),
    month: sealedList(),
    year: sealedList(),
    two_year: sealedList(),
    three_year: sealedList(),
    all_time: sealedList(),
  },
}

// Mirrors the component's Intl formatting so the expected labels track the host locale
// (a hard-coded 'Jul 10' only passes on machines that resolve to a US-style locale).
function asOfLabel(iso: string) {
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(
    new Date(`${iso}T00:00:00`),
  )
}

describe('CollectionMovers holding-kind switch', () => {
  it('switches the shared gainers/losers panel between singles and sealed products', async () => {
    h.query = {
      data: ref(movers),
      isPending: ref(false),
      isError: ref(false),
    }
    const wrapper = mount(CollectionMovers, {
      props: { game: 'mtg' },
      global: {
        stubs: {
          MoverRow: {
            props: ['mover'],
            template:
              '<div class="mover-stub">{{ mover.card ? mover.card.name : mover.product.name }}</div>',
          },
        },
      },
    })

    expect(wrapper.text()).toContain('Singles winner')
    expect(wrapper.text()).not.toContain('Sealed winner')
    expect(wrapper.text()).toContain(`as of ${asOfLabel('2026-07-10')}`)

    const day = wrapper.findAll('button').find((button) => button.text() === '1D')!
    await day.trigger('click')

    expect(wrapper.text()).toContain(`as of ${asOfLabel('2026-07-09')}`)

    const sealed = wrapper.findAll('button').find((button) => button.text() === 'Sealed')!
    await sealed.trigger('click')

    expect(wrapper.text()).toContain('Sealed winner')
    expect(wrapper.text()).not.toContain('Singles winner')
    // The 1D window survives the kind switch, so this reads the sealed day_as_of…
    expect(wrapper.text()).toContain(`as of ${asOfLabel('2026-07-08')}`)

    // …while a non-day window reads the sealed series' own as_of.
    const week = wrapper.findAll('button').find((button) => button.text() === '7D')!
    await week.trigger('click')

    expect(wrapper.text()).toContain(`as of ${asOfLabel('2026-07-11')}`)
  })
})
