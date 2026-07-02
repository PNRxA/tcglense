<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { RefreshCw, Settings } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import ImportCollectionDialog from '@/components/collection/ImportCollectionDialog.vue'
import { useGameName } from '@/composables/useCatalog'
import { invalidateCollectionData } from '@/composables/useCollection'
import {
  useCollectionSourceQuery,
  usePolledImportJob,
  useSaveCollectionSourceMutation,
  useSyncCollectionSourceMutation,
} from '@/composables/useCollectionImport'
import { ApiError, providerLabel as providerName, type CollectionProvider } from '@/lib/api'

// The import / re-sync surface for the per-game collection landing: the import dialog, a
// re-sync button for a saved link, and the live job status. Keyed only off the game;
// mounted by GameCollectionView when the visitor is signed in.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

// Import / sync from an external collection provider (Archidekt or Moxfield).
const qc = useQueryClient()
const sourceQuery = useCollectionSourceQuery(game)
const source = computed(() => sourceQuery.data.value ?? null)
const syncMutation = useSyncCollectionSourceMutation()
const syncMessage = ref<string | null>(null)

const providerLabel = computed(() => providerName(source.value?.provider ?? 'archidekt'))
// A saved link can re-sync by smart (incremental) sync or a full mirror; the label,
// confirmation, and result copy differ because smart never removes cards.
const smart = computed(() => source.value?.smart ?? false)
const lastSyncedText = computed(() => {
  const t = source.value?.last_synced_at
  if (!t) return 'Not synced yet'
  const d = new Date(t)
  return Number.isNaN(d.getTime()) ? '' : `Last synced ${d.toLocaleString()}`
})

// The cog next to the re-sync button switches how the saved link re-syncs — smart
// (incremental) vs. a full mirror — the same choice the import dialog offers, without
// reopening it. It re-saves the link with the new `smart` flag (preserving the provider,
// source, and last-synced marker); the reloaded source re-labels the Re-sync button.
type ResyncMode = 'smart' | 'mirror'
const saveMutation = useSaveCollectionSourceMutation()
const savingResyncMode = computed(() => saveMutation.isPending.value)
// Mirror the server truth locally so the menu reflects the pick instantly; reconcile to
// the reloaded source whenever it changes (a save landing, or a switch between games).
const resyncMode = ref<ResyncMode>(smart.value ? 'smart' : 'mirror')
watch(smart, (s) => {
  resyncMode.value = s ? 'smart' : 'mirror'
})

async function setResyncMode(value: string | undefined) {
  const src = source.value
  if (!src || (value !== 'smart' && value !== 'mirror')) return
  // Skip a truly-redundant re-pick, but compare against the current (optimistic) selection
  // rather than the possibly-stale server `smart`: after a save resolves but before its
  // refetch lands, `smart` is still the old value, so guarding on it would silently drop a
  // quick toggle back and leave the controlled radio stuck on the pending pick.
  if (value === resyncMode.value) return
  const nextSmart = value === 'smart'
  resyncMode.value = value // optimistic; reverted below if the save fails
  syncMessage.value = null
  try {
    await saveMutation.mutateAsync({
      game: game.value,
      provider: src.provider as CollectionProvider,
      source: src.external_id,
      smart: nextSmart,
    })
  } catch (err) {
    resyncMode.value = smart.value ? 'smart' : 'mirror'
    syncMessage.value = err instanceof ApiError ? err.message : 'Could not update the re-sync mode.'
  }
}

// Re-sync runs in the background (throttled by the provider rate limit); poll its job to a
// terminal status via the shared poller (usePolledImportJob), tailoring the copy to smart
// vs. mirror. On completion, refresh the collection views and the saved-link timestamp.
const syncJob = usePolledImportJob(game, {
  onRunning: () => {
    syncMessage.value = smart.value
      ? `Smart-syncing from ${providerLabel.value}… this can take a couple of minutes.`
      : `Re-syncing from ${providerLabel.value}… this can take a couple of minutes.`
  },
  onComplete: (summary) => {
    if (!summary) {
      syncMessage.value = 'Re-sync complete.'
    } else if (summary.mode === 'smart') {
      syncMessage.value =
        `Smart-synced ${summary.matched_cards.toLocaleString()} cards` +
        (summary.stopped_early ? ' (stopped at already-synced cards).' : '.')
    } else {
      syncMessage.value =
        `Synced ${summary.matched_cards.toLocaleString()} cards` +
        (summary.removed_cards ? `, removed ${summary.removed_cards.toLocaleString()}.` : '.')
    }
    invalidateCollectionData(qc, game.value)
    qc.invalidateQueries({ queryKey: ['collection-source', game.value] })
  },
  onError: (error) => {
    syncMessage.value = error ?? 'Re-sync failed. Please try again.'
  },
})
const syncing = computed(() => syncMutation.isPending.value || syncJob.processing.value)

// A full re-sync mirrors (replace), so it can remove cards — confirm before running. A
// smart re-sync only updates recently-changed cards and never removes, so it's gentler.
async function resync() {
  if (!source.value || syncing.value) return
  const message = smart.value
    ? `Smart-sync updates recently-changed cards from your ${providerLabel.value} collection ` +
      "(it won't remove cards). Continue?"
    : `Re-syncing replaces your ${gameName.value} collection with your ${providerLabel.value} ` +
      'collection, removing cards that are no longer in it. Continue?'
  const ok = window.confirm(message)
  if (!ok) return
  syncMessage.value = 'Re-sync queued…'
  syncJob.reset()
  try {
    const job = await syncMutation.mutateAsync({ game: game.value })
    syncJob.start(job.job_id)
  } catch (err) {
    syncMessage.value = err instanceof ApiError ? err.message : 'Re-sync failed. Please try again.'
  }
}
</script>

<template>
  <!-- Import / re-sync from an external collection provider. -->
  <div class="mt-5 flex flex-wrap items-center gap-3">
    <ImportCollectionDialog :game="game" :source="source" />
    <template v-if="source">
      <div class="flex items-center gap-1">
        <Button variant="secondary" size="sm" :disabled="syncing" @click="resync">
          <RefreshCw :class="{ 'animate-spin': syncing }" />
          {{ smart ? 'Smart re-sync' : 'Re-sync' }} from {{ providerLabel }}
        </Button>
        <!-- Cog: switch how the saved link re-syncs (smart vs. full mirror). -->
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button variant="ghost" size="icon-sm" :disabled="savingResyncMode">
              <Settings />
              <span class="sr-only">Re-sync settings</span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" class="w-64">
            <DropdownMenuLabel>Re-sync mode</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuRadioGroup :model-value="resyncMode" @update:model-value="setResyncMode">
              <DropdownMenuRadioItem value="mirror">
                <span class="flex flex-col">
                  <span class="font-medium">Full mirror</span>
                  <span class="text-muted-foreground text-xs">
                    Mirror the collection exactly; removes cards no longer in it.
                  </span>
                </span>
              </DropdownMenuRadioItem>
              <DropdownMenuRadioItem value="smart">
                <span class="flex flex-col">
                  <span class="font-medium">Smart sync</span>
                  <span class="text-muted-foreground text-xs">
                    Only update recently-changed cards; won’t remove deleted cards.
                  </span>
                </span>
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <span class="text-muted-foreground text-sm">{{ lastSyncedText }}</span>
    </template>
  </div>
  <p v-if="syncMessage" class="text-muted-foreground mt-2 text-sm" aria-live="polite">
    {{ syncMessage }}
  </p>
</template>
