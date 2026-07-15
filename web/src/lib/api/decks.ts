import { API_URL, apiErrorFromResponse, request } from './client'
import type {
  ChangeDeckCardPrintingRequest,
  CollectionQuantities,
  CreateDeckRequest,
  Deck,
  DeckDetail,
  DeckFolder,
  DeckImportRequest,
  DeckImportResponse,
  DeckSection,
  DeckVisibility,
  SetDeckCardRequest,
  UpdateDeckRequest,
} from './generated'

// ---------- Decks (per-user, authenticated) ----------
//
// A deck (issue #363) is a first-class, named container of cards for a game, organised
// into user-orderable *sections* (Archidekt-style categories) and, at the deck level,
// into *folders*. Unlike the collection/wish list (one implicit list per (user, game)),
// a user has many decks, so the routes nest a `{deckId}` and add CRUD verbs that the
// `makeHoldingApi` factory can't express — so this surface lives on its own (the same
// "beside the factory" shape the wishlist's sealed products use). Every authed call takes
// an access `token` (via the auth store's `authFetch`); the public reads take none.

export type {
  CreateDeckRequest,
  Deck,
  DeckCardEntry,
  DeckDetail,
  DeckFolder,
  DeckImportFileFormat,
  DeckImportRequest,
  DeckImportResponse,
  DeckSection,
  DeckVisibility,
  SetDeckCardRequest,
  UpdateDeckRequest,
} from './generated'

const base = (game: string): string => `/api/decks/${encodeURIComponent(game)}`
const deckBase = (game: string, deckId: number): string => `${base(game)}/${deckId}`

// ----- Deck CRUD -----

/** The signed-in user's decks for a game, most-recently-updated first. */
export function getDecks(token: string, game: string): Promise<{ data: Deck[] }> {
  return request<{ data: Deck[] }>(base(game), { token })
}

/** The full detail of one deck (metadata, sections, every card, value summary). */
export function getDeck(token: string, game: string, deckId: number): Promise<DeckDetail> {
  return request<DeckDetail>(deckBase(game, deckId), { token })
}

/** Create a deck (seeded with the default sections) and return its full detail. */
export function createDeck(
  token: string,
  game: string,
  body: CreateDeckRequest,
): Promise<DeckDetail> {
  return request<DeckDetail>(base(game), { method: 'POST', body, token })
}

/** Create a new deck from a public provider link or uploaded deck-list contents. */
export function importDeck(
  token: string,
  game: string,
  body: DeckImportRequest,
): Promise<DeckImportResponse> {
  return request<DeckImportResponse>(`${base(game)}/import`, { method: 'POST', body, token })
}

/** Replace a deck's editable metadata (name/description/format). */
export function updateDeck(
  token: string,
  game: string,
  deckId: number,
  body: UpdateDeckRequest,
): Promise<Deck> {
  return request<Deck>(deckBase(game, deckId), { method: 'PUT', body, token })
}

/** Delete a deck (its sections + cards cascade away). */
export function deleteDeck(token: string, game: string, deckId: number): Promise<void> {
  return request<void>(deckBase(game, deckId), { method: 'DELETE', token })
}

/** Move a deck to a folder, or `null` to loosen it. */
export function moveDeckToFolder(
  token: string,
  game: string,
  deckId: number,
  folderId: number | null,
): Promise<Deck> {
  return request<Deck>(`${deckBase(game, deckId)}/folder`, {
    method: 'PUT',
    body: { folder_id: folderId },
    token,
  })
}

/** Enable/disable public sharing for a deck (enabling needs a username first -> 409). */
export function setDeckVisibility(
  token: string,
  game: string,
  deckId: number,
  isPublic: boolean,
): Promise<DeckVisibility> {
  return request<DeckVisibility>(`${deckBase(game, deckId)}/visibility`, {
    method: 'PUT',
    body: { public: isPublic },
    token,
  })
}

// ----- Folders -----

/** The user's deck folders for a game (alphabetical), each with a deck count. */
export function getFolders(token: string, game: string): Promise<{ data: DeckFolder[] }> {
  return request<{ data: DeckFolder[] }>(`${base(game)}/folders`, { token })
}

/** Create a folder. */
export function createFolder(token: string, game: string, name: string): Promise<DeckFolder> {
  return request<DeckFolder>(`${base(game)}/folders`, { method: 'POST', body: { name }, token })
}

