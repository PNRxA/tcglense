import { useQuery, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import type { Ref } from 'vue'
import {
  createDeck,
  createFolder,
  createSection,
  deleteDeck,
  deleteFolder,
  deleteSection,
  getDeck,
  getDecks,
  getFolders,
  getPublicDeck,
  getPublicDecks,
  moveDeckCard,
  moveDeckToFolder,
  reorderSections,
  setDeckCard,
  setDeckVisibility,
  updateDeck,
  updateFolder,
  updateSection,
} from '@/lib/api/decks'
import type {
  ApiError,
  CollectionQuantities,
  CreateDeckRequest,
  Deck,
  DeckDetail,
  DeckFolder,
  DeckSection,
  DeckVisibility,
  UpdateDeckRequest,
} from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// ---------- Deck query + mutation composables (issue #363) ----------
//
// Decks are a per-user *container* surface — many decks per game, each with sections and
// cards — so they don't fit the collection/wish-list `makeHoldingQueries` factory (which
// bakes in one implicit list per game). They live beside it, mirroring its idioms: every
// key family head-starts with `deck`/`decks` so `useAuthCacheReset` wipes them on an
// identity change, and reactive params (a deck id) go INSIDE the query key as refs. As in
// `holdingQueries.ts`, each option object is an intermediate variable (with explicitly
// typed callbacks) so TanStack's deeply-reactive types don't trip excess-property checks
// through the `useAuthed*` wrappers.

// ----- Reads -----

/** The signed-in user's decks for a game, newest edit first. */
export function useDecksQuery(game: Ref<string>) {
  const options = {
    queryKey: ['decks', game],
    queryFn: (token: string) => getDecks(token, game.value),
  }
  return useAuthedQuery<{ data: Deck[] }>(options)
}

/** The full detail of one deck. `deckId` is a ref inside the key, so navigating between
 * decks refetches. */
export function useDeckQuery(game: Ref<string>, deckId: Ref<number>, enabled?: Ref<boolean>) {
  const options = {
    queryKey: ['deck', game, deckId],
    queryFn: (token: string) => getDeck(token, game.value, deckId.value),
    enabled,
  }
  return useAuthedQuery<DeckDetail>(options)
}

/** The user's deck folders for a game. */
export function useFoldersQuery(game: Ref<string>) {
  const options = {
    queryKey: ['deck-folders', game],
    queryFn: (token: string) => getFolders(token, game.value),
  }
  return useAuthedQuery<{ data: DeckFolder[] }>(options)
}

// ----- Public reads (unauthenticated, handle-addressed) -----

/** A user's public decks, by handle. Plain `useQuery` (token-less, CDN-cacheable);
 * `retry: false` so a 404 (unknown handle / nothing public) is terminal. */
export function usePublicDecksQuery(handle: Ref<string>) {
  return useQuery<{ data: Deck[] }, ApiError>({
    queryKey: ['public-decks', handle],
    queryFn: () => getPublicDecks(handle.value),
    retry: false,
  })
}

/** One public deck's full detail, by handle + deck id. */
export function usePublicDeckQuery(handle: Ref<string>, deckId: Ref<number>) {
  return useQuery<DeckDetail, ApiError>({
    queryKey: ['public-deck', handle, deckId],
    queryFn: () => getPublicDeck(handle.value, deckId.value),
    retry: false,
  })
}

// ----- Invalidation -----

/** Refresh a single deck's detail plus the deck list (its card count / metadata may have
 * changed) after a write. */
export function invalidateDeck(qc: QueryClient, game: string, deckId?: number) {
  if (deckId !== undefined) qc.invalidateQueries({ queryKey: ['deck', game, deckId] })
  qc.invalidateQueries({ queryKey: ['decks', game] })
}

// ----- Mutation variable shapes -----

export interface CreateDeckVars {
  game: string
  body: CreateDeckRequest
}
export interface UpdateDeckVars {
  game: string
  deckId: number
  body: UpdateDeckRequest
}
export interface DeckIdVars {
  game: string
  deckId: number
}
export interface MoveFolderVars {
  game: string
  deckId: number
  folderId: number | null
}
export interface DeckVisibilityVars {
  game: string
  deckId: number
  public: boolean
}
export interface FolderCreateVars {
  game: string
  name: string
}
export interface FolderUpdateVars {
  game: string
  folderId: number
  name: string
}
export interface FolderIdVars {
  game: string
  folderId: number
}
export interface SectionCreateVars {
  game: string
  deckId: number
  name: string
}
export interface SectionUpdateVars {
  game: string
  deckId: number
  sectionId: number
  name?: string
  position?: number
}
export interface SectionIdVars {
  game: string
  deckId: number
  sectionId: number
}
export interface ReorderSectionsVars {
  game: string
  deckId: number
  sectionIds: number[]
}
export interface SetDeckCardVars {
  game: string
  deckId: number
  sectionId: number
  id: string
  quantity: number
  foil_quantity: number
}
export interface MoveDeckCardVars {
  game: string
  deckId: number
  id: string
  fromSectionId: number
  toSectionId: number
}

// ----- Deck mutations -----

export function useCreateDeckMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: CreateDeckVars) => createDeck(token, vars.game, vars.body),
    onSettled: (_d: DeckDetail | undefined, _e: ApiError | null, vars: CreateDeckVars) =>
      invalidateDeck(qc, vars.game),
  }
  return useAuthedMutation<DeckDetail, CreateDeckVars>(options)
}

