import type { Ref } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { getReleaseCalendar, type ReleaseWindow } from '@/lib/api'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'

/**
 * The release calendar read (issue #679). A PUBLIC catalog endpoint — the same for every
 * visitor — so plain `useQuery`, with the game and the window inside the key so a change
 * refetches. The window is a plain value rather than a ref: the view derives it once from
 * the day it mounted (month-aligned, see `lib/releases.ts`), and a calendar that re-windowed
 * itself mid-view would shift its own sections under the reader.
 *
 * Priced stale time, not the structural one: the nested sealed products carry live prices.
 */
export function useReleaseCalendarQuery(game: Ref<string>, window: ReleaseWindow) {
  return useQuery({
    queryKey: ['releases', game, window.from, window.to],
    queryFn: ({ signal }) => getReleaseCalendar(game.value, window, signal),
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}
