<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Copy,
  FileDown,
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
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardTile from '@/components/cards/CardTile.vue'
import DeckAddCard from '@/components/decks/DeckAddCard.vue'
import DeckCardControl from '@/components/decks/DeckCardControl.vue'
import DeckColorFilter from '@/components/decks/DeckColorFilter.vue'
import DeckFormatField from '@/components/decks/DeckFormatField.vue'
import DeckLegalityBanner from '@/components/decks/DeckLegalityBanner.vue'
import DeckSectionNav from '@/components/decks/DeckSectionNav.vue'
import DeckStats from '@/components/decks/DeckStats.vue'
import SetUsernameDialog from '@/components/collection/SetUsernameDialog.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDeckEditor } from '@/composables/useDeckEditor'
import { DECK_CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { deckSectionTargetId } from '@/lib/deckSectionNav'
import { evaluateDeckLegality, legalityLabel } from '@/lib/legality'
import { usePageMeta } from '@/lib/seo'
import { useCardSizeStore } from '@/stores/cardSize'

const props = defineProps<{ game: string; id: string }>()
const money = useCurrency()
const {
  auth,
  game,
  deckQuery,
  deck,
  sections,
  allCards,
  cardsBySection,
  showEmpty,
  visibleSections,
  sectionNavItems,
  filterQuery,
  filterColors,
  filterActive,
  clearFilters,
  matchCount,
  totalCount,
  ownedInCollection,
  wantedInWishlist,
  folders,
  renameOpen,
  editName,
  editFormat,
  openRename,
  submitRename,
  deleteOpen,
  deleteError,
  deletingDeck,
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
  submitNewSection,
  renameSection,
  sectionDeleteTarget,
  sectionDeleteError,
  deletingSection,
  requestSectionDelete,
  onSectionDeleteOpenChange,
  confirmSectionDelete,
  moveSection,
} = useDeckEditor(props)
const cardSize = useCardSizeStore()

usePageMeta({ title: computed(() => deck.value?.name ?? 'Deck'), noindex: true })

// Format legality (issue #557): evaluated client-side from the cards the page already
// holds. Null when the deck's format isn't a legality-tracked one (custom text, Cube…).
const legality = computed(() =>
  deck.value ? evaluateDeckLegality(deck.value.format, allCards.value) : null,
)
// Per-tile breach chips (bottom-right; the control sits bottom-left, ownership top-right).
const LEGALITY_CHIP_TEXT: Record<string, string> = {
  banned: 'text-red-600 dark:text-red-400',
  not_legal: 'text-muted-foreground',
  restricted: 'text-amber-600 dark:text-amber-400',
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

          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button variant="outline" size="sm" :disabled="exporting">
                <FileDown class="size-4" /> Export
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Export deck</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem @select="exportDeck('archidekt')">Archidekt CSV</DropdownMenuItem>
              <DropdownMenuItem @select="exportDeck('moxfield')">Moxfield CSV</DropdownMenuItem>
              <DropdownMenuItem @select="exportDeck('moxfield-text')"
                >Moxfield plain text</DropdownMenuItem
              >
            </DropdownMenuContent>
          </DropdownMenu>

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
              <DropdownMenuItem class="text-destructive" @click="requestDeckDelete"
                ><Trash2 class="size-4" /> Delete deck</DropdownMenuItem
              >
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>
      <p v-if="exportError" class="text-destructive -mt-3 mb-4 text-sm" aria-live="polite">
        {{ exportError }}
      </p>

      <!-- Is this deck legal in its format? (issue #557) -->
      <DeckLegalityBanner v-if="legality" :legality="legality" class="mb-4" />

      <DeckStats :cards="allCards" :sections="sections" />

      <!-- Add cards -->
      <DeckAddCard
        class="mb-6"
        :game="game"
        :deck-id="deck.id"
        :sections="sections"
        :cards="allCards"
      />

      <!-- Card list controls (issue #562): text + colour filters narrow the sections below
        (client-side — the whole deck is already loaded); the size menu writes the shared
        display preference. Deck actions (add section, show empty) sit at the row's end. -->
      <div class="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
        <CardSearchBox
          v-if="allCards.length > 0"
          v-model="filterQuery"
          class="w-full sm:w-60"
          placeholder="Filter cards…"
          aria-label="Filter cards by name, type, text, set, number, rarity, or language"
        />
        <DeckColorFilter v-if="allCards.length > 0" v-model="filterColors" />
        <CardSizeMenu />
        <div class="ml-auto flex flex-wrap items-center gap-3">
          <Dialog v-model:open="newSectionOpen">
            <DialogTrigger as-child>
              <Button variant="outline" size="sm"><Plus class="size-4" /> Add section</Button>
            </DialogTrigger>
            <DialogContent
              class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl"
            >
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
      </div>
      <p v-if="filterActive" class="text-muted-foreground mb-4 text-sm" aria-live="polite">
        Showing {{ matchCount }} of {{ totalCount }} card{{ totalCount === 1 ? '' : 's' }}.
        <button type="button" class="text-primary underline" @click="clearFilters">
          Clear filters
        </button>
      </p>

      <p
        v-if="visibleSections.length === 0 && filterActive"
        class="text-muted-foreground py-12 text-center"
      >
        No cards in this deck match your filter.
      </p>
      <p v-else-if="visibleSections.length === 0" class="text-muted-foreground py-12 text-center">
        This deck is empty. Use the box above to add cards, or show the empty sections to sort into.
      </p>

      <!-- Sections -->
      <div
        v-if="visibleSections.length > 0"
        class="xl:grid xl:grid-cols-[12rem_minmax(0,1fr)] xl:gap-6"
      >
        <DeckSectionNav :items="sectionNavItems" />
        <div class="min-w-0">
          <section
            v-for="(section, index) in visibleSections"
            :id="deckSectionTargetId(section.id)"
            :key="section.id"
            class="mb-8 scroll-mt-16"
          >
            <div class="mb-3 flex items-center justify-between gap-2 border-b pb-1.5">
              <div class="flex items-center gap-2">
                <h2 class="font-medium">{{ section.name }}</h2>
                <span class="text-muted-foreground text-sm"
                  >({{ cardsBySection.get(section.id)?.length ?? 0 }})</span
                >
              </div>
              <div class="flex items-center gap-0.5">
                <!-- Reordering is disabled while a filter narrows the list: the visible
                  neighbour may not be the real neighbour (hidden sections in between). -->
                <button
                  class="text-muted-foreground hover:text-foreground rounded p-1 disabled:opacity-30"
                  aria-label="Move section up"
                  :disabled="index === 0 || filterActive"
                  @click="moveSection(section.id, -1)"
                >
                  <ChevronUp class="size-4" />
                </button>
                <button
                  class="text-muted-foreground hover:text-foreground rounded p-1 disabled:opacity-30"
                  aria-label="Move section down"
                  :disabled="index === visibleSections.length - 1 || filterActive"
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
                      @click="requestSectionDelete(section.id, section.name)"
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
            <div v-else class="grid gap-3" :class="DECK_CARD_SIZE_GRID_CLASS[cardSize.size]">
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
                    :card="entry.card"
                    :quantity="entry.quantity"
                    :foil-quantity="entry.foil_quantity"
                    :sections="sections"
                  />
                  <!-- Format-legality breach chip (issue #557): bottom-right, the one
                    corner the control (bottom-left) and ownership (top-right) never use —
                    on an 88px mobile tile a top-left "Not Legal" would collide with the
                    ownership badges. pointer-events-none keeps the tile's stretched link
                    clickable through it; the banner above carries the full explanation. -->
                  <span
                    v-if="legality?.statusByCardId.get(entry.card.id)"
                    class="bg-background/90 pointer-events-none absolute right-1.5 bottom-1.5 z-20 inline-flex items-center rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none"
                    :class="LEGALITY_CHIP_TEXT[legality.statusByCardId.get(entry.card.id)!]"
                  >
                    {{ legalityLabel(legality.statusByCardId.get(entry.card.id)!) }}
                  </span>
                  <!-- Ownership indicators (top-right): how many of this card you own
                   (collection) and want (wish list), each shown only when non-zero. -->
                  <div
                    v-if="
                      ownedInCollection(entry.card.id) > 0 || wantedInWishlist(entry.card.id) > 0
                    "
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
                      <Heart class="size-3" aria-hidden="true" />{{
                        wantedInWishlist(entry.card.id)
                      }}
                    </span>
                  </div>
                </template>
              </CardTile>
            </div>
          </section>
        </div>
      </div>

      <!-- Rename deck dialog -->
      <Dialog v-model:open="renameOpen">
        <DialogContent class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl">
          <DialogTitle>Edit deck</DialogTitle>
          <form class="mt-2 space-y-3" @submit.prevent="submitRename">
            <Input v-model="editName" placeholder="Deck name" />
            <DeckFormatField v-model="editFormat" :game="game" />
            <div class="flex justify-end gap-2">
              <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
              <Button type="submit" :disabled="!editName.trim()">Save</Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog :open="sectionDeleteTarget != null" @update:open="onSectionDeleteOpenChange">
        <DialogContent class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl">
          <DialogTitle>Delete {{ sectionDeleteTarget?.name }}?</DialogTitle>
          <DialogDescription v-if="sectionDeleteTarget?.count">
            Its {{ sectionDeleteTarget.count }} card
            {{ sectionDeleteTarget.count === 1 ? 'entry moves' : 'entries move' }} to the first
            remaining section.
          </DialogDescription>
          <DialogDescription v-else>
            This empty section will be permanently deleted.
          </DialogDescription>
          <p v-if="sectionDeleteError" class="text-destructive text-sm" aria-live="polite">
            {{ sectionDeleteError }}
          </p>
          <div class="mt-2 flex justify-end gap-2">
            <DialogClose :class="buttonVariants({ variant: 'ghost' })" :disabled="deletingSection">
              Cancel
            </DialogClose>
            <Button variant="destructive" :disabled="deletingSection" @click="confirmSectionDelete">
              {{ deletingSection ? 'Deleting…' : 'Delete section' }}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      <Dialog v-model:open="deleteOpen">
        <DialogContent class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl">
          <DialogTitle>Delete {{ deck.name }}?</DialogTitle>
          <DialogDescription>
            This permanently deletes the deck, its sections, and every card entry. This action
            cannot be undone.
          </DialogDescription>
          <p v-if="deleteError" class="text-destructive text-sm" aria-live="polite">
            {{ deleteError }}
          </p>
          <div class="mt-2 flex justify-end gap-2">
            <DialogClose :class="buttonVariants({ variant: 'ghost' })" :disabled="deletingDeck">
              Cancel
            </DialogClose>
            <Button variant="destructive" :disabled="deletingDeck" @click="confirmDeckDelete">
              {{ deletingDeck ? 'Deleting…' : 'Delete deck' }}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </template>
  </div>
</template>
