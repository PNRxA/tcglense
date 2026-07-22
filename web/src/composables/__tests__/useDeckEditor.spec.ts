import { describe, expect, it, vi } from 'vitest'
import { makeCard } from '@/test/fixtures'
import type { DeckDetail } from '@/lib/api'

// The deck the mocked query returns: two populated sections + one empty, with the
// populated ones holding different cards so a filter can hide one of them.
const deck: DeckDetail = {
  id: 7,
  game: 'mtg',
  name: 'Test Deck',
  description: null,
  format: null,
  folder_id: null,
  is_public: false,
  handle: null,
  summary: { unique_cards: 3, total_cards: 3, total_value_usd: null, bulk_value_usd: null },
  sections: [
    { id: 1, name: 'Creatures', position: 0 },
    { id: 2, name: 'Lands', position: 1 },
    { id: 3, name: 'Sideboard', position: 2 },
  ],
  cards: [
    {
      card: makeCard('goblin', { name: 'Goblin Guide' }),
      section_id: 1,
      quantity: 1,
      foil_quantity: 0,
    },
    {
      card: makeCard('bear', { name: 'Grizzly Bears' }),
      section_id: 1,
      quantity: 1,
      foil_quantity: 0,
    },
    { card: makeCard('island', { name: 'Island' }), section_id: 2, quantity: 1, foil_quantity: 0 },
  ],
  created_at: '',
  updated_at: '',
}

const reorderMutateAsync = vi.hoisted(() => vi.fn<() => Promise<object>>(async () => ({})))

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  const mutation = () => ({
    mutateAsync: vi.fn<() => Promise<object>>(async () => ({})),
    isPending: ref(false),
  })
  return {
    useDeckQuery: () => ({ data: ref(deck), isPending: ref(false), isError: ref(false) }),
    useFoldersQuery: () => ({ data: ref({ data: [] }) }),
    useCreateSectionMutation: mutation,
    useDeleteDeckMutation: mutation,
    useDeleteSectionMutation: mutation,
    useMoveDeckToFolderMutation: mutation,
    useReorderSectionsMutation: () => ({ mutateAsync: reorderMutateAsync, isPending: false }),
    useSetDeckVisibilityMutation: mutation,
    useUpdateDeckMutation: mutation,
    useUpdateSectionMutation: mutation,
  }
})

vi.mock('@/composables/useCollection', async () => {
  const { ref } = await import('vue')
  return { useOwnedCounts: () => ({ ownership: ref({}) }) }
})
vi.mock('@/composables/useWishlist', async () => {
  const { ref } = await import('vue')
  return { useWishlistCounts: () => ({ ownership: ref({}) }) }
})
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({}) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn<() => void>() }) }))

import { useDeckEditor } from '../useDeckEditor'

describe('useDeckEditor with an active filter (issue #562)', () => {
  it('counts the whole section in the delete confirmation, not the filtered matches', () => {
    const editor = useDeckEditor({ game: 'mtg', id: '7' })
    editor.filterQuery.value = 'goblin'
    expect(editor.cardsBySection.value.get(1)).toHaveLength(1)

    editor.requestSectionDelete(1, 'Creatures')
    // Deleting moves BOTH entries, so the dialog must say 2 even though 1 matches.
    expect(editor.sectionDeleteTarget.value).toEqual({ id: 1, name: 'Creatures', count: 2 })
  })

  it('refuses to reorder sections while a filter narrows the visible list', () => {
    const editor = useDeckEditor({ game: 'mtg', id: '7' })
    reorderMutateAsync.mockClear()

    editor.filterQuery.value = 'goblin'
    editor.moveSection(1, 1)
    expect(reorderMutateAsync).not.toHaveBeenCalled()

    editor.clearFilters()
    editor.moveSection(1, 1)
    expect(reorderMutateAsync).toHaveBeenCalledOnce()
  })
})
