import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import type { ProductCardSection, ProductComponent } from '@/lib/api'
import ProductOverview from '../ProductOverview.vue'

// Drive the strip off controlled query state, stubbing the three composables so no QueryClient
// is needed. The unit under test is the wiring the chips ride: which chips exist for a given
// manifest, in what order, what each says, and that every card chip still jumps to the card
// section. The counting + labelling rules themselves live in lib/productCounts.ts and are
// tested there.
const state = vi.hoisted(() => ({
  sections: [] as ProductCardSection[],
  components: [] as ProductComponent[],
  containers: [] as unknown[],
}))

vi.mock('@/composables/useProducts', () => ({
  useProductCardSectionsQuery: () => ({
    data: {
      get value() {
        return { data: state.sections }
      },
    },
  }),
  useProductContentsQuery: () => ({
    data: {
      get value() {
        return { data: state.components }
      },
    },
  }),
  useProductContainersQuery: () => ({
    data: {
      get value() {
        return { data: state.containers }
      },
    },
  }),
}))

const section = (key: string, total = 1, boosterFamily: string | null = null): ProductCardSection =>
  ({
    key,
    total,
    booster_family: boosterFamily,
    component: null,
    inherited: false,
  }) as ProductCardSection

const comp = (name: string, quantity: number) =>
  ({ kind: 'sealed', name, quantity, product: null, card: null }) as ProductComponent

function mountStrip(
  opts: {
    sections?: ProductCardSection[]
    components?: ProductComponent[]
    containers?: number
  } = {},
) {
  state.sections = opts.sections ?? []
  state.components = opts.components ?? []
  state.containers = Array.from({ length: opts.containers ?? 0 }, () => ({}))
  const wrapper = mount(ProductOverview, { props: { game: 'mtg', id: '900001' } })
  return {
    wrapper,
    // The count and the label are separate spans (flex gap, no whitespace node between them),
    // so join them rather than reading the button's concatenated textContent.
    chips: wrapper.findAll('button').map((b) => ({
      text: b
        .findAll('span span')
        .map((s) => s.text().trim())
        .join(' ')
        .trim(),
      title: b.attributes('title') ?? '',
    })),
  }
}

beforeEach(() => {
  state.sections = []
  state.components = []
  state.containers = []
})

describe('ProductOverview', () => {
  it('renders nothing when no count is known yet', () => {
    expect(mountStrip().wrapper.find('button').exists()).toBe(false)
  })

  it('never announces a booster’s pull pool as cards inside it', () => {
    // The reported bug, in the chip that carried it: 600 is the pool, the pack holds ~15.
    const { chips } = mountStrip({ sections: [section('booster', 600)] })
    expect(chips.map((c) => c.text)).toEqual(['600 cards in the pull pool'])
    expect(chips[0]!.text).not.toContain('inside')
    expect(chips[0]!.title).toContain("not one pack's worth")
  })

  it('splits a mixed manifest into one chip per certainty, in descending certainty', () => {
    const { chips } = mountStrip({
      sections: [
        section('contains', 3),
        section('exclusive', 8, 'collector_pack'),
        section('booster', 52),
        section('variable', 2),
      ],
      components: [comp('Collector Booster', 12)],
      containers: 1,
    })
    expect(chips.map((c) => c.text)).toEqual([
      '12 items in the box',
      '3 guaranteed cards',
      // 8 + 52 = the whole pool; the exclusives are a slice of it, never added to it.
      '60 cards in the pull pool',
      '8 of the pool, exclusive to Collector Booster',
      '2 cards it might include',
      '1 product includes this',
    ])
  })

  it('keeps every card chip self-contained, so a wrapped row loses no antecedent', () => {
    const { chips } = mountStrip({
      sections: [section('exclusive', 8, 'collector_pack'), section('booster', 52)],
    })
    expect(chips.every((c) => !c.text.includes('of them'))).toBe(true)
  })

  it('goes generic when the backend names no booster family', () => {
    const { chips } = mountStrip({
      sections: [section('exclusive', 8), section('booster', 52)],
    })
    expect(chips[1]!.text).toBe("8 of the pool, exclusive to this product's boosters")
  })

  it('drops the exclusives chip when the whole pool is exclusive', () => {
    const { chips } = mountStrip({ sections: [section('exclusive', 8, 'collector_pack')] })
    expect(chips.map((c) => c.text)).toEqual(['8 cards in the pull pool'])
  })

  it('counts box pieces, not line items', () => {
    // 12 packs + 1 topper is 13 pieces; the pre-fix count said "2 items".
    const { chips } = mountStrip({
      components: [comp('Play Booster', 12), comp('Box Topper', 1)],
    })
    expect(chips.map((c) => c.text)).toEqual(['13 items in the box'])
  })

  it('jumps every card chip to the card section, and the others to their own', () => {
    const { wrapper } = mountStrip({
      sections: [section('contains', 1), section('booster', 2), section('variable', 1)],
      components: [comp('Pack', 1)],
      containers: 1,
    })
    const buttons = wrapper.findAll('button')
    buttons.forEach((b) => b.trigger('click'))
    expect(wrapper.emitted('jump')?.flat()).toEqual([
      'contents',
      'cards',
      'cards',
      'cards',
      'containers',
    ])
  })

  it('keeps the jump affordance in the tooltip even when a chip adds a hint', () => {
    const { chips } = mountStrip({ sections: [section('booster', 600)] })
    expect(chips[0]!.title).toMatch(/^Jump to 600 cards in the pull pool — /)
  })

  it('leaves an inherited pool out of the chips — the sections below hide it too', () => {
    // A bundle whose whole pool arrived through its linked boosters: the chip strip must
    // agree with ProductCards (both read visibleProductSections), or a chip would jump to
    // a section that isn't there.
    const { chips } = mountStrip({
      sections: [
        section('contains', 2),
        { ...section('exclusive', 80, 'collector_pack'), inherited: true },
        { ...section('booster', 520), inherited: true },
      ],
    })
    expect(chips.map((c) => c.text)).toEqual(['2 guaranteed cards'])
  })

  it('counts an unlisted component’s cards into the certainty chips', () => {
    const { chips } = mountStrip({
      sections: [
        section('contains', 1),
        { ...section('contains', 5), component: 'Land Pack' },
        { ...section('variable', 1), component: 'Land Pack' },
      ],
    })
    expect(chips.map((c) => c.text)).toEqual(['6 guaranteed cards', '1 card it might include'])
  })

  it('renders an icon for every chip kind', () => {
    const { wrapper } = mountStrip({
      sections: [
        section('contains', 1),
        section('exclusive', 1, 'collector_pack'),
        section('booster', 2),
        section('variable', 1),
      ],
      components: [comp('Pack', 1)],
      containers: 1,
    })
    // A CHIP_ICONS key that didn't resolve would render nothing for its chip.
    expect(wrapper.findAll('button svg')).toHaveLength(wrapper.findAll('button').length)
  })
})
