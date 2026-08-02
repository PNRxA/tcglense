import { computed } from 'vue'
import { defineStore } from 'pinia'
import { persistedNumberRef, persistedRef } from '@/lib/persistedRef'
import { defaultLayoutFor } from '@/lib/lifeLayout'
import { isCounterKind, type LifeCounterKind } from '@/lib/lifeCounters'
import { LIFE_LAYOUTS, type LifeLayout } from '@/lib/api/life'

/**
 * The life counter's remembered game *shape* — player count, starting life, format, seating and
 * which counters are tracked.
 *
 * A pod plays the same shape of game over and over: four players, 40 life, Commander, sitting
 * the same way. Remembering that turns setup from a form into a confirmation, and makes a
 * one-tap quick-start possible. Client state, so Pinia + localStorage (the `stores/cardSize`
 * idiom), never the server: it's a per-device preference, not part of any game's record.
 *
 * Seat names and deck links are deliberately **not** remembered — see `NewGameDialog`.
 */
export const useLifeSetupStore = defineStore('lifeSetup', () => {
  const clampPlayers = (value: number) => Math.min(6, Math.max(1, Math.round(value)))
  const clampLife = (value: number) => Math.min(9_999, Math.max(1, Math.round(value)))

  const storedPlayerCount = persistedNumberRef('tcglense_life_players', 4, clampPlayers)
  const storedStartingLife = persistedNumberRef('tcglense_life_starting', 40, clampLife)

  /**
   * Both counts are clamped on **write** as well as on read. `persistedNumberRef` sanitizes what
   * it reads back out of storage, but these are bound to number inputs — which hand back `''`
   * mid-edit — and the value is both persisted and sent to the API, where a non-number is a 422
   * and a stored `''` reads back as 0 (clamping to 1 life) on the next visit.
   */
  const guarded = (stored: typeof storedPlayerCount, clamp: (value: number) => number) =>
    computed<number>({
      get: () => stored.value,
      set: (value) => {
        // `Number('')` is 0, not NaN, so an empty input has to be rejected by hand — clamping it
        // would silently mean "1 player, 1 life" rather than "leave it alone".
        const raw: unknown = value
        if (raw === '' || raw === null || raw === undefined) return
        const parsed = Number(raw)
        if (Number.isFinite(parsed)) stored.value = clamp(parsed)
      },
    })

  const playerCount = guarded(storedPlayerCount, clampPlayers)
  const startingLife = guarded(storedStartingLife, clampLife)
  const format = persistedRef<string>(
    'tcglense_life_format',
    'commander',
    // Free text (it mirrors `decks.format`), so anything storable is valid — but bound the
    // length so a hand-edited key can't push an oversized field at the API.
    (value): value is string => typeof value === 'string' && value.length <= 50,
  )
  const storedLayout = persistedRef<string>(
    'tcglense_life_layout',
    defaultLayoutFor(4),
    (value): value is string =>
      typeof value === 'string' && (LIFE_LAYOUTS as readonly string[]).includes(value),
  )

  // A layout stored by another build (or for a different player count) must never reach the
  // API, so it's narrowed on read rather than trusted.
  const layout = computed<LifeLayout>({
    get: () =>
      (LIFE_LAYOUTS as readonly string[]).includes(storedLayout.value)
        ? (storedLayout.value as LifeLayout)
        : defaultLayoutFor(playerCount.value),
    set: (value) => {
      storedLayout.value = value
    },
  })

  // Stored as the same CSV the server keeps `life_sessions.counters` in, because `persistedRef`
  // persists with `String(value)` and validates the raw stored *string* — handing it an array
  // would write `"poison,energy"` and then read it back through an `Array.isArray` guard that
  // is always false, silently reverting the preference on every reload.
  const storedCounters = persistedRef<string>(
    'tcglense_life_counters',
    // A pod that plays Commander (the remembered default format) is counting commander damage,
    // which is the same conclusion `default_counters_for` reaches server-side.
    'commander_damage',
    (value): value is string => typeof value === 'string' && value.length <= 100,
  )

  // Narrowed on read for the same reason `layout` is: a slug stored by another build must never
  // reach the API, where it's a 422.
  const counters = computed<LifeCounterKind[]>({
    get: () => storedCounters.value.split(',').filter(isCounterKind),
    set: (value) => {
      storedCounters.value = value.join(',')
    },
  })

  return { playerCount, startingLife, format, layout, counters }
})
