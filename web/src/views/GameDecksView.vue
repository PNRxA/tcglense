<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { Boxes, FolderPlus, Layers, Plus, ShoppingCart, TriangleAlert } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import DeckTile from '@/components/decks/DeckTile.vue'
import DeckFormatField from '@/components/decks/DeckFormatField.vue'
import DeckImportDialog from '@/components/decks/DeckImportDialog.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CountLineCue from '@/components/cards/CountLineCue.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import {
  useCreateDeckMutation,
  useCreateFolderMutation,
  useDecksQuery,
  useDeleteDeckMutation,
  useDeleteFolderMutation,
  useFoldersQuery,
  useMoveDeckToFolderMutation,
} from '@/composables/useDecks'
import { useStableOrder } from '@/composables/useStableOrder'
import { ApiError, type Deck } from '@/lib/api'
import { defaultFormatFor } from '@/lib/deckFormats'
import { DECK_DEFAULT_SORT, DECK_SORT_OPTIONS, sortDecks } from '@/lib/deckSort'
import { useAuthStore } from '@/stores/auth'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string }>()
const game = computed(() => props.game)
const router = useRouter()
const auth = useAuthStore()

const { data: games } = useGamesQuery()
const gameName = computed(
  () => games.value?.data.find((g) => g.id === props.game)?.name ?? props.game.toUpperCase(),
)
usePageMeta({ title: computed(() => `Your ${gameName.value} decks`), noindex: true })

const decksQuery = useDecksQuery(game)
const foldersQuery = useFoldersQuery(game)
// The API returns decks `updated_at DESC`, so the deck you last edited comes back FIRST —
// and every deck write marks this list stale. Arriving here from a deck you just edited
// therefore paints the cached (old) order, then swaps a reordered list in a moment later,
// pushing every tile below the mover down a full row while you're reaching for one. Pin the
// order this visit painted: each refetch still refreshes every tile's name, count and colours
// in place, it just can't move them. The true order lands on the next visit.
const stableDecks = useStableOrder(
  () => decksQuery.data.value?.data ?? [],
  (deck) => deck.id,
)
// The user-chosen order over the pinned list. `updated` (the default) passes the stable
// order through untouched, so the freeze above keeps doing its job; `name`/`price` impose
// a deterministic order with full tie-breaks, which a refetch can't reshuffle either —
// the two mechanisms answer the same "tiles must not move under a tap" concern, one for
// the re-pickable sorts `useStableOrder` explicitly doesn't cover.
const deckSort = ref(DECK_DEFAULT_SORT)
const decks = computed(() => sortDecks(stableDecks.value, deckSort.value))
const folders = computed(() => foldersQuery.data.value?.data ?? [])

// Decks grouped: one bucket per folder (even empty ones), then the loose decks — partitioned
// in ONE pass, so every deck lands in exactly one bucket by construction. The two buckets used
// to be independent filters (`folder_id === folderId` and `folder_id == null`), which left a
// deck whose folder isn't in `folders` matching NEITHER: it rendered nowhere while the header
// above kept counting it (issue #622).
//
// That gap isn't hypothetical — the two queries settle at different times. Deleting a folder
// invalidates `['deck-folders', game]` and `['decks', game]` as two independent refetches, and
// the decks response is deterministically the slower one (it runs `deck_headers` with the
// per-deck facet scans, and sea-orm pins the default SQLite backend to a single connection), so
// in that window the folder's section is already gone while the cached decks still carry its
// id. Same shape when a folder is deleted in another tab, and when the folders query itself
// fails while the decks query succeeds.
//
// Treating an unknown folder as loose is what the server will say once its response lands, so
// the tiles stay put and settle in place instead of vanishing and popping back a row lower.
const grouped = computed(() => {
  const byFolder = new Map<number, Deck[]>(folders.value.map((f) => [f.id, []]))
  const loose: Deck[] = []
  for (const deck of decks.value) {
    const bucket = deck.folder_id == null ? undefined : byFolder.get(deck.folder_id)
    if (bucket) bucket.push(deck)
    else loose.push(deck)
  }
  return { byFolder, loose }
})
const looseDecks = computed(() => grouped.value.loose)
function decksInFolder(folderId: number): Deck[] {
  return grouped.value.byFolder.get(folderId) ?? []
}

