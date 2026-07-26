import { computed, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getArtTags } from '@/lib/api'

/** Public art-tag lookups for the advanced-search panel (issue #140): typed-term
 * suggestions for the tag input, and the whole vocabulary for the tag browser. */

/** Minimum characters before the tag input queries for suggestions. */
export const ART_TAG_MIN_CHARS = 2

/** Art-tag hints for the panel's tag input. `term` is the (already debounced) text;
 * the query only runs once its trimmed length reaches {@link ART_TAG_MIN_CHARS}. */
export function useArtTagSuggestions(game: Ref<string>, term: Ref<string>) {
  const trimmed = computed(() => term.value.trim())
  const enabled = computed(() => trimmed.value.length >= ART_TAG_MIN_CHARS)
  return useQuery({
    queryKey: ['art-tags', game, trimmed],
    queryFn: () => getArtTags(game.value, trimmed.value),
    enabled,
    // Keep the last hints visible while the next keystroke's query resolves.
    placeholderData: keepPreviousData,
    // Tags change at most daily, so a short cache spares a refetch when the user
    // backspaces to a term they just typed.
    staleTime: 60_000,
  })
}

/** The game's whole tag vocabulary (a few thousand entries) for the tag-browser
 * dialog. Fetched only while the dialog is open (`enabled`), then kept for the
 * session — the vocabulary changes at most daily. */
export function useArtTagList(game: Ref<string>, enabled: Ref<boolean>) {
  return useQuery({
    queryKey: ['art-tags-all', game],
    queryFn: () => getArtTags(game.value),
    enabled,
    staleTime: 60 * 60_000,
  })
}
