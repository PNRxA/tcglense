import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { useSetCollectionEntryMutation } from '@/composables/useCollection'

/** The authoritative owned counts to seed an editor from (the server holding). */
export interface OwnedCountSeed {
  quantity: number
  foil_quantity: number
}

/**
 * Local, instantly-updated regular/foil counts for a card with a debounced + serialized
 * save to {@link useSetCollectionEntryMutation} (which writes *absolute* counts). Extracted
 * from CollectionControls so the card-detail steppers and the browse/collection-grid
 * quick-add control share one copy of the tricky bits:
 *
 * - `dirty` marks a local edit not yet reflected by the server, so a background refetch
 *   never clobbers an in-progress change (the `seed` watch reseeds only when clean).
 * - saves are debounced (a trailing save after a short pause) and serialized (each waits
 *   for the previous), so rapid `+ + +` collapses into one PUT of the final value.
 * - an edit-generation counter keeps a late save from clearing the dirty flag out from
 *   under a newer pending edit.
 * - a pending save is flushed on unmount so navigating away mid-edit doesn't drop it.
 *
 * `seed` is `undefined` until the authoritative holding has loaded; because writes are
 * absolute, callers should keep the +/- disabled until it resolves so an early click
 * can't save an adjustment off a stale zero.
 */
export function useOwnedCountEditor(
  game: Ref<string>,
  cardId: Ref<string>,
  seed: Ref<OwnedCountSeed | undefined>,
) {
  const mutation = useSetCollectionEntryMutation()

  const regular = ref(0)
  const foil = ref(0)
  const dirty = ref(false)
  const saveError = ref(false)

  // Seed from the server holding whenever it (re)loads, unless a local edit is pending.
  watch(
    seed,
    (value) => {
      if (value && !dirty.value) {
        regular.value = value.quantity
        foil.value = value.foil_quantity
      }
    },
    { immediate: true },
  )

  // Switching to a different card starts fresh.
  watch(cardId, () => {
    dirty.value = false
    saveError.value = false
  })

  let timer: ReturnType<typeof setTimeout> | null = null
  let inFlight: Promise<unknown> = Promise.resolve()
  let editGen = 0

  function runSave() {
    const gen = editGen
    return mutation
      .mutateAsync({
        game: game.value,
        id: cardId.value,
        quantity: regular.value,
        foil_quantity: foil.value,
      })
      .then(() => {
        saveError.value = false
      })
      .catch(() => {
        saveError.value = true
      })
      .finally(() => {
        // Only clear dirty if no further edit happened while this save ran, so the
        // pending edit's own save (and reseed) stays authoritative.
        if (gen === editGen) dirty.value = false
      })
  }

  function save() {
    inFlight = inFlight.then(runSave)
  }

  function scheduleSave() {
    dirty.value = true
    editGen += 1
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      save()
    }, 350)
  }

  onBeforeUnmount(() => {
    // Flush a pending edit so a quick navigation away doesn't drop the last change.
    if (timer) {
      clearTimeout(timer)
      timer = null
      save()
    }
  })

  function adjust(which: 'quantity' | 'foil', delta: number) {
    if (which === 'quantity') regular.value = Math.max(0, regular.value + delta)
    else foil.value = Math.max(0, foil.value + delta)
    scheduleSave()
  }

  // A save is outstanding while the mutation is in flight or an edit is still debouncing.
  const saving = computed(() => mutation.isPending.value || dirty.value)

  return { regular, foil, adjust, dirty, saving, saveError }
}
