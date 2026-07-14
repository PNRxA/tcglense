<script setup lang="ts">
import { computed, ref } from 'vue'
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
import DeckTile from '@/components/decks/DeckTile.vue'
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
import type { Deck } from '@/lib/api'
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

// --- Create deck ---
const createOpen = ref(false)
const newDeckName = ref('')
const newDeckFormat = ref('')
const createDeck = useCreateDeckMutation()
async function submitCreateDeck() {
  const name = newDeckName.value.trim()
  if (!name) return
  const deck = await createDeck.mutateAsync({
    game: props.game,
    body: { name, format: newDeckFormat.value.trim() || null, description: null, folder_id: null },
  })
  createOpen.value = false
  newDeckName.value = ''
  newDeckFormat.value = ''
  void router.push(`/decks/${props.game}/${deck.id}`)
}

// --- Create folder ---
const folderOpen = ref(false)
const newFolderName = ref('')
const createFolder = useCreateFolderMutation()
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

function move(deck: Deck, folderId: number | null) {
  if (deck.folder_id === folderId) return
  void moveDeck.mutateAsync({ game: props.game, deckId: deck.id, folderId })
}
function removeDeck(deck: Deck) {
  if (!confirm(`Delete the deck "${deck.name}"? This can't be undone.`)) return
  void deleteDeck.mutateAsync({ game: props.game, deckId: deck.id })
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
              <DialogDescription>Give your deck a name (and an optional format).</DialogDescription>
              <form class="mt-2 space-y-3" @submit.prevent="submitCreateDeck">
                <Input v-model="newDeckName" placeholder="Deck name" autofocus />
                <Input v-model="newDeckFormat" placeholder="Format (optional, e.g. Commander)" />
                <div class="flex justify-end gap-2">
                  <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
                  <Button type="submit" :disabled="!newDeckName.trim() || createDeck.isPending.value"
                    >Create</Button
                  >
                </div>
              </form>
            </DialogContent>
          </Dialog>
        </div>
      </header>

      <LoadingRow v-if="decksQuery.isPending.value" label="Loading decks…" />
      <p v-else-if="decksQuery.isError.value" class="text-destructive py-8">
        Couldn't load your decks. Please retry.
      </p>
      <p v-else-if="decks.length === 0" class="text-muted-foreground py-16 text-center">
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
              @remove="removeDeck(deck)"
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
              @remove="removeDeck(deck)"
            />
          </div>
        </section>
      </div>
    </template>
  </div>
</template>
