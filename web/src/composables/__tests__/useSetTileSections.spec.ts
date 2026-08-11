import { describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import type { CardSet } from '@/lib/api'

// The engine reads the catalog set list for each set's icon + release date; stub that one query
// so the sectioning itself is what's under test (the composable is otherwise pure).
const catalogSets = ref<CardSet[]>([])
vi.mock('@/composables/useCatalog', () => ({
  useSetsQuery: () => ({
    data: computed(() => ({ data: catalogSets.value })),
    isPending: computed(() => false),
  }),
}))

const { useSetTileSections } = await import('@/composables/useSetTileSections')

function catalogSet(code: string, releasedAt: string | null): CardSet {
  return {
    code,
    name: code.toUpperCase(),
    released_at: releasedAt,
    set_type: 'expansion',
    card_count: 0,
    icon_svg_uri: null,
    parent_set_code: null,
    has_drops: false,
    has_subtypes: false,
  }
}

interface Facet {
  code: string
  name: string | null
  count: number
}

function setup(facets: Facet[], catalog: CardSet[]) {
  catalogSets.value = catalog
  return useSetTileSections(ref('mtg'), ref(facets))
}

describe('useSetTileSections', () => {
  it('buckets sets into release-year sections, newest year first', () => {
    const { sections } = setup(
      [
        { code: 'old', name: 'Old Set', count: 1 },
        { code: 'new', name: 'New Set', count: 2 },
        { code: 'mid', name: 'Mid Set', count: 3 },
      ],
      [
        catalogSet('old', '2021-03-01'),
        catalogSet('new', '2026-06-01'),
        catalogSet('mid', '2024-01-15'),
      ],
    )
    expect(sections.value.map((s) => s.label)).toEqual(['2026', '2024', '2021'])
    expect(sections.value[0]!.sets.map((s) => s.code)).toEqual(['new'])
  })

  it('sinks a set with no catalog row (or no date) into a trailing Unknown year section', () => {
    const { sections } = setup(
      [
        { code: 'dated', name: 'Dated', count: 1 },
        { code: 'ghost', name: 'Ghost', count: 1 },
      ],
      [catalogSet('dated', '2024-01-15')],
    )
    expect(sections.value.map((s) => s.label)).toEqual(['2024', 'Unknown year'])
    const last = sections.value[sections.value.length - 1]!
    expect(last.sets.map((s) => s.code)).toEqual(['ghost'])
  })

  it('leads with a Featured section for a pinned set, whatever its date', () => {
    // Secret Lair is pinned: its 2019 date would otherwise bury a line that restocks forever.
    const { sections } = setup(
      [
        { code: 'sld', name: 'Secret Lair Drop', count: 9 },
        { code: 'new', name: 'New Set', count: 1 },
      ],
      [catalogSet('sld', '2019-12-02'), catalogSet('new', '2026-06-01')],
    )
    expect(sections.value.map((s) => s.label)).toEqual(['Featured', '2026'])
    expect(sections.value[0]!.sets.map((s) => s.code)).toEqual(['sld'])
  })

  it('filters by name or code, and a filter that excludes the pinned set drops Featured', () => {
    const engine = setup(
      [
        { code: 'sld', name: 'Secret Lair Drop', count: 9 },
        { code: 'tmc', name: 'Ninja Turtles', count: 1 },
      ],
      [catalogSet('sld', '2019-12-02'), catalogSet('tmc', '2026-03-06')],
    )
    engine.filter.value = '  turtles '
    expect(engine.filtering.value).toBe(true)
    expect(engine.trimmedFilter.value).toBe('turtles')
    expect(engine.filteredSets.value.map((s) => s.code)).toEqual(['tmc'])
    expect(engine.sections.value.map((s) => s.label)).toEqual(['2026'])

    // Code matches too, case-insensitively.
    engine.filter.value = 'SLD'
    expect(engine.filteredSets.value.map((s) => s.code)).toEqual(['sld'])
    expect(engine.sections.value.map((s) => s.label)).toEqual(['Featured'])
  })

  it('orders sets within a year newest-release first, then by code', () => {
    const { sections } = setup(
      [
        { code: 'bbb', name: 'B', count: 1 },
        { code: 'aaa', name: 'A', count: 1 },
        { code: 'ccc', name: 'C', count: 1 },
      ],
      [
        catalogSet('bbb', '2024-01-01'),
        catalogSet('aaa', '2024-01-01'),
        catalogSet('ccc', '2024-09-01'),
      ],
    )
    expect(sections.value[0]!.sets.map((s) => s.code)).toEqual(['ccc', 'aaa', 'bbb'])
  })
})
