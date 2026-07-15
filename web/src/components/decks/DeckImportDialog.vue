<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Download, LoaderCircle, TriangleAlert } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useImportDeckMutation } from '@/composables/useDecks'
import { ApiError } from '@/lib/api'
import type { CollectionProvider, DeckImportFileFormat, DeckImportResponse } from '@/lib/api'

const props = defineProps<{ game: string }>()
const router = useRouter()
const importDeck = useImportDeckMutation()

type SourceType = 'link' | 'file'
const open = ref(false)
const sourceType = ref<SourceType>('link')
const provider = ref<CollectionProvider>('archidekt')
const source = ref('')
const file = ref<File | null>(null)
const deckName = ref('')
const errorMessage = ref('')
const result = ref<DeckImportResponse | null>(null)

const providers: { value: CollectionProvider; label: string; linkDisabled?: boolean }[] = [
  { value: 'archidekt', label: 'Archidekt' },
  {
    value: 'moxfield',
    label: 'Moxfield',
    linkDisabled: true,
  },
]

const placeholders: Record<CollectionProvider, string> = {
  archidekt: 'https://archidekt.com/decks/12345/deck-name',
  moxfield: 'https://moxfield.com/decks/4xUdq-66IEKK6X53bhUS8Q',
}

watch(open, (isOpen) => {
  if (!isOpen) return
  sourceType.value = 'link'
  provider.value = 'archidekt'
  source.value = ''
  file.value = null
  deckName.value = ''
  errorMessage.value = ''
  result.value = null
  importDeck.reset()
})

watch(sourceType, (next) => {
  if (next === 'link' && provider.value === 'moxfield') provider.value = 'archidekt'
  errorMessage.value = ''
  result.value = null
})

watch(provider, (next) => {
  if (sourceType.value === 'link' && next === 'moxfield') provider.value = 'archidekt'
  file.value = null
  errorMessage.value = ''
  result.value = null
})

function onFile(event: Event) {
  const picked = (event.target as HTMLInputElement).files?.[0] ?? null
  file.value = picked
  if (picked && !deckName.value.trim()) {
    deckName.value = picked.name.replace(/\.(csv|txt)$/i, '')
  }
  errorMessage.value = ''
  result.value = null
}

const canSubmit = computed(() => {
  if (importDeck.isPending.value) return false
  if (sourceType.value === 'link') return source.value.trim().length > 0
  return file.value != null && deckName.value.trim().length > 0
})

function fileFormat(picked: File): DeckImportFileFormat {
  return picked.name.toLowerCase().endsWith('.txt') ? 'text' : 'csv'
}

async function runImport() {
  if (!canSubmit.value) return
  errorMessage.value = ''
  result.value = null
  try {
    if (sourceType.value === 'link') {
      result.value = await importDeck.mutateAsync({
        game: props.game,
        body: {
          provider: provider.value,
          source: source.value.trim(),
          contents: null,
          format: null,
          name: null,
        },
      })
      return
    }
    const picked = file.value
    if (!picked) return
    const format = fileFormat(picked)
    if (provider.value === 'archidekt' && format === 'text') {
      errorMessage.value = 'Archidekt uploads must use a CSV deck export.'
      return
    }
    result.value = await importDeck.mutateAsync({
      game: props.game,
      body: {
        provider: provider.value,
        source: null,
        contents: await picked.text(),
        format,
        name: deckName.value.trim(),
      },
    })
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : 'The deck could not be imported. Please retry.'
  }
}

