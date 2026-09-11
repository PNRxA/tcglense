import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  useAddDeckToCollectionMutation,
  useCopyDeckMutation,
  useCreateSectionMutation,
  useDeckQuery,
  useDeleteDeckMutation,
  useDeleteSectionMutation,
  useFoldersQuery,
  useMoveDeckToFolderMutation,
  useReorderSectionsMutation,
  useSetDeckVisibilityMutation,
  useUpdateDeckMutation,
  useUpdateSectionMutation,
} from '@/composables/useDecks'
import { useDeckCardDisplay } from '@/composables/useDeckCardDisplay'
import { useDeckOwnership } from '@/composables/useDeckOwnership'
import { useDeckRolesQuery } from '@/composables/useDeckAnalysis'
import { ApiError, exportDeckFile } from '@/lib/api'
import type { DeckCardEntry, DeckExportFormat } from '@/lib/api'
import { downloadBlob } from '@/lib/download'
import { useAuthStore } from '@/stores/auth'

interface DeckEditorProps {
  game: string
  id: string
}

/** Reactive engine for the owner deck view: load and group its cards, overlay collection and
 * wish-list counts, and coordinate deck metadata, export, sharing, and section mutations. */
export function useDeckEditor(props: DeckEditorProps) {
  const router = useRouter()
  const auth = useAuthStore()
  const game = computed(() => props.game)
  const deckId = computed(() => Number(props.id))
  const deckQuery = useDeckQuery(game, deckId)
  const deck = computed(() => deckQuery.data.value)

  const sections = computed(() => deck.value?.sections ?? [])
  const allCards = computed<DeckCardEntry[]>(() => deck.value?.cards ?? [])

  // Empty sections are hidden by default (a deck seeds ~19), with a toggle to reveal them so
  // the user can still target them from the add box (which always lists every section).
  // Filtering + grouping live in the display engine shared with the public view (#562).
  const showEmpty = ref(false)
  // What each card does (issue #671). Fetched here rather than by the panel that draws it,
  // because the same answer feeds two things: the roles bars and the card list's role
  // filter — exactly as the view fetches legality once for its banner and its per-card
  // chips. A deck write invalidates it (`invalidateDeckAnalysis`).
  const rolesQuery = useDeckRolesQuery(game, deckId)
  const roles = computed(() => rolesQuery.data.value)
  const {
    filterQuery,
    filterColors,
    filterRole,
    filterActive,
    clearFilters,
    cardsBySection,
    deckCards,
    maybeboardCards,
    visibleSections,
    sectionNavItems,
    matchCount,
    totalCount,
  } = useDeckCardDisplay({ cards: allCards, sections, showEmpty, roles })

  // The collection/wish-list overlays, batched over the deck's catalog card ids — the same
  // seam the precon page reads, so the two pages' chips can't drift (issue #707).
  const { ownedInCollection, wantedInWishlist } = useDeckOwnership(game, allCards)

  // "I bought this": every card outside the maybeboards into the collection, on top of what
  // is already owned. The button that calls this confirms first — the write is additive, so
  // a second call records a second copy of everything.
  const addToCollection = useAddDeckToCollectionMutation()
  function addDeckToCollection() {
    return addToCollection.mutateAsync({ game: props.game, deckId: deckId.value })
  }

  // "Make a v2" (issue #674): duplicate this deck and open the copy. Nothing to confirm —
  // the copy is a new private deck the user can delete, unlike the add-to-collection write.
  const copyDeck = useCopyDeckMutation()
  const duplicateError = ref('')
  async function duplicateDeck() {
    const current = deck.value
    if (!current || copyDeck.isPending.value) return
    duplicateError.value = ''
    try {
      const copy = await copyDeck.mutateAsync({ game: props.game, deckId: current.id })
      await router.push(`/decks/${props.game}/${copy.id}`)
    } catch (error) {
      duplicateError.value =
        error instanceof ApiError ? error.message : 'Could not duplicate this deck.'
    }
  }

  // Deck metadata and folder actions.
  const updateDeck = useUpdateDeckMutation()
  const deleteDeck = useDeleteDeckMutation()
  const setVisibility = useSetDeckVisibilityMutation()
  const moveToFolder = useMoveDeckToFolderMutation()
  const foldersQuery = useFoldersQuery(game)
  const folders = computed(() => foldersQuery.data.value?.data ?? [])
  const renameOpen = ref(false)
  const editName = ref('')
  const editFormat = ref('')

  function openRename() {
    editName.value = deck.value?.name ?? ''
    editFormat.value = deck.value?.format ?? ''
    renameOpen.value = true
  }

  async function submitRename() {
    if (!editName.value.trim() || !deck.value) return
    await updateDeck.mutateAsync({
      game: props.game,
      deckId: deck.value.id,
      body: {
        name: editName.value.trim(),
        format: editFormat.value.trim() || null,
        description: deck.value.description,
      },
    })
    renameOpen.value = false
  }

  const deleteOpen = ref(false)
  const deleteError = ref('')

  function requestDeckDelete() {
    const current = deck.value
    if (!current) return
    deleteError.value = ''
    deleteOpen.value = true
  }

  async function confirmDeckDelete() {
    const current = deck.value
    if (!current || deleteDeck.isPending.value) return
    deleteError.value = ''
    try {
      await deleteDeck.mutateAsync({ game: props.game, deckId: current.id })
      deleteOpen.value = false
      await router.push(`/decks/${props.game}`)
    } catch (error) {
      deleteError.value = error instanceof ApiError ? error.message : 'Could not delete this deck.'
    }
  }

  function move(folderId: number | null) {
    if (!deck.value || deck.value.folder_id === folderId) return
    void moveToFolder.mutateAsync({ game: props.game, deckId: deck.value.id, folderId })
  }

  // Authenticated provider-shaped downloads. Capture the deck before awaiting session refresh
  // so a simultaneous route change cannot turn a later `deck.value` access into an exception.
  const exporting = ref(false)
  const exportError = ref('')
  async function exportDeck(format: DeckExportFormat) {
    const current = deck.value
    if (!current || exporting.value) return
    exporting.value = true
    exportError.value = ''
    try {
      const blob = await auth.authFetch((token) =>
        exportDeckFile(token, props.game, current.id, format),
      )
      const extension = format === 'moxfield-text' ? 'txt' : 'csv'
      downloadBlob(blob, `tcglense-${props.game}-deck-${current.id}-${format}.${extension}`)
    } catch (error) {
      exportError.value = error instanceof ApiError ? error.message : 'Export failed. Please retry.'
    } finally {
      exporting.value = false
    }
  }

  // Sharing mirrors the collection visibility flow: choose a username before the first share.
  const shareError = ref('')
  const usernameDialogOpen = ref(false)
  async function setPublic(next: boolean) {
    if (!deck.value) return
    shareError.value = ''
    try {
      await setVisibility.mutateAsync({
        game: props.game,
        deckId: deck.value.id,
        public: next,
      })
    } catch {
      shareError.value = 'Could not update sharing. Please retry.'
    }
  }

  async function toggleShare() {
    if (!deck.value) return
    shareError.value = ''
    if (deck.value.is_public) {
      await setPublic(false)
      return
    }
    if (!auth.user?.username) {
      usernameDialogOpen.value = true
      return
    }
    await setPublic(true)
  }

  function onUsernameSaved() {
    void setPublic(true)
  }

  const shareUrl = computed(() =>
    deck.value?.handle
      ? `${window.location.origin}/u/${deck.value.handle}/decks/${deck.value.id}`
      : '',
  )
  const copied = ref(false)
  function copyShare() {
    const url = shareUrl.value
    if (!url) return
    void navigator.clipboard.writeText(url).then(() => {
      copied.value = true
      setTimeout(() => (copied.value = false), 2000)
    })
  }

  // Section creation, rename/delete, and visible-neighbour ordering.
  const createSection = useCreateSectionMutation()
  const updateSection = useUpdateSectionMutation()
  const deleteSection = useDeleteSectionMutation()
  const reorderSections = useReorderSectionsMutation()
  const newSectionOpen = ref(false)
  const newSectionName = ref('')
  const newSectionMaybeboard = ref(false)
  const sectionDeleteTarget = ref<{ id: number; name: string; count: number } | null>(null)
  const sectionDeleteError = ref('')

  async function submitNewSection() {
    if (!newSectionName.value.trim() || !deck.value) return
    await createSection.mutateAsync({
      game: props.game,
      deckId: deck.value.id,
      name: newSectionName.value.trim(),
      isMaybeboard: newSectionMaybeboard.value,
    })
    newSectionName.value = ''
    newSectionMaybeboard.value = false
    newSectionOpen.value = false
  }

  /** Move a whole section in or out of the deck proper (issue #570). No card moves — only
   * what the header totals, the legality banner, the analytics, and the cross-deck needed
   * list count changes. */
  function toggleSectionMaybeboard(sectionId: number, isMaybeboard: boolean) {
    if (!deck.value) return
    void updateSection.mutateAsync({
      game: props.game,
      deckId: deck.value.id,
      sectionId,
      isMaybeboard,
    })
  }

  function renameSection(sectionId: number, current: string) {
    const name = prompt('Rename section', current)
    if (!name || !name.trim() || !deck.value) return
    void updateSection.mutateAsync({
      game: props.game,
      deckId: deck.value.id,
      sectionId,
      name: name.trim(),
    })
  }

  function requestSectionDelete(sectionId: number, name: string) {
    if (!deck.value) return
    // Deleting moves EVERY entry in the section, so the confirmation must count the
    // unfiltered deck — the display map only holds the entries matching an active filter.
    const count = allCards.value.filter((entry) => entry.section_id === sectionId).length
    sectionDeleteError.value = ''
    sectionDeleteTarget.value = { id: sectionId, name, count }
  }

  function onSectionDeleteOpenChange(open: boolean) {
    if (!open && !deleteSection.isPending.value) sectionDeleteTarget.value = null
  }

  async function confirmSectionDelete() {
    const currentDeck = deck.value
    const target = sectionDeleteTarget.value
    if (!currentDeck || !target || deleteSection.isPending.value) return
    sectionDeleteError.value = ''
    try {
      await deleteSection.mutateAsync({
        game: props.game,
        deckId: currentDeck.id,
        sectionId: target.id,
      })
      sectionDeleteTarget.value = null
    } catch (error) {
      sectionDeleteError.value =
        error instanceof ApiError ? error.message : 'Could not delete this section.'
    }
  }

  function moveSection(sectionId: number, delta: number) {
    // Never reorder against a filter-narrowed list: the "neighbour" could be separated
    // from this section by hidden populated sections, and the swap would silently move
    // it past them. The view also disables the buttons while a filter is active.
    if (!deck.value || filterActive.value) return
    // Swap against the visible neighbour while still submitting the complete section id list.
    const visible = visibleSections.value
    const visibleIndex = visible.findIndex((section) => section.id === sectionId)
    const neighbour = visible[visibleIndex + delta]
    if (visibleIndex < 0 || !neighbour) return
    const ids = sections.value.map((section) => section.id)
    const index = ids.indexOf(sectionId)
    const neighbourIndex = ids.indexOf(neighbour.id)
    if (index < 0 || neighbourIndex < 0) return
    ids[index] = neighbour.id
    ids[neighbourIndex] = sectionId
    void reorderSections.mutateAsync({ game: props.game, deckId: deck.value.id, sectionIds: ids })
  }

  return {
    auth,
    game,
    deckId,
    deckQuery,
    deck,
    sections,
    allCards,
    deckCards,
    maybeboardCards,
    cardsBySection,
    showEmpty,
    visibleSections,
    sectionNavItems,
    rolesQuery,
    roles,
    filterQuery,
    filterColors,
    filterRole,
    filterActive,
    clearFilters,
    matchCount,
    totalCount,
    ownedInCollection,
    wantedInWishlist,
    addDeckToCollection,
    duplicateDeck,
    duplicating: copyDeck.isPending,
    duplicateError,
    folders,
    renameOpen,
    editName,
    editFormat,
    openRename,
    submitRename,
    deleteOpen,
    deleteError,
    deletingDeck: deleteDeck.isPending,
    requestDeckDelete,
    confirmDeckDelete,
    move,
    exporting,
    exportError,
    exportDeck,
    shareError,
    usernameDialogOpen,
    toggleShare,
    onUsernameSaved,
    shareUrl,
    copied,
    copyShare,
    newSectionOpen,
    newSectionName,
    newSectionMaybeboard,
    submitNewSection,
    toggleSectionMaybeboard,
    renameSection,
    sectionDeleteTarget,
    sectionDeleteError,
    deletingSection: deleteSection.isPending,
    requestSectionDelete,
    onSectionDeleteOpenChange,
    confirmSectionDelete,
    moveSection,
  }
}
