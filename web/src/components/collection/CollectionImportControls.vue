<script setup lang="ts">
import { ref, toRef } from 'vue'
import { FileDown } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import ImportCollectionDialog from '@/components/collection/ImportCollectionDialog.vue'
import { ApiError, type CollectionExportFormat, exportCollectionCsv } from '@/lib/api'
import { downloadBlob } from '@/lib/download'
import { useAuthStore } from '@/stores/auth'

// The import / export surface for the per-game collection landing: the import dialog and
// a provider-shaped CSV export. Keyed only off the game; mounted by GameCollectionView
// when the visitor is signed in.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

// Export the collection to a provider-shaped CSV (Archidekt or Moxfield). The download
// needs the access token, so it goes through the auth store's authFetch (which refreshes
// once on a 401) rather than a plain link; the blob is then saved client-side.
const auth = useAuthStore()
const exporting = ref(false)
const exportMessage = ref<string | null>(null)
async function exportCollection(format: CollectionExportFormat) {
  if (exporting.value) return
  exporting.value = true
  exportMessage.value = null
  try {
    const blob = await auth.authFetch((token) => exportCollectionCsv(token, game.value, format))
    downloadBlob(blob, `tcglense-${game.value}-collection-${format}.csv`)
  } catch (err) {
    exportMessage.value = err instanceof ApiError ? err.message : 'Export failed. Please try again.'
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <!-- Import from an external collection provider, or export what you own. -->
  <div class="mt-5 flex flex-wrap items-center gap-3">
    <ImportCollectionDialog :game="game" />
    <!-- Export the collection to a provider-shaped CSV (Archidekt or Moxfield). -->
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <Button variant="outline" size="sm" :disabled="exporting">
          <FileDown />
          Export
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" class="w-56">
        <DropdownMenuLabel>Export collection</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="exportCollection('archidekt')">Archidekt CSV</DropdownMenuItem>
        <DropdownMenuItem @select="exportCollection('moxfield')">Moxfield CSV</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
  <p v-if="exportMessage" class="text-destructive mt-2 text-sm" aria-live="polite">
    {{ exportMessage }}
  </p>
</template>