function openDeck() {
  if (!result.value) return
  open.value = false
  void router.push(`/decks/${props.game}/${result.value.deck.id}`)
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger :class="buttonVariants({ variant: 'outline' })">
      <Download class="size-4" />
      Import deck
    </DialogTrigger>
    <DialogContent class="max-h-[90vh] w-[min(92vw,32rem)] overflow-y-auto">
      <DialogTitle>Import a deck</DialogTitle>
      <DialogDescription>
        Create a new deck from a public Archidekt link or an Archidekt/Moxfield export. Categories
        and boards become deck sections.
      </DialogDescription>

      <div class="mt-4 space-y-4">
        <div class="bg-muted grid grid-cols-2 gap-1 rounded-lg p-1" role="tablist">
          <button
            type="button"
            role="tab"
            :aria-selected="sourceType === 'link'"
            class="rounded-md px-3 py-1.5 text-sm font-medium"
            :class="sourceType === 'link' ? 'bg-background shadow-sm' : 'text-muted-foreground'"
            @click="sourceType = 'link'"
          >
            Paste a link
          </button>
          <button
            type="button"
            role="tab"
            :aria-selected="sourceType === 'file'"
            class="rounded-md px-3 py-1.5 text-sm font-medium"
            :class="sourceType === 'file' ? 'bg-background shadow-sm' : 'text-muted-foreground'"
            @click="sourceType = 'file'"
          >
            Upload a file
          </button>
        </div>

        <div class="space-y-1.5">
          <Label for="deck-import-provider">Provider</Label>
          <Select v-model="provider">
            <SelectTrigger id="deck-import-provider" class="w-full">
              <SelectValue placeholder="Choose a provider" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="item in providers"
                :key="item.value"
                :value="item.value"
                :disabled="sourceType === 'link' && item.linkDisabled"
              >
                {{ item.label
                }}{{ sourceType === 'link' && item.linkDisabled ? ' — upload only' : '' }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="sourceType === 'link'" class="space-y-1.5">
          <Label for="deck-import-source">Deck URL or id</Label>
          <Input id="deck-import-source" v-model="source" :placeholder="placeholders[provider]" />
          <p class="text-muted-foreground text-xs">
            The deck must be public. Moxfield links remain upload-only until the server has an
            approved Moxfield User-Agent.
          </p>
        </div>

        <template v-else>
          <div class="space-y-1.5">
            <Label for="deck-import-name">Deck name</Label>
            <Input id="deck-import-name" v-model="deckName" placeholder="Imported deck" />
          </div>
          <div class="space-y-1.5">
            <Label for="deck-import-file">Deck export</Label>
            <input
              id="deck-import-file"
              type="file"
              :accept="provider === 'archidekt' ? '.csv,text/csv' : '.csv,.txt,text/csv,text/plain'"
              class="border-input dark:bg-input/30 file:bg-muted block w-full cursor-pointer rounded-md border bg-transparent text-sm file:mr-3 file:border-0 file:px-3 file:py-2 file:text-sm file:font-medium"
              @change="onFile"
            />
            <p class="text-muted-foreground text-xs">
              <template v-if="provider === 'archidekt'">
                Keep the CSV header row and include the Quantity, Name, and Scryfall ID columns.
              </template>
              <template v-else> Upload a Moxfield CSV or plain-text deck export. </template>
              Files are parsed server-side and do not change an existing deck.
            </p>
          </div>
        </template>

        <p
          v-if="importDeck.isPending.value"
          class="text-muted-foreground flex items-center gap-2 text-sm"
          aria-live="polite"
        >
          <LoaderCircle class="size-4 animate-spin" /> Importing deck…
        </p>
        <p v-if="errorMessage" class="text-destructive flex gap-2 text-sm" aria-live="polite">
          <TriangleAlert class="mt-0.5 size-4 shrink-0" /> {{ errorMessage }}
        </p>
        <div v-if="result" class="bg-muted space-y-1 rounded-md p-3 text-sm" aria-live="polite">
          <p class="font-medium">{{ result.deck.name }} was created.</p>
          <p>{{ result.matched_cards.toLocaleString() }} catalog card(s) matched.</p>
          <p v-if="result.unmatched_cards" class="text-muted-foreground">
            {{ result.unmatched_cards.toLocaleString() }} card(s) could not be matched and were
            skipped<span v-if="result.unmatched_sample.length"
              >: {{ result.unmatched_sample.join(', ') }}</span
            >.
          </p>
        </div>
      </div>

      <div class="mt-6 flex justify-end gap-2">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
        <Button v-if="result" @click="openDeck">Open deck</Button>
        <Button v-else :disabled="!canSubmit" @click="runImport">
          {{ importDeck.isPending.value ? 'Importing…' : 'Import' }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