/** Rename a folder. */
export function updateFolder(
  token: string,
  game: string,
  folderId: number,
  name: string,
): Promise<DeckFolder> {
  return request<DeckFolder>(`${base(game)}/folders/${folderId}`, {
    method: 'PUT',
    body: { name },
    token,
  })
}

/** Delete a folder (its decks are ungrouped, not deleted). */
export function deleteFolder(token: string, game: string, folderId: number): Promise<void> {
  return request<void>(`${base(game)}/folders/${folderId}`, { method: 'DELETE', token })
}

// ----- Sections -----

/** Create a custom section (appended after the last). */
export function createSection(
  token: string,
  game: string,
  deckId: number,
  name: string,
): Promise<DeckSection> {
  return request<DeckSection>(`${deckBase(game, deckId)}/sections`, {
    method: 'POST',
    body: { name },
    token,
  })
}

/** Rename and/or reposition a section (each field optional). */
export function updateSection(
  token: string,
  game: string,
  deckId: number,
  sectionId: number,
  body: { name?: string; position?: number },
): Promise<DeckSection> {
  return request<DeckSection>(`${deckBase(game, deckId)}/sections/${sectionId}`, {
    method: 'PUT',
    body,
    token,
  })
}

/** Delete a section (its cards move to the deck's first remaining section). */
export function deleteSection(
  token: string,
  game: string,
  deckId: number,
  sectionId: number,
): Promise<void> {
  return request<void>(`${deckBase(game, deckId)}/sections/${sectionId}`, {
    method: 'DELETE',
    token,
  })
}

/** Set the display order of a deck's sections (must be exactly the deck's section ids). */
export function reorderSections(
  token: string,
  game: string,
  deckId: number,
  sectionIds: number[],
): Promise<{ data: DeckSection[] }> {
  return request<{ data: DeckSection[] }>(`${deckBase(game, deckId)}/sections/reorder`, {
    method: 'PUT',
    body: { section_ids: sectionIds },
    token,
  })
}

// ----- Deck cards -----

/** Set the absolute counts for a card in one section (both zero removes it there). */
export function setDeckCard(
  token: string,
  game: string,
  deckId: number,
  id: string,
  body: SetDeckCardRequest,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(
    `${deckBase(game, deckId)}/cards/${encodeURIComponent(id)}`,
    { method: 'PUT', body, token },
  )
}

/** Move a card between two of the deck's sections (merging counts on a collision). */
export function moveDeckCard(
  token: string,
  game: string,
  deckId: number,
  id: string,
  fromSectionId: number,
  toSectionId: number,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(
    `${deckBase(game, deckId)}/cards/${encodeURIComponent(id)}/move`,
    {
      method: 'PUT',
      body: { from_section_id: fromSectionId, to_section_id: toSectionId },
      token,
    },
  )
}

/** Atomically replace a card with another printing of the same gameplay card. */
export function changeDeckCardPrinting(
  token: string,
  game: string,
  deckId: number,
  id: string,
  body: ChangeDeckCardPrintingRequest,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(
    `${deckBase(game, deckId)}/cards/${encodeURIComponent(id)}/printing`,
    { method: 'PUT', body, token },
  )
}

// ----- Import/export -----

export type DeckExportFormat = 'archidekt' | 'moxfield' | 'moxfield-text'

export function deckExportPath(game: string, deckId: number, format: DeckExportFormat): string {
  return `${deckBase(game, deckId)}/export?format=${format}`
}

/** Download a deck export. File responses bypass the JSON request wrapper. */
export async function exportDeckFile(
  token: string,
  game: string,
  deckId: number,
  format: DeckExportFormat,
): Promise<Blob> {
  const response = await fetch(`${API_URL}${deckExportPath(game, deckId, format)}`, {
    headers: { Authorization: `Bearer ${token}` },
    credentials: 'include',
  })
  if (!response.ok) {
    const error = await apiErrorFromResponse(
      response,
      `Export failed with status ${response.status}`,
    )
    throw error
  }
  return response.blob()
}

// ----- Public (unauthenticated, handle-addressed) -----

const publicBase = (handle: string): string => `/api/u/${encodeURIComponent(handle)}/decks`

/** A user's public decks (across games), by handle. Token-less; CDN-cacheable. */
export function getPublicDecks(handle: string): Promise<{ data: Deck[] }> {
  return request<{ data: Deck[] }>(publicBase(handle))
}

/** One public deck's full detail, by handle + deck id. Token-less; CDN-cacheable. */
export function getPublicDeck(handle: string, deckId: number): Promise<DeckDetail> {
  return request<DeckDetail>(`${publicBase(handle)}/${deckId}`)
}
