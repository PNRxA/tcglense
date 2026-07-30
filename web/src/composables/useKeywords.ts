import { computed, ref, type Ref } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { getKeywords, type KeywordEntry, type KeywordKind } from '@/lib/api'
import { keywordSlug, relatedKeywords } from '@/lib/keywords'

/** The rules-keyword glossary: one shared fetch behind the inline card-text tooltips
 * and the `/keywords` pages (issue: keyword tooltips). */

/** Every keyword the glossary game defines, name-ordered.
 *
 * One cache entry for the whole app: the card-text tooltips mount this once per card
 * page and the glossary pages read the same key, so hovering a keyword and then opening
 * its page costs no second request. `staleTime: Infinity` because the table is static
 * per API release — there is nothing to revalidate between deploys, and a card page
 * should never spend a request re-asking. */
export function useKeywordsQuery(game: Ref<string>) {
  return useQuery({
    // The game ref goes INSIDE the key (never `.value`), so navigating between games
    // refetches instead of serving the first one's glossary forever.
    queryKey: ['keywords', game],
    queryFn: () => getKeywords(game.value).then((response) => response.data),
    staleTime: Infinity,
  })
}

/** Just the entries, defaulting to an empty list while the query is in flight — the
 * shape the text matcher wants. */
export function useKeywordGlossary(game: Ref<string>) {
  const query = useKeywordsQuery(game)
  const entries = computed<KeywordEntry[]>(() => query.data.value ?? [])
  return { query, entries }
}

/** Filter/segment state and derived sections for the `/keywords` index page. */
export function useKeywordIndex(game: Ref<string>) {
  const query = useKeywordsQuery(game)
  const entries = computed<KeywordEntry[]>(() => query.data.value ?? [])

  const filter = ref('')
  const kind = ref<KeywordKind | 'all'>('all')
  const trimmed = computed(() => filter.value.trim().toLowerCase())
  const filtering = computed(() => trimmed.value.length > 0)

  /** Text-filtered but kind-agnostic, so the kind tabs can show honest counts for the
   * current search rather than counts of the whole glossary. */
  const searched = computed(() =>
    trimmed.value
      ? entries.value.filter(
          (entry) =>
            entry.name.toLowerCase().includes(trimmed.value) ||
            entry.text.toLowerCase().includes(trimmed.value),
        )
      : entries.value,
  )

  const kindCounts = computed<Record<KeywordKind | 'all', number>>(() => {
    const counts = { all: searched.value.length, ability: 0, action: 0, ability_word: 0 }
    for (const entry of searched.value) counts[entry.kind] += 1
    return counts
  })

  const filtered = computed(() =>
    kind.value === 'all'
      ? searched.value
      : searched.value.filter((entry) => entry.kind === kind.value),
  )

  /** The letter an entry files under; anything not starting a–z buckets together. */
  const letterOf = (entry: KeywordEntry) => {
    const initial = entry.name[0]?.toUpperCase() ?? '#'
    return initial >= 'A' && initial <= 'Z' ? initial : '#'
  }

  const sections = computed(() => {
    const groups = new Map<string, KeywordEntry[]>()
    for (const entry of filtered.value) {
      const letter = letterOf(entry)
      const group = groups.get(letter)
      if (group) group.push(entry)
      else groups.set(letter, [entry])
    }
    return [...groups.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([letter, group]) => ({
        letter,
        id: letter === '#' ? 'other' : letter.toLowerCase(),
        group,
      }))
  })

  /** The full A–Z strip, each letter flagged with whether the current filter left it
   * anything — so the jump bar keeps its shape instead of reflowing as you type. */
  const letters = computed(() => {
    const present = new Set(sections.value.map((section) => section.letter))
    const alphabet = Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i))
    return [...alphabet, '#'].map((letter) => ({
      letter,
      id: letter === '#' ? 'other' : letter.toLowerCase(),
      present: present.has(letter),
    }))
  })

  return {
    query,
    entries,
    filter,
    kind,
    filtering,
    kindCounts,
    filtered,
    sections,
    letters,
    total: computed(() => entries.value.length),
  }
}

/** One keyword's page: the entry itself plus the links out of it. */
export function useKeywordEntry(game: Ref<string>, slug: Ref<string>) {
  const query = useKeywordsQuery(game)
  const entries = computed<KeywordEntry[]>(() => query.data.value ?? [])

  // Normalise what's in the URL before looking it up, so `/keywords/First-Strike` finds
  // the entry (the page then redirects to its canonical slug).
  const wanted = computed(() => keywordSlug(slug.value))
  const entry = computed(() => entries.value.find((item) => item.slug === wanted.value))

  /** Settled with no match — not merely `isError`: a resolved-but-empty glossary would
   * otherwise sit on the loading state forever. The page's `noindex` hangs off this, so
   * an unknown slug is signalled as a soft 404 rather than indexed as a real page. */
  const notFound = computed(() => query.isError.value || (!entry.value && !query.isPending.value))

  const index = computed(() =>
    entry.value ? entries.value.findIndex((item) => item.slug === entry.value?.slug) : -1,
  )
  const previous = computed(() => (index.value > 0 ? entries.value[index.value - 1] : undefined))
  const next = computed(() =>
    index.value >= 0 && index.value < entries.value.length - 1
      ? entries.value[index.value + 1]
      : undefined,
  )
  const related = computed(() => (entry.value ? relatedKeywords(entry.value, entries.value) : []))

  return { query, entry, notFound, previous, next, related }
}
