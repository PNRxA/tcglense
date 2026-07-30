import { computed } from 'vue'
import { defineStore } from 'pinia'
import { persistedNumberRef, persistedRef } from '@/lib/persistedRef'
import { defaultLayoutFor } from '@/lib/lifeLayout'
import { LIFE_LAYOUTS, type LifeLayout } from '@/lib/api/life'

/**
 * The life counter's remembered game *shape* — player count, starting life, format and seating.
 *
 * A pod plays the same shape of game over and over: four players, 40 life, Commander, sitting
 * the same way. Remembering that turns setup from a form into a confirmation, and makes a
 * one-tap quick-start possible. Client state, so Pinia + localStorage (the `stores/cardSize`
 * idiom), never the server: it's a per-device preference, not part of any game's record.
 *
 * Seat names and deck links are deliberately **not** remembered — see `NewGameDialog`.
 */
export const useLifeSetupStore = defineStore('lifeSetup', () => {
  const playerCount = persistedNumberRef('tcglense_life_players', 4, (value) =>
    Math.min(6, Math.max(1, Math.round(value))),
  )
  const startingLife = persistedNumberRef('tcglense_life_starting', 40, (value) =>
    Math.min(9_999, Math.max(1, Math.round(value))),
  )
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

  return { playerCount, startingLife, format, layout }
})
