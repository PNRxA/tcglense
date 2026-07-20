import { describe, it, expect } from 'vitest'
import { ref } from 'vue'
import type { CardSet } from '@/lib/api'
import {
  NOTABLE_SET_TYPES,
  positionSetMarkers,
  selectSetMarkers,
  useChartSetMarkers,
  type PlotGeometry,
  type SetMarker,
} from '../useChartSetMarkers'

function makeSet(over: Partial<CardSet> & Pick<CardSet, 'code'>): CardSet {
  return {
    name: over.code.toUpperCase(),
    set_type: 'expansion',
    released_at: '2024-06-01',
    card_count: 100,
    icon_svg_uri: 'https://example.test/icon.svg',
    parent_set_code: null,
    has_drops: false,
    has_subtypes: false,
    ...over,
  }
}

const ms = (date: string) => Date.parse(date)

describe('selectSetMarkers', () => {
  const window = { min: ms('2024-01-01'), max: ms('2024-12-31') }

  it('keeps notable releases inside the window, oldest first', () => {
    const sets = [
      makeSet({ code: 'a', set_type: 'expansion', released_at: '2024-08-01' }),
      makeSet({ code: 'b', set_type: 'core', released_at: '2024-03-01' }),
    ]
    const markers = selectSetMarkers(sets, window.min, window.max)
    expect(markers.map((m) => m.code)).toEqual(['b', 'a'])
    expect(markers[0]).toMatchObject({
      code: 'b',
      name: 'B',
      released: '2024-03-01',
      hasIcon: true,
    })
    expect(markers[0]!.x).toBe(ms('2024-03-01'))
  })

  it('drops sets outside the window, with no date, or of a non-notable type', () => {
    const sets = [
      makeSet({ code: 'before', released_at: '2023-06-01' }),
      makeSet({ code: 'after', released_at: '2025-06-01' }),
      makeSet({ code: 'nodate', released_at: null }),
      makeSet({ code: 'promo', set_type: 'promo', released_at: '2024-06-01' }),
      makeSet({ code: 'token', set_type: 'token', released_at: '2024-06-01' }),
      makeSet({ code: 'keep', set_type: 'masters', released_at: '2024-06-01' }),
    ]
    expect(selectSetMarkers(sets, window.min, window.max).map((m) => m.code)).toEqual(['keep'])
  })

  it('folds same-day releases into the highest-priority set', () => {
    // A Commander deck ships with its expansion on the same day — one marker, the expansion.
    const sets = [
      makeSet({ code: 'cmd', set_type: 'commander', released_at: '2024-08-02' }),
      makeSet({ code: 'exp', set_type: 'expansion', released_at: '2024-08-02' }),
    ]
    const markers = selectSetMarkers(sets, window.min, window.max)
    expect(markers.map((m) => m.code)).toEqual(['exp'])
  })

  it('marks hasIcon false when the set has no icon', () => {
    const sets = [makeSet({ code: 'x', released_at: '2024-06-01', icon_svg_uri: null })]
    expect(selectSetMarkers(sets, window.min, window.max)[0]!.hasIcon).toBe(false)
  })

  it('returns nothing for a degenerate window', () => {
    const sets = [makeSet({ code: 'a', released_at: '2024-06-01' })]
    expect(selectSetMarkers(sets, window.max, window.min)).toEqual([])
    expect(selectSetMarkers(sets, window.min, window.min)).toEqual([])
  })

  it('exposes the notable set types', () => {
    expect(NOTABLE_SET_TYPES.has('expansion')).toBe(true)
    expect(NOTABLE_SET_TYPES.has('promo')).toBe(false)
  })
})

describe('positionSetMarkers', () => {
  const geo: PlotGeometry = {
    marginLeft: 40,
    plotWidth: 200,
    xMin: ms('2024-01-01'),
    xMax: ms('2024-12-31'),
  }
  const marker = (code: string, date: string, hasIcon = true): SetMarker => ({
    code,
    name: code,
    released: date,
    x: ms(date),
    hasIcon,
  })

  it('maps the domain edges to the plot edges', () => {
    const placed = positionSetMarkers(
      [marker('start', '2024-01-01'), marker('end', '2024-12-31')],
      geo,
      0,
    )
    expect(placed[0]!.left).toBeCloseTo(40)
    expect(placed[1]!.left).toBeCloseTo(240)
  })

  it('suppresses a logo that collides with a kept neighbour but keeps its line', () => {
    const placed = positionSetMarkers(
      [marker('a', '2024-06-01'), marker('b', '2024-06-05')],
      geo,
      24,
    )
    expect(placed[0]!.showIcon).toBe(true)
    // Only a few px apart at this scale — the second logo is hidden, its plotline still drawn.
    expect(placed[1]!.showIcon).toBe(false)
    expect(placed.map((m) => m.code)).toEqual(['a', 'b'])
  })

  it('never shows a logo for an icon-less marker', () => {
    expect(positionSetMarkers([marker('a', '2024-06-01', false)], geo, 24)[0]!.showIcon).toBe(false)
  })

  it('returns nothing when the plot has no width or span', () => {
    expect(positionSetMarkers([marker('a', '2024-06-01')], { ...geo, plotWidth: 0 })).toEqual([])
    expect(positionSetMarkers([marker('a', '2024-06-01')], { ...geo, xMax: geo.xMin })).toEqual([])
  })
})

describe('useChartSetMarkers', () => {
  it('stays empty until geometry is known, then positions markers', () => {
    const sets = ref<CardSet[]>([makeSet({ code: 'a', released_at: '2024-06-01' })])
    const xMin = ref(ms('2024-01-01'))
    const xMax = ref(ms('2024-12-31'))
    const geometry = ref<PlotGeometry | null>(null)
    const { markers, positioned } = useChartSetMarkers(sets, xMin, xMax, geometry)

    expect(markers.value.map((m) => m.code)).toEqual(['a'])
    expect(positioned.value).toEqual([])

    geometry.value = { marginLeft: 40, plotWidth: 200, xMin: xMin.value, xMax: xMax.value }
    expect(positioned.value).toHaveLength(1)
    expect(positioned.value[0]!.showIcon).toBe(true)
  })
})
