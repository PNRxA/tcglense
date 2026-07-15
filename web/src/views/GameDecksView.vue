<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { FolderPlus, Layers, Plus } from '@lucide/vue'
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
import DeckImportDialog from '@/components/decks/DeckImportDialog.vue'
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
import { ApiError, type Deck } from '@/lib/api'
import { formatPresetsFor } from '@/lib/deckFormats'
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
const decks = computed(() => decksQuery.data.value?.data ?? [])
const folders = computed(() => foldersQuery.data.value?.data ?? [])

// Decks grouped: one bucket per folder (even empty ones), then the loose decks.
const looseDecks = computed(() => decks.value.filter((d) => d.folder_id == null))
function decksInFolder(folderId: number): Deck[] {
  return decks.value.filter((d) => d.folder_id === folderId)
}

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
const newDeckFormat = ref('')
const formatPresets = computed(() => formatPresetsFor(props.game))
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
  newDeckFormat.value = ''
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
  newDeckFormat.value = ''
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
    <div v-if="auth.sessionResolved && !auth.isAuthenticated" class="mx-auto max-w-md py-16 text-center">
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Sign in to build decks</h1>
      <p class="text-muted-foreground mt-2">
        Create and organise {{ gameName }} decks, and share them with a link. Sign in or create a
        free account to get started.
      </p>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink :class="buttonVariants()" :to="{ path: '/login', query: { redirect: `/decks/${game}` } }"
          >Sign in</RouterLink
        >
        <RouterLink
          :class="buttonVariants({ variant: 'outline' })"
          :to="{ path: '/register', query: { redirect: `/decks/${game}` } }"
          >Create account</RouterLink
        >
      </div>
    </div>

    <template v-else>
      <header class="mb-6 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-2xl font-semibold tracking-tight">{{ gameName }} decks</h1>
          <p class="text-muted-foreground text-sm">{{ decks.length }} deck(s)</p>
        </div>
        <div class="flex gap-2">
          <DeckImportDialog :game="game" />
          <Dialog v-model:open="folderOpen">
            <DialogTrigger as-child>
              <Button variant="outline"><FolderPlus class="size-4" /> New folder</Button>
            </DialogTrigger>
            <DialogContent class="max-w-sm">
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
            <DialogContent class="max-w-sm">
              <DialogTitle>New deck</DialogTitle>
              <DialogDescription>
                Give your deck a name, pick a format, and file it in a folder — all optional.
              </DialogDescription>
              <form class="mt-2 space-y-3" @submit.prevent="submitCreateDeck">
                <Input v-model="newDeckName" placeholder="Deck name" autofocus />
                <!-- Free-typed format with preset suggestions via a native datalist. -->
                <Input
                  v-model="newDeckFormat"
                  list="deck-format-presets"
                  placeholder="Format (optional, e.g. Commander)"
                />
                <datalist id="deck-format-presets">
                  <option v-for="f in formatPresets" :key="f" :value="f" />
                </datalist>
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
      <p v-else-if="decksQuery.isError.value" class="text-destructive py-8">
        Couldn't load your decks. Please retry.
      </p>
      <p
        v-else-if="decks.length === 0 && folders.length === 0"
        class="text-muted-foreground py-16 text-center"
      >
        You haven't built any decks yet. Hit <strong>New deck</strong> to start one.
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
