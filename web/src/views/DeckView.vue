<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Copy,
  Globe,
  Heart,
  Layers,
  Library,
  Lock,
  MoreVertical,
  Plus,
  Settings2,
  Trash2,
} from '@lucide/vue'
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import CardTile from '@/components/cards/CardTile.vue'
import DeckAddCard from '@/components/decks/DeckAddCard.vue'
import DeckCardControl from '@/components/decks/DeckCardControl.vue'
import SetUsernameDialog from '@/components/collection/SetUsernameDialog.vue'
import type { Card, DeckCardEntry } from '@/lib/api'
import {
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
import { useOwnedCounts as useCollectionOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import { useCurrency } from '@/composables/useCurrency'
import { useAuthStore } from '@/stores/auth'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string; id: string }>()
const router = useRouter()
const auth = useAuthStore()
const money = useCurrency()

const game = computed(() => props.game)
const deckId = computed(() => Number(props.id))
const deckQuery = useDeckQuery(game, deckId)
const deck = computed(() => deckQuery.data.value)

usePageMeta({ title: computed(() => deck.value?.name ?? 'Deck'), noindex: true })

const sections = computed(() => deck.value?.sections ?? [])
const allCards = computed<DeckCardEntry[]>(() => deck.value?.cards ?? [])
const cardsBySection = computed(() => {
  const map = new Map<number, DeckCardEntry[]>()
  for (const s of sections.value) map.set(s.id, [])
  for (const c of allCards.value) map.get(c.section_id)?.push(c)
  return map
})

// Empty sections are hidden by default (a deck seeds ~19), with a toggle to reveal them so
// the user can still target them from the add box (which always lists every section).
const showEmpty = ref(false)
const visibleSections = computed(() =>
  showEmpty.value
    ? sections.value
    : sections.value.filter((s) => (cardsBySection.value.get(s.id)?.length ?? 0) > 0),
)

// Ownership overlays: which of the deck's cards the user owns (collection) and wants
// (wish list), for the chips on each tile (issue #363: indicate what's already in your
// collection; #394: the wish-list count alongside it). Both reuse the shared holdings
// batch-counts seam, keyed by the deck's card ids. Owner-only — the public deck view is
// read-only and mounts neither.
const catalogCards = computed<Card[]>(() => allCards.value.map((c) => c.card))
const { ownership } = useCollectionOwnedCounts(game, catalogCards)
const { ownership: wishlistWanted } = useWishlistCounts(game, catalogCards)
function ownedInCollection(cardId: string): number {
  const c = ownership.value[cardId]
  return c ? c.quantity + c.foil_quantity : 0
}
function wantedInWishlist(cardId: string): number {
  const c = wishlistWanted.value[cardId]
  return c ? c.quantity + c.foil_quantity : 0
}

// --- Deck-level mutations ---
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

function removeDeck() {
  if (!deck.value || !confirm(`Delete the deck "${deck.value.name}"? This can't be undone.`)) return
  void deleteDeck.mutateAsync({ game: props.game, deckId: deck.value.id }).then(() => {
    void router.push(`/decks/${props.game}`)
  })
}

function move(folderId: number | null) {
  if (!deck.value || deck.value.folder_id === folderId) return
  void moveToFolder.mutateAsync({ game: props.game, deckId: deck.value.id, folderId })
}

// --- Sharing ---
// Making a deck public needs a username first (the server 409s otherwise), exactly like a
// public collection: if the user has none, the toggle opens the shared "choose a username"
// dialog and finishes the share on its `saved` event, rather than round-tripping a
// guaranteed conflict. Mirrors CollectionVisibilityControl.
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
// The username dialog saved — finish the "make public" the toggle started.
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
  if (!shareUrl.value) return
  void navigator.clipboard.writeText(shareUrl.value).then(() => {
    copied.value = true
    setTimeout(() => (copied.value = false), 2000)
  })
}

