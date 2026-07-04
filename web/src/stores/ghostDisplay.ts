import { defineStore } from 'pinia'
import { DEFAULT_GHOST_STYLE, isGhostStyle, type GhostStyle } from '@/lib/ghostDisplay'
import { persistedBoolRef, persistedRef } from '@/lib/persistedRef'

// Display preferences for the show-ghosts browse mode, adjusted from the ghost button's
// settings dropdown (issue #213). Personal, like the card size / theme, so they live in
// localStorage and apply across every ghost view rather than in the per-list URL state.
const STYLE_KEY = 'tcglense_ghost_style'
const SHOW_OWNED_KEY = 'tcglense_ghost_show_owned'

export const useGhostDisplayStore = defineStore('ghostDisplay', () => {
  // How ghosts are desaturated: grayscale (default) or full colour.
  const style = persistedRef<GhostStyle>(STYLE_KEY, DEFAULT_GHOST_STYLE, isGhostStyle)

  // Wish-list only: also flag which cards on the page you already own in your collection
  // (off by default). The setting is shared plumbing, but only the wish-list views read it.
  const showOwned = persistedBoolRef(SHOW_OWNED_KEY, false)

  function setStyle(next: GhostStyle) {
    style.value = next
  }

  function setShowOwned(next: boolean) {
    showOwned.value = next
  }

  return { style, showOwned, setStyle, setShowOwned }
})
