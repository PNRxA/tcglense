import { nextTick, onMounted, ref, watch } from 'vue'

/**
 * Load/fail state for a lazily-loaded `<img>`, shared by CardImage and SetTile. Bind
 * the returned `el` to the image (`ref="el"`) and wire `onLoad`/`onError` to its
 * events. A cached image can finish loading before the `load` listener attaches (so
 * its event never fires); we reflect the already-complete state on mount and after
 * each reset so the image never stays stuck invisible waiting for an event that won't
 * come.
 *
 * `resetKey` is a getter over the src-identity deps (card id/face/size, set code, …).
 * When it changes the `<img>` points at a new source, so the flags clear and we
 * re-check once the new src is in the DOM (it may resolve instantly from cache).
 */
export function useImageLoad(resetKey: () => unknown) {
  const el = ref<HTMLImageElement | null>(null)
  const loaded = ref(false)
  const failed = ref(false)

  function onLoad() {
    loaded.value = true
  }

  function onError() {
    failed.value = true
  }

  function sync() {
    const node = el.value
    if (node?.complete && node.naturalWidth > 0) loaded.value = true
  }

  onMounted(sync)

  watch(resetKey, () => {
    failed.value = false
    loaded.value = false
    nextTick(sync)
  })

  return { el, loaded, failed, onLoad, onError }
}