export function useUpdateDeckMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: UpdateDeckVars) =>
      updateDeck(token, vars.game, vars.deckId, vars.body),
    onSettled: (_d: Deck | undefined, _e: ApiError | null, vars: UpdateDeckVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<Deck, UpdateDeckVars>(options)
}

export function useDeleteDeckMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: DeckIdVars) => deleteDeck(token, vars.game, vars.deckId),
    onSettled: (_d: void | undefined, _e: ApiError | null, vars: DeckIdVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<void, DeckIdVars>(options)
}

export function useMoveDeckToFolderMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: MoveFolderVars) =>
      moveDeckToFolder(token, vars.game, vars.deckId, vars.folderId),
    onSettled: (_d: Deck | undefined, _e: ApiError | null, vars: MoveFolderVars) => {
      invalidateDeck(qc, vars.game, vars.deckId)
      qc.invalidateQueries({ queryKey: ['deck-folders', vars.game] })
    },
  }
  return useAuthedMutation<Deck, MoveFolderVars>(options)
}

export function useSetDeckVisibilityMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: DeckVisibilityVars) =>
      setDeckVisibility(token, vars.game, vars.deckId, vars.public),
    onSettled: (_d: DeckVisibility | undefined, _e: ApiError | null, vars: DeckVisibilityVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<DeckVisibility, DeckVisibilityVars>(options)
}

// ----- Folder mutations -----

export function useCreateFolderMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: FolderCreateVars) => createFolder(token, vars.game, vars.name),
    onSettled: (_d: DeckFolder | undefined, _e: ApiError | null, vars: FolderCreateVars) =>
      qc.invalidateQueries({ queryKey: ['deck-folders', vars.game] }),
  }
  return useAuthedMutation<DeckFolder, FolderCreateVars>(options)
}

export function useUpdateFolderMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: FolderUpdateVars) =>
      updateFolder(token, vars.game, vars.folderId, vars.name),
    onSettled: (_d: DeckFolder | undefined, _e: ApiError | null, vars: FolderUpdateVars) =>
      qc.invalidateQueries({ queryKey: ['deck-folders', vars.game] }),
  }
  return useAuthedMutation<DeckFolder, FolderUpdateVars>(options)
}

export function useDeleteFolderMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: FolderIdVars) =>
      deleteFolder(token, vars.game, vars.folderId),
    onSettled: (_d: void | undefined, _e: ApiError | null, vars: FolderIdVars) => {
      // Deleting a folder ungroups its decks, so the deck list changes too.
      qc.invalidateQueries({ queryKey: ['deck-folders', vars.game] })
      qc.invalidateQueries({ queryKey: ['decks', vars.game] })
    },
  }
  return useAuthedMutation<void, FolderIdVars>(options)
}

// ----- Section mutations (all refresh the owning deck's detail) -----

export function useCreateSectionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SectionCreateVars) =>
      createSection(token, vars.game, vars.deckId, vars.name),
    onSettled: (_d: DeckSection | undefined, _e: ApiError | null, vars: SectionCreateVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<DeckSection, SectionCreateVars>(options)
}

export function useUpdateSectionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SectionUpdateVars) =>
      updateSection(token, vars.game, vars.deckId, vars.sectionId, {
        name: vars.name,
        position: vars.position,
      }),
    onSettled: (_d: DeckSection | undefined, _e: ApiError | null, vars: SectionUpdateVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<DeckSection, SectionUpdateVars>(options)
}

export function useDeleteSectionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SectionIdVars) =>
      deleteSection(token, vars.game, vars.deckId, vars.sectionId),
    onSettled: (_d: void | undefined, _e: ApiError | null, vars: SectionIdVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<void, SectionIdVars>(options)
}

export function useReorderSectionsMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: ReorderSectionsVars) =>
      reorderSections(token, vars.game, vars.deckId, vars.sectionIds),
    onSettled: (
      _d: { data: DeckSection[] } | undefined,
      _e: ApiError | null,
      vars: ReorderSectionsVars,
    ) => invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<{ data: DeckSection[] }, ReorderSectionsVars>(options)
}

// ----- Deck-card mutations -----

/** Set a card's absolute counts within a section (both zero removes it there), then
 * refresh the deck. Drives the deck-card quantity control (via the editor's `saveFn`). */
export function useSetDeckCardMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetDeckCardVars) =>
      setDeckCard(token, vars.game, vars.deckId, vars.id, {
        quantity: vars.quantity,
        foil_quantity: vars.foil_quantity,
        section_id: vars.sectionId,
      }),
    onSettled: (_d: CollectionQuantities | undefined, _e: ApiError | null, vars: SetDeckCardVars) =>
      invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<CollectionQuantities, SetDeckCardVars>(options)
}

/** Move a card between two of the deck's sections. */
export function useMoveDeckCardMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: MoveDeckCardVars) =>
      moveDeckCard(token, vars.game, vars.deckId, vars.id, vars.fromSectionId, vars.toSectionId),
    onSettled: (
      _d: CollectionQuantities | undefined,
      _e: ApiError | null,
      vars: MoveDeckCardVars,
    ) => invalidateDeck(qc, vars.game, vars.deckId),
  }
  return useAuthedMutation<CollectionQuantities, MoveDeckCardVars>(options)
}