// query-core's error reducer sets `status: 'error'` on ANY failed fetch while KEEPING the
// cached `data`, and this list carries the default 5-minute `staleTime` plus
// `refetchOnWindowFocus` — so gating the list on bare `isError` swapped a perfectly good list
// for "Couldn't load your decks" the first time a background refetch hiccuped (issue #622).
// TanStack already splits the two cases, so use its own predicates rather than re-deriving
// them: `isLoadingError` is a failure with nothing ever loaded (the page genuinely has nothing
// to show), `isRefetchError` is a failure over data that's still cached (keep showing it, and
// say so quietly beside the count).
const listFailed = computed(() => decksQuery.isLoadingError.value)
const refreshFailed = computed(() => decksQuery.isRefetchError.value)

// Folder creation is shared: the standalone New-folder dialog and the New-deck dialog's
// "+ New folder…" option both mint folders through this one mutation.
const createFolder = useCreateFolderMutation()

// Resolve a typed folder name to an id, reusing an existing folder whose name matches
// case-insensitively so a repeat name never trips the create endpoint's duplicate 409.
async function resolveFolderByName(name: string): Promise<number> {
  const existing = folders.value.find((f) => f.name.toLowerCase() === name.toLowerCase())
  if (existing) return existing.id
  const folder = await createFolder.mutateAsync({ game: props.game, name })
  return folder.id
}

// --- Create deck ---
const createOpen = ref(false)
const newDeckName = ref('')
// Starts on the game's most-played format (Commander for MTG); the select still offers
// "No format" and every other option.
const newDeckFormat = ref(defaultFormatFor(props.game))
// Folder choice for the new deck. reka's Select reserves '' for "no selection", so the
// picker uses explicit string sentinels: NO_FOLDER = no folder, NEW_FOLDER = create one
// from `newDeckFolderName`; any other value is the chosen folder's id as a string.
const NO_FOLDER = 'none'
const NEW_FOLDER = 'new'
const newDeckFolderChoice = ref(NO_FOLDER)
const newDeckFolderName = ref('')
const createDeck = useCreateDeckMutation()
// Open the dialog fresh every time, so a folder selection left over from a cancelled run
// (whose folder may since have been deleted) can't be submitted as a stale id.
watch(createOpen, (open) => {
  if (!open) return
  newDeckName.value = ''
  newDeckFormat.value = defaultFormatFor(props.game)
  newDeckFolderChoice.value = NO_FOLDER
  newDeckFolderName.value = ''
})
async function submitCreateDeck() {
  const name = newDeckName.value.trim()
  if (!name) return
  let folderId: number | null = null
  if (newDeckFolderChoice.value === NEW_FOLDER) {
    const folderName = newDeckFolderName.value.trim()
    if (!folderName) return
    folderId = await resolveFolderByName(folderName)
  } else if (newDeckFolderChoice.value !== NO_FOLDER) {
    // Guard against a folder deleted after it was selected: fall back to loose (no folder)
    // rather than POSTing an id the backend would 404.
    const id = Number(newDeckFolderChoice.value)
    folderId = folders.value.some((f) => f.id === id) ? id : null
  }
  const deck = await createDeck.mutateAsync({
    game: props.game,
    body: {
      name,
      format: newDeckFormat.value.trim() || null,
      description: null,
      folder_id: folderId,
    },
  })
  createOpen.value = false
  newDeckName.value = ''
  newDeckFormat.value = defaultFormatFor(props.game)
  newDeckFolderChoice.value = NO_FOLDER
  newDeckFolderName.value = ''
  void router.push(`/decks/${props.game}/${deck.id}`)
}

// --- Create folder (standalone dialog) ---
const folderOpen = ref(false)
const newFolderName = ref('')
async function submitCreateFolder() {
  const name = newFolderName.value.trim()
  if (!name) return
  await createFolder.mutateAsync({ game: props.game, name })
  folderOpen.value = false
  newFolderName.value = ''
}

const deleteFolder = useDeleteFolderMutation()
const deleteDeck = useDeleteDeckMutation()
const moveDeck = useMoveDeckToFolderMutation()
const deckDeleteTarget = ref<Deck | null>(null)
const deckDeleteError = ref('')

