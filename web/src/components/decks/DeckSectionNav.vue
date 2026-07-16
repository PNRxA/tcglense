<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { deckSectionTargetId } from '@/lib/deckSectionNav'

export interface DeckSectionNavItem {
  id: number
  name: string
  count: number
}

const props = defineProps<{ items: DeckSectionNavItem[] }>()
const activeSectionId = ref<number | null>(props.items[0]?.id ?? null)

function targetFor(sectionId: number): HTMLElement | null {
  return document.getElementById(deckSectionTargetId(sectionId))
}

// True once scrolling has bottomed out and no heading can be brought any further up. The
// tolerance absorbs the sub-pixel shortfall fractional zoom leaves at maximum scroll; the
// scrollability gate keeps a deck that fits the viewport — permanently at its "end", never
// scrolled — from reporting its last section as current on load.
function atDocumentEnd(): boolean {
  const doc = document.documentElement
  if (doc.scrollHeight <= window.innerHeight) return false
  return window.innerHeight + window.scrollY >= doc.scrollHeight - 1
}

function updateActiveSection() {
  const first = props.items[0]
  if (!first) {
    activeSectionId.value = null
    return
  }

  // Treat the section crossing the upper quarter of the viewport as current. This keeps the
  // highlight useful while a heading is below the sticky mobile picker.
  const marker = Math.min(window.innerHeight * 0.25, 160)
  let current = first.id
  for (const item of props.items) {
    const target = targetFor(item.id)
    if (target && target.getBoundingClientRect().top <= marker) current = item.id
    else if (target) break
  }

  // A short final section's heading can sit below the marker even at maximum scroll — its cards,
  // the page's trailing padding and the footer all take room under it — which would leave it
  // permanently unreachable. With nowhere left to scroll, the reader is on it regardless.
  const last = props.items[props.items.length - 1]
  if (last && atDocumentEnd()) current = last.id
  activeSectionId.value = current
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches === true
}

function jumpTo(sectionId: number) {
  const target = targetFor(sectionId)
  if (!target) return
  activeSectionId.value = sectionId
  target.scrollIntoView?.({
    behavior: prefersReducedMotion() ? 'auto' : 'smooth',
    block: 'start',
  })
  // Suppressing the fragment navigation to keep the scroll smooth also drops the focus move it
  // would have made: without this the next Tab resumes from the nav and walks back into the first
  // section, and a screen reader never leaves the nav at all. `preventScroll` leaves the smooth
  // scroll in flight, and the sections' `scroll-mt-16` keeps the heading clear of the picker.
  target.tabIndex = -1
  target.focus({ preventScroll: true })
}

function onMobileChange(event: Event) {
  jumpTo(Number((event.target as HTMLSelectElement).value))
}

onMounted(() => {
  updateActiveSection()
  window.addEventListener('scroll', updateActiveSection, { passive: true })
  window.addEventListener('resize', updateActiveSection)
})

onBeforeUnmount(() => {
  window.removeEventListener('scroll', updateActiveSection)
  window.removeEventListener('resize', updateActiveSection)
})

watch(
  () => props.items,
  async (items) => {
    if (!items.some((item) => item.id === activeSectionId.value)) {
      activeSectionId.value = items[0]?.id ?? null
    }
    await nextTick()
    updateActiveSection()
  },
  { deep: true },
)
</script>

<template>
  <!-- `contents` makes each responsive treatment a direct child of the page's section grid.
       The mobile picker can therefore stick for the full height of the section list, while the
       desktop aside occupies the grid's narrow first column. -->
  <div class="contents">
    <nav
      aria-label="Deck categories"
      class="bg-background/95 sticky top-0 z-30 -mx-4 mb-4 border-y px-4 py-2 backdrop-blur xl:hidden"
    >
      <label class="flex items-center gap-3">
        <span class="shrink-0 text-sm font-medium">Category</span>
        <select
          class="border-input bg-background focus-visible:border-ring focus-visible:ring-ring/50 h-9 min-w-0 flex-1 rounded-md border px-3 text-sm outline-none focus-visible:ring-3"
          aria-label="Jump to a deck category"
          :value="activeSectionId ?? undefined"
          @change="onMobileChange"
        >
          <option v-for="item in items" :key="item.id" :value="item.id">
            {{ item.name }} ({{ item.count }})
          </option>
        </select>
      </label>
    </nav>

    <aside class="hidden xl:block">
      <nav
        aria-label="Deck categories"
        class="sticky top-4 max-h-[calc(100vh-2rem)] overflow-y-auto rounded-lg border p-2"
      >
        <p class="text-muted-foreground px-2 pt-1 pb-2 text-xs font-medium tracking-wide uppercase">
          Categories
        </p>
        <ul class="space-y-0.5">
          <li v-for="item in items" :key="item.id">
            <a
              :href="`#${deckSectionTargetId(item.id)}`"
              class="hover:bg-muted hover:text-foreground flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors"
              :class="
                item.id === activeSectionId
                  ? 'bg-accent text-accent-foreground font-medium'
                  : 'text-muted-foreground'
              "
              :aria-current="item.id === activeSectionId ? 'location' : undefined"
              @click.prevent="jumpTo(item.id)"
            >
              <span class="min-w-0 flex-1 truncate">{{ item.name }}</span>
              <span class="shrink-0 text-xs tabular-nums">{{ item.count }}</span>
            </a>
          </li>
        </ul>
      </nav>
    </aside>
  </div>
</template>
