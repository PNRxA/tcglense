<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useArtTagList } from '@/composables/useArtTags'
import { artTagSlug, getArtTagValues, toggleArtTag } from '@/lib/searchBuilder'
import type { ArtTagEntry } from '@/lib/api'

// The art-tag browser (issue #140), styled after Scryfall's tagger-tags page: the
// game's whole tag vocabulary in A–Z sections with a filter box, so tags are
// discoverable without knowing their slug. Clicking a tag toggles its `art:` token in
// the shared query — the dialog edits the very same string the panel and the search
// box edit, so selections show up immediately behind it.
const query = defineModel<string>({ required: true })
const open = defineModel<boolean>('open', { required: true })
const props = defineProps<{ game: string }>()
// Forwarded so the opener can prevent reka's default close-focus (which would land
// on <body> — this dialog has no trigger element — and dismiss the Filters popover
// hosting it) and refocus its own Browse button instead. Same contract as
// QuickAddPrintDialog's closeAutoFocus.
const emit = defineEmits<{ closeAutoFocus: [event: Event] }>()

const enabled = computed(() => open.value)
const { data, isPending, isError } = useArtTagList(
  computed(() => props.game),
  enabled,
)

const term = ref('')

// The vocabulary, narrowed by the filter box across slug, label, and description.
const filtered = computed<ArtTagEntry[]>(() => {
  const all = data.value?.data ?? []
  const needle = term.value.trim().toLowerCase()
  if (!needle) return all
  return all.filter(
    (t) =>
      t.slug.includes(needle) ||
      t.label.toLowerCase().includes(needle) ||
      (t.description?.toLowerCase().includes(needle) ?? false),
  )
})

// A–Z sections (slug order comes from the server); anything not starting with a
// letter files under '#', like Scryfall's listing.
const sections = computed(() => {
  const groups = new Map<string, ArtTagEntry[]>()
  for (const tag of filtered.value) {
    const first = tag.slug[0] ?? '#'
    const letter = /[a-z]/.test(first) ? first.toUpperCase() : '#'
    const group = groups.get(letter)
    if (group) group.push(tag)
    else groups.set(letter, [tag])
  }
  return [...groups.entries()].map(([letter, tags]) => ({ letter, tags }))
})

// Tags already in the query (any alias/quoting), for the selected checkmarks.
const selected = computed(() => new Set(getArtTagValues(query.value).map(artTagSlug)))

function toggle(slug: string) {
  query.value = toggleArtTag(query.value, slug)
}

// Letter quick-nav: scroll the section into view inside the dialog's scroll pane.
const sectionEls = ref(new Map<string, HTMLElement>())
function bindSection(letter: string, el: unknown) {
  if (el instanceof HTMLElement) sectionEls.value.set(letter, el)
  else sectionEls.value.delete(letter)
}
function jumpTo(letter: string) {
  sectionEls.value.get(letter)?.scrollIntoView({ block: 'start' })
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="bg-background flex max-h-[85svh] w-[min(96vw,48rem)] flex-col gap-3 rounded-xl border p-5 shadow-lg"
      @close-auto-focus="emit('closeAutoFocus', $event)"
    >
      <div class="flex items-start justify-between gap-2">
        <DialogTitle>Browse art tags</DialogTitle>
        <!-- A real, focusable close control: keyboard/AT users shouldn't depend on
             Escape or the overlay to leave a modal holding thousands of buttons. -->
        <DialogClose
          class="text-muted-foreground hover:text-foreground -mt-1 -mr-1 rounded-md p-1"
          aria-label="Close"
        >
          <X class="size-4" aria-hidden="true" />
        </DialogClose>
      </div>
      <DialogDescription class="text-muted-foreground text-sm">
        Community Tagger labels for what a card's artwork depicts. Pick any number of tags — each
        becomes an
        <code class="bg-muted rounded px-1 py-0.5">art:</code> filter.
      </DialogDescription>

      <Input
        v-model="term"
        type="text"
        placeholder="Filter tags — name or description"
        aria-label="Filter art tags"
        autocomplete="off"
      />

      <!-- Letter quick-nav; hidden while a filter narrows the list (sections shrink
           to a screenful anyway). -->
      <div v-if="!term.trim() && sections.length > 1" class="flex flex-wrap gap-0.5">
        <Button
          v-for="section in sections"
          :key="section.letter"
          variant="ghost"
          size="sm"
          class="h-6 min-w-6 px-1 text-xs tabular-nums"
          @click="jumpTo(section.letter)"
        >
          {{ section.letter }}
        </Button>
      </div>

      <div class="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
        <p v-if="isPending" class="text-muted-foreground py-8 text-center text-sm">Loading tags…</p>
        <p v-else-if="isError" class="text-muted-foreground py-8 text-center text-sm">
          Couldn't load the tag list. Close and try again.
        </p>
        <p v-else-if="!sections.length" class="text-muted-foreground py-8 text-center text-sm">
          No tags match "{{ term.trim() }}".
        </p>
        <section
          v-for="section in sections"
          v-else
          :key="section.letter"
          :ref="(el) => bindSection(section.letter, el)"
          class="scroll-mt-1 [content-visibility:auto]"
        >
          <h3 class="text-muted-foreground mb-1.5 text-xs font-semibold tracking-wide">
            {{ section.letter }}
          </h3>
          <!-- Native <button>s, not the Button component: the full vocabulary is
               ~10k tags, and that many component instances would stall the open /
               filter-clear render; plain elements patch an order of magnitude
               cheaper (the QuickAddBox option-list precedent). -->
          <div class="flex flex-wrap gap-1.5">
            <button
              v-for="tag in section.tags"
              :key="tag.slug"
              type="button"
              class="inline-flex h-7 items-center gap-1 rounded-md border px-2 text-xs transition-colors"
              :class="
                selected.has(tag.slug)
                  ? 'bg-secondary text-secondary-foreground border-transparent'
                  : 'hover:bg-accent hover:text-accent-foreground'
              "
              :aria-pressed="selected.has(tag.slug)"
              :title="tag.description ?? undefined"
              @click="toggle(tag.slug)"
            >
              <Check v-if="selected.has(tag.slug)" class="size-3" aria-hidden="true" />
              <span>{{ tag.label }}</span>
              <span class="text-muted-foreground tabular-nums">{{ tag.count }}</span>
            </button>
          </div>
        </section>
      </div>
    </DialogContent>
  </Dialog>
</template>
