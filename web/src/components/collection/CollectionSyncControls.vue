<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { RefreshCw } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import ImportCollectionDialog from '@/components/collection/ImportCollectionDialog.vue'
import { useGameName } from '@/composables/useCatalog'
import { invalidateCollectionData } from '@/composables/useCollection'
import {
  useCollectionSourceQuery,
  usePolledImportJob,
  useSyncCollectionSourceMutation,
} from '@/composables/useCollectionImport'
import { ApiError, providerLabel as providerName } from '@/lib/api'

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
      <Button variant="secondary" size="sm" :disabled="syncing" @click="resync">
        <RefreshCw :class="{ 'animate-spin': syncing }" />
        {{ smart ? 'Smart re-sync' : 'Re-sync' }} from {{ providerLabel }}
      </Button>
      <span class="text-muted-foreground text-sm">{{ lastSyncedText }}</span>
    </template>
  </div>
  <p v-if="syncMessage" class="text-muted-foreground mt-2 text-sm" aria-live="polite">
    {{ syncMessage }}
  </p>
</template>
