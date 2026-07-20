import { computed, type Ref } from 'vue'
import type { CardSet } from '@/lib/api'

/**
 * Set-release markers for the price-history chart (issue: price-graph set indicators).
 *
 * Overlays "a set dropped here" flags on a price/value chart so a jump or dip lines up
 * against the release that may have caused it (fresh demand for new cards, or a reprint
 * supply shock). The chart hands us the game's full set list plus the plotted date window
 * and the rendered plot geometry; we return the releases that fall inside the window,
 * positioned in pixels so the caller can drop a set logo on each. Pure + DOM-free (the
 * geometry is passed in), so the selection and layout are unit-testable without the
 * unovis chart body — matching `useSeriesToggle`.
 */

/**
 * Set types worth flagging as a market-moving "drop": new expansions and core sets (fresh
 * card demand + supply), the big Commander / draft-innovation supplements, and reprint
 * (Masters) sets (a supply shock on existing cards). Narrower supplements — promos, tokens,
 * duel decks, from-the-vault, … — are deliberately left off so a multi-year window stays
 * legible rather than becoming a picket fence of logos.
 */
export const NOTABLE_SET_TYPES: ReadonlySet<string> = new Set([
  'core',
  'expansion',
  'draft_innovation',
  'masters',
  'commander',
])

/**
 * Tie-break priority when two notable sets share a release date (a Commander deck ships
 * alongside its expansion, so their dates collide). Lower wins, so the headline expansion /
 * core set keeps the marker and its same-day supplement is folded into it — one flag per
 * release event, not a cluster.
 */
const TYPE_PRIORITY: Record<string, number> = {
  core: 0,
  expansion: 1,
  draft_innovation: 2,
  masters: 3,
  commander: 4,
}

/** A notable set release inside the chart's plotted window. */
export interface SetMarker {
  /** Set code (lowercase), for the icon proxy URL + a stable v-for key. */
  code: string
  /** Full set name, for the marker's hover label. */
  name: string
  /** Release date as the stored `YYYY-MM-DD` string, for the hover label. */
  released: string
  /** Release date as epoch ms — the x value the plotline/logo is drawn at. */
  x: number
  /** Whether the set has an icon to show (else the caller draws a plain cap). */
  hasIcon: boolean
}

/** A [`SetMarker`] placed horizontally within the plot area. */
export interface PositionedSetMarker extends SetMarker {
  /** Pixel offset from the chart container's left edge for the marker's logo. */
  left: number
  /** False when a prior logo sits too close — the plotline still draws, but its logo is
   * suppressed so adjacent releases don't overlap into an unreadable smear. */
  showIcon: boolean
}

/** Rendered plot geometry + the pinned x-domain, all the pixel math needs. */
export interface PlotGeometry {
  /** Plot area's left edge, in px from the container's left (axis + margin width). */
  marginLeft: number
  /** Plot area width in px (the drawable band the x-domain maps across). */
  plotWidth: number
  /** Domain start (epoch ms) — the chart's earliest plotted day. */
  xMin: number
  /** Domain end (epoch ms) — the chart's latest plotted day. */
  xMax: number
}

/**
 * The notable set releases inside `[xMin, xMax]`, one per release date (same-day supplements
 * folded into their headline set by [`TYPE_PRIORITY`]), oldest first. A set with no release
 * date, an unparseable date, a non-notable type, or a date outside the window is dropped.
 */
export function selectSetMarkers(
  sets: readonly CardSet[],
  xMin: number,
  xMax: number,
): SetMarker[] {
  if (!(xMax > xMin)) return []
  // Keep the highest-priority set per release date.
  const byDate = new Map<string, CardSet>()
  for (const set of sets) {
    if (!set.released_at || !set.set_type || !NOTABLE_SET_TYPES.has(set.set_type)) continue
    const x = Date.parse(set.released_at)
    if (!Number.isFinite(x) || x < xMin || x > xMax) continue
    const existing = byDate.get(set.released_at)
    if (
      !existing ||
      (TYPE_PRIORITY[set.set_type] ?? Infinity) <
        (TYPE_PRIORITY[existing.set_type ?? ''] ?? Infinity)
    ) {
      byDate.set(set.released_at, set)
    }
  }
  return [...byDate.values()]
    .map((set) => ({
      code: set.code,
      name: set.name,
      released: set.released_at as string,
      x: Date.parse(set.released_at as string),
      hasIcon: !!set.icon_svg_uri,
    }))
    .sort((a, b) => a.x - b.x)
}

/**
 * Place each marker horizontally within the plot and thin out logos that would collide.
 * Markers must be oldest-first (so `left` ascends); the greedy pass keeps a logo only when
 * it clears the previous kept logo by `minGapPx`, leaving the plotline to still mark the
 * suppressed ones.
 */
export function positionSetMarkers(
  markers: readonly SetMarker[],
  geo: PlotGeometry,
  minGapPx = 30,
): PositionedSetMarker[] {
  const span = geo.xMax - geo.xMin
  if (geo.plotWidth <= 0 || span <= 0) return []
  let lastIconLeft = Number.NEGATIVE_INFINITY
  return markers.map((m) => {
    const left = geo.marginLeft + ((m.x - geo.xMin) / span) * geo.plotWidth
    const showIcon = m.hasIcon && left - lastIconLeft >= minGapPx
    if (showIcon) lastIconLeft = left
    return { ...m, left, showIcon }
  })
}

/**
 * Reactive glue: given the game's sets, the plotted x-window, and the rendered geometry
 * (null until the chart's first `onRenderComplete`), derive the positioned markers. Returns
 * an empty list until geometry is known, so the overlay simply doesn't paint pre-layout.
 */
export function useChartSetMarkers(
  sets: Ref<readonly CardSet[]>,
  xMin: Ref<number>,
  xMax: Ref<number>,
  geometry: Ref<PlotGeometry | null>,
) {
  const markers = computed(() => selectSetMarkers(sets.value, xMin.value, xMax.value))
  const positioned = computed(() =>
    geometry.value ? positionSetMarkers(markers.value, geometry.value) : [],
  )
  return { markers, positioned }
}