function move(deck: Deck, folderId: number | null) {
  if (deck.folder_id === folderId) return
  void moveDeck.mutateAsync({ game: props.game, deckId: deck.id, folderId })
}
function requestDeckDelete(deck: Deck) {
  deckDeleteError.value = ''
  deckDeleteTarget.value = deck
}
function onDeckDeleteOpenChange(open: boolean) {
  if (!open && !deleteDeck.isPending.value) deckDeleteTarget.value = null
}
async function confirmDeckDelete() {
  const target = deckDeleteTarget.value
  if (!target || deleteDeck.isPending.value) return
  deckDeleteError.value = ''
  try {
    await deleteDeck.mutateAsync({ game: props.game, deckId: target.id })
    deckDeleteTarget.value = null
  } catch (error) {
    deckDeleteError.value =
      error instanceof ApiError ? error.message : 'Could not delete this deck.'
  }
}
function removeFolder(folderId: number, name: string) {
  if (!confirm(`Delete the folder "${name}"? Its decks are kept (just ungrouped).`)) return
  void deleteFolder.mutateAsync({ game: props.game, folderId })
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <!-- Signed-out: prompt in place rather than bouncing to /login. -->
    <div
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      class="mx-auto max-w-md py-16 text-center"
    >
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Sign in to build decks</h1>
      <p class="text-muted-foreground mt-2">
        Create and organise {{ gameName }} decks, and share them with a link. Sign in or create a
        free account to get started.
      </p>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink
          :class="buttonVariants()"
          :to="{ path: '/login', query: { redirect: `/decks/${game}` } }"
          >Sign in</RouterLink
        >
        <RouterLink
          :class="buttonVariants({ variant: 'outline' })"
          :to="{ path: '/register', query: { redirect: `/decks/${game}` } }"
          >Create account</RouterLink
        >
      </div>
      <!-- Preconstructed decks are public catalog data, so there's something to read here
           without an account — and a precon is a good first deck to copy once there is one. -->
      <p class="text-muted-foreground mt-6 text-sm">
        Or browse the
        <RouterLink :to="`/decks/${game}/precons`" class="text-primary underline"
          >preconstructed decks</RouterLink
        >
        — every list {{ gameName }} shipped, no account needed.
      </p>
    </div>

    <template v-else>
      <header
        class="mb-6 flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center sm:justify-between"
      >
        <div>
          <h1 class="text-2xl font-semibold tracking-tight">{{ gameName }} decks</h1>
          <p class="text-muted-foreground text-sm">
            {{ decks.length }} deck(s)
            <!-- A failed *background* refetch keeps the list it couldn't refresh, so name the
                 staleness beside the count rather than replacing the page — the same
                 non-destructive, in-the-count-line cue `UpdatingCue` establishes for the
                 browse views. -->
            <template v-if="refreshFailed">
              ·
              <span class="text-destructive" aria-live="polite">
                <CountLineCue
                  :icon="TriangleAlert"
                  label="Couldn't refresh — showing your last loaded decks."
                />
              </span>
            </template>
          </p>
        </div>
        <!-- Wrap on narrow screens so the action buttons never run off-screen. -->
        <div class="flex flex-wrap gap-2">
          <!-- Re-order the shelf (recency / name / price) — client-side, the list is
               unpaginated. -->
          <CardSortMenu v-if="decks.length > 1" v-model="deckSort" :options="DECK_SORT_OPTIONS" />
          <!-- Cards needed across all decks vs the collection (issue #499). -->
          <RouterLink
            v-if="decks.length"
            :class="buttonVariants({ variant: 'outline' })"
            :to="`/decks/${game}/needed`"
          >
            <ShoppingCart class="size-4" /> Cards needed
          </RouterLink>
          <!-- The published decklists, beside your own (they're catalog data, not yours). -->
          <RouterLink
            :class="buttonVariants({ variant: 'outline' })"
            :to="`/decks/${game}/precons`"
          >
            <Boxes class="size-4" /> Preconstructed
          </RouterLink>
          <DeckImportDialog :game="game" />
          <Dialog v-model:open="folderOpen">
            <DialogTrigger as-child>
              <Button variant="outline"><FolderPlus class="size-4" /> New folder</Button>
            </DialogTrigger>
            <DialogContent
              class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl"
            >
              <DialogTitle>New folder</DialogTitle>
              <DialogDescription>Group your decks under a named folder.</DialogDescription>
              <form class="mt-2 space-y-3" @submit.prevent="submitCreateFolder">
                <Input v-model="newFolderName" placeholder="Folder name" autofocus />
                <div class="flex justify-end gap-2">
                  <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
                  <Button type="submit" :disabled="!newFolderName.trim()">Create</Button>
                </div>
              </form>
            </DialogContent>
          </Dialog>

          <Dialog v-model:open="createOpen">
            <DialogTrigger as-child>
              <Button><Plus class="size-4" /> New deck</Button>
            </DialogTrigger>
            <DialogContent
              class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl"
            >
              <DialogTitle>New deck</DialogTitle>
              <DialogDescription>
                Give your deck a name, pick a format, and file it in a folder — all optional.
              </DialogDescription>
              <form class="mt-2 space-y-3" @submit.prevent="submitCreateDeck">
                <Input v-model="newDeckName" placeholder="Deck name" autofocus />
                <!-- Real formats first, free text as the last-resort "Custom…" (issue #557). -->
                <DeckFormatField v-model="newDeckFormat" :game="game" />
                <!-- Folder: none, an existing one, or a brand-new one. -->
                <Select v-model="newDeckFolderChoice">
                  <SelectTrigger class="w-full" aria-label="Folder">
                    <SelectValue placeholder="No folder" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem :value="NO_FOLDER">No folder</SelectItem>
                    <SelectItem v-for="f in folders" :key="f.id" :value="String(f.id)">
                      {{ f.name }}
                    </SelectItem>
                    <SelectItem :value="NEW_FOLDER">+ New folder…</SelectItem>
                  </SelectContent>
                </Select>
                <Input
                  v-if="newDeckFolderChoice === NEW_FOLDER"
                  v-model="newDeckFolderName"
                  placeholder="New folder name"
                />
                <div class="flex justify-end gap-2">
                  <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
                  <Button
                    type="submit"
                    :disabled="
                      !newDeckName.trim() ||
                      (newDeckFolderChoice === NEW_FOLDER && !newDeckFolderName.trim()) ||
                      createDeck.isPending.value ||
                      createFolder.isPending.value
                    "
                    >Create</Button
                  >
                </div>
              </form>
            </DialogContent>
          </Dialog>
        </div>
      </header>

      <LoadingRow
        v-if="decksQuery.isPending.value || foldersQuery.isPending.value"
        label="Loading decks…"
      />
      <p v-else-if="listFailed" class="text-destructive py-8">
        Couldn't load your decks. Please retry.
      </p>
      <p
        v-else-if="decks.length === 0 && folders.length === 0"
        class="text-muted-foreground py-16 text-center"
      >
        You haven't built any decks yet. Hit <strong>New deck</strong> to start one, or copy a
        <RouterLink :to="`/decks/${game}/precons/all`" class="text-primary underline"
          >preconstructed deck</RouterLink
        >.
      </p>

      <div v-else class="space-y-8">
        <!-- One section per folder (with a delete control), then the loose decks. -->
        <section v-for="folder in folders" :key="folder.id">
          <div class="mb-2 flex items-center justify-between border-b pb-1">
            <h2 class="font-medium">{{ folder.name }}</h2>
            <button
              class="text-muted-foreground hover:text-destructive text-xs"
              @click="removeFolder(folder.id, folder.name)"
            >
              Delete folder
            </button>
          </div>
          <p v-if="decksInFolder(folder.id).length === 0" class="text-muted-foreground text-sm">
            No decks in this folder yet.
          </p>
          <div v-else class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <DeckTile
              v-for="deck in decksInFolder(folder.id)"
              :key="deck.id"
              :deck="deck"
              :game="game"
              :folders="folders"
              @move="(fid) => move(deck, fid)"
              @remove="requestDeckDelete(deck)"
            />
          </div>
        </section>

        <section v-if="looseDecks.length">
          <h2 v-if="folders.length" class="mb-2 border-b pb-1 font-medium">Ungrouped</h2>
          <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <DeckTile
              v-for="deck in looseDecks"
              :key="deck.id"
              :deck="deck"
              :game="game"
              :folders="folders"
              @move="(fid) => move(deck, fid)"
              @remove="requestDeckDelete(deck)"
            />
          </div>
        </section>
      </div>

      <Dialog :open="deckDeleteTarget != null" @update:open="onDeckDeleteOpenChange">
        <DialogContent class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl">
          <DialogTitle>Delete {{ deckDeleteTarget?.name }}?</DialogTitle>
          <DialogDescription class="text-muted-foreground mt-1 text-sm">
            This permanently deletes the deck, its sections, and every card entry. This action
            cannot be undone.
          </DialogDescription>
          <p v-if="deckDeleteError" class="text-destructive mt-3 text-sm" aria-live="polite">
            {{ deckDeleteError }}
          </p>
          <div class="mt-5 flex justify-end gap-2">
            <DialogClose
              :class="buttonVariants({ variant: 'ghost' })"
              :disabled="deleteDeck.isPending.value"
            >
              Cancel
            </DialogClose>
            <Button
              variant="destructive"
              :disabled="deleteDeck.isPending.value"
              @click="confirmDeckDelete"
            >
              {{ deleteDeck.isPending.value ? 'Deleting…' : 'Delete deck' }}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </template>
  </div>
</template>