// --- Section mutations ---
const createSection = useCreateSectionMutation()
const updateSection = useUpdateSectionMutation()
const deleteSection = useDeleteSectionMutation()
const reorderSections = useReorderSectionsMutation()

const newSectionOpen = ref(false)
const newSectionName = ref('')
async function submitNewSection() {
  if (!newSectionName.value.trim() || !deck.value) return
  await createSection.mutateAsync({
    game: props.game,
    deckId: deck.value.id,
    name: newSectionName.value.trim(),
  })
  newSectionName.value = ''
  newSectionOpen.value = false
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
function removeSection(sectionId: number, name: string, count: number) {
  if (!deck.value) return
  const msg = count
    ? `Delete "${name}"? Its ${count} card(s) move to your first section.`
    : `Delete the empty section "${name}"?`
  if (!confirm(msg)) return
  void deleteSection.mutateAsync({ game: props.game, deckId: deck.value.id, sectionId })
}
function moveSection(sectionId: number, delta: number) {
  if (!deck.value) return
  // Swap against the VISIBLE neighbour (skipping hidden empty sections), else the up/down
  // buttons appear to no-op when empty sections sit between the visible ones. The full
  // (complete) id list is still submitted, so the backend's "exactly the deck's sections"
  // check holds.
  const visible = visibleSections.value
  const vi = visible.findIndex((s) => s.id === sectionId)
  const vj = vi + delta
  const neighbour = visible[vj]
  if (vi < 0 || !neighbour) return
  const ids = sections.value.map((s) => s.id)
  const i = ids.indexOf(sectionId)
  const j = ids.indexOf(neighbour.id)
  if (i < 0 || j < 0) return
  ids[i] = neighbour.id
  ids[j] = sectionId
  void reorderSections.mutateAsync({ game: props.game, deckId: deck.value.id, sectionIds: ids })
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-6">
    <div
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      class="mx-auto max-w-md py-16 text-center"
    >
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Sign in to view this deck</h1>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink
          :class="buttonVariants()"
          :to="{ path: '/login', query: { redirect: `/decks/${game}/${id}` } }"
          >Sign in</RouterLink
        >
      </div>
    </div>

    <LoadingRow v-else-if="deckQuery.isPending.value" label="Loading deck…" />
    <p v-else-if="deckQuery.isError.value" class="text-muted-foreground py-16 text-center">
      This deck couldn't be found.
      <RouterLink :to="`/decks/${game}`" class="text-primary underline"
        >Back to your decks</RouterLink
      >
    </p>

    <template v-else-if="deck">
      <!-- Header -->
      <RouterLink
        :to="`/decks/${game}`"
        class="text-muted-foreground hover:text-foreground mb-3 inline-flex items-center gap-1 text-sm"
      >
        <ArrowLeft class="size-4" /> All decks
      </RouterLink>

      <header class="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <h1 class="truncate text-2xl font-semibold tracking-tight">{{ deck.name }}</h1>
          <p class="text-muted-foreground mt-1 text-sm">
            {{ deck.summary.total_cards }} card{{ deck.summary.total_cards === 1 ? '' : 's' }}
            <span v-if="deck.format"> · {{ deck.format }}</span>
            <span v-if="money.formatUsd(deck.summary.total_value_usd)">
              · {{ money.formatUsd(deck.summary.total_value_usd) }}</span
            >
          </p>
        </div>

        <div class="flex items-center gap-2">
          <!-- Share -->
          <Popover>
            <PopoverTrigger as-child>
              <Button variant="outline" size="sm">
                <component :is="deck.is_public ? Globe : Lock" class="size-4" />
                {{ deck.is_public ? 'Public' : 'Share' }}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" class="w-72 p-3">
              <p class="text-sm font-medium">Public sharing</p>
              <p class="text-muted-foreground mt-1 text-xs">
                A public deck is viewable by anyone with the link.
              </p>
              <Button
                class="mt-3 w-full"
                size="sm"
                :variant="deck.is_public ? 'outline' : 'default'"
                @click="toggleShare"
              >
                {{ deck.is_public ? 'Make private' : 'Make public' }}
              </Button>
              <p v-if="shareError" class="text-destructive mt-2 text-xs">{{ shareError }}</p>
              <div v-if="deck.is_public && shareUrl" class="mt-3">
                <div class="flex items-center gap-1.5">
                  <input
                    :value="shareUrl"
                    readonly
                    class="border-input bg-muted min-w-0 flex-1 truncate rounded-md border px-2 py-1 text-xs"
                  />
                  <Button variant="outline" size="icon" aria-label="Copy link" @click="copyShare">
                    <Copy class="size-4" />
                  </Button>
                </div>
                <p v-if="copied" class="text-muted-foreground mt-1 text-xs">Copied!</p>
                <RouterLink
                  :to="`/u/${deck.handle}/decks/${deck.id}`"
                  class="text-primary mt-2 inline-block text-xs underline"
                  >View public page</RouterLink
                >
              </div>
            </PopoverContent>
          </Popover>
          <SetUsernameDialog v-model:open="usernameDialogOpen" @saved="onUsernameSaved" />

          <!-- Settings -->
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button variant="outline" size="icon" aria-label="Deck settings"
                ><Settings2 class="size-4"
              /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem @click="openRename"
                ><Settings2 class="size-4" /> Rename / format</DropdownMenuItem
              >
              <DropdownMenuSeparator />
              <DropdownMenuLabel>Move to folder</DropdownMenuLabel>
              <DropdownMenuItem v-if="deck.folder_id != null" @click="move(null)"
                >Remove from folder</DropdownMenuItem
              >
              <DropdownMenuItem
                v-for="folder in folders.filter((f) => f.id !== deck?.folder_id)"
                :key="folder.id"
                @click="move(folder.id)"
                >{{ folder.name }}</DropdownMenuItem
              >
              <DropdownMenuSeparator />
              <DropdownMenuItem class="text-destructive" @click="removeDeck"
                ><Trash2 class="size-4" /> Delete deck</DropdownMenuItem
              >
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>

      <!-- Add cards -->
      <DeckAddCard
        class="mb-6"
        :game="game"
        :deck-id="deck.id"
        :sections="sections"
        :cards="allCards"
      />

      <div class="mb-4 flex items-center justify-between gap-2">
        <Dialog v-model:open="newSectionOpen">
          <DialogTrigger as-child>
            <Button variant="outline" size="sm"><Plus class="size-4" /> Add section</Button>
          </DialogTrigger>
          <DialogContent class="max-w-sm">
            <DialogTitle>New section</DialogTitle>
            <DialogDescription>Add a custom category to sort cards into.</DialogDescription>
            <form class="mt-2 space-y-3" @submit.prevent="submitNewSection">
              <Input v-model="newSectionName" placeholder="Section name" autofocus />
              <div class="flex justify-end gap-2">
                <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
                <Button type="submit" :disabled="!newSectionName.trim()">Add</Button>
              </div>
            </form>
          </DialogContent>
        </Dialog>
        <label class="text-muted-foreground flex items-center gap-1.5 text-sm">
          <input v-model="showEmpty" type="checkbox" class="rounded border" />
          Show empty sections
        </label>
      </div>

      <p v-if="visibleSections.length === 0" class="text-muted-foreground py-12 text-center">
        This deck is empty. Use the box above to add cards, or show the empty sections to sort into.
      </p>

      <!-- Sections -->
      <section v-for="(section, index) in visibleSections" :key="section.id" class="mb-8">
        <div class="mb-3 flex items-center justify-between gap-2 border-b pb-1.5">
          <div class="flex items-center gap-2">
            <h2 class="font-medium">{{ section.name }}</h2>
            <span class="text-muted-foreground text-sm"
              >({{ cardsBySection.get(section.id)?.length ?? 0 }})</span
            >
          </div>
          <div class="flex items-center gap-0.5">
            <button
              class="text-muted-foreground hover:text-foreground rounded p-1 disabled:opacity-30"
              aria-label="Move section up"
              :disabled="index === 0"
              @click="moveSection(section.id, -1)"
            >
              <ChevronUp class="size-4" />
            </button>
            <button
              class="text-muted-foreground hover:text-foreground rounded p-1 disabled:opacity-30"
              aria-label="Move section down"
              :disabled="index === visibleSections.length - 1"
              @click="moveSection(section.id, 1)"
            >
              <ChevronDown class="size-4" />
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger
                class="text-muted-foreground hover:text-foreground rounded p-1"
                aria-label="Section actions"
              >
                <MoreVertical class="size-4" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem @click="renameSection(section.id, section.name)"
                  >Rename</DropdownMenuItem
                >
                <DropdownMenuItem
                  class="text-destructive"
                  @click="
                    removeSection(
                      section.id,
                      section.name,
                      cardsBySection.get(section.id)?.length ?? 0,
                    )
                  "
                  >Delete section</DropdownMenuItem
                >
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        <p
          v-if="(cardsBySection.get(section.id)?.length ?? 0) === 0"
          class="text-muted-foreground text-sm"
        >
          No cards here yet.
        </p>
        <div v-else class="grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6">
          <CardTile
            v-for="entry in cardsBySection.get(section.id) ?? []"
            :key="`${entry.card.id}-${entry.section_id}`"
            :game="game"
            :card="entry.card"
          >
            <template #badge>
              <DeckCardControl
                :game="game"
                :deck-id="deck.id"
                :section-id="entry.section_id"
                :card-id="entry.card.id"
                :name="entry.card.name"
                :quantity="entry.quantity"
                :foil-quantity="entry.foil_quantity"
                :sections="sections"
              />
              <!-- Ownership indicators (top-right): how many of this card you own
                   (collection) and want (wish list), each shown only when non-zero. -->
              <div
                v-if="ownedInCollection(entry.card.id) > 0 || wantedInWishlist(entry.card.id) > 0"
                class="absolute top-1.5 right-1.5 z-20 flex items-center gap-1"
              >
                <span
                  v-if="ownedInCollection(entry.card.id) > 0"
                  class="bg-background/90 text-foreground inline-flex cursor-default items-center gap-0.5 rounded-md border px-1.5 py-0.5 text-xs shadow select-none"
                  :title="`You own ${ownedInCollection(entry.card.id)} of this card`"
                >
                  <Library class="size-3" aria-hidden="true" />{{
                    ownedInCollection(entry.card.id)
                  }}
                </span>
                <span
                  v-if="wantedInWishlist(entry.card.id) > 0"
                  class="bg-background/90 text-foreground inline-flex cursor-default items-center gap-0.5 rounded-md border px-1.5 py-0.5 text-xs shadow select-none"
                  :title="`You have ${wantedInWishlist(entry.card.id)} of this card on your wish list`"
                >
                  <Heart class="size-3" aria-hidden="true" />{{ wantedInWishlist(entry.card.id) }}
                </span>
              </div>
            </template>
          </CardTile>
        </div>
      </section>

      <!-- Rename deck dialog -->
      <Dialog v-model:open="renameOpen">
        <DialogContent class="max-w-sm">
          <DialogTitle>Edit deck</DialogTitle>
          <form class="mt-2 space-y-3" @submit.prevent="submitRename">
            <Input v-model="editName" placeholder="Deck name" />
            <Input v-model="editFormat" placeholder="Format (optional)" />
            <div class="flex justify-end gap-2">
              <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
              <Button type="submit" :disabled="!editName.trim()">Save</Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>
    </template>
  </div>
</template>
