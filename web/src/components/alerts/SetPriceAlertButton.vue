<script setup lang="ts">
import { ref } from 'vue'
import { Bell } from '@lucide/vue'
import CreateAlertDialog from '@/components/alerts/CreateAlertDialog.vue'
import { Button } from '@/components/ui/button'
import type { AlertFinish, AlertTargetKind } from '@/lib/api'

// The "Set price alert" affordance (issue #525). It lives in the shared detail body
// (CardDetailContent / ProductDetailContent) rather than the page chrome, so it appears on
// BOTH the full detail page AND the browse-grid modal — the modal was previously the only way
// most people reached a card/product, so the page-only button was effectively invisible.
//
// Shown to everyone: signed-out visitors get the same button, and the dialog it opens nudges
// them to create an account (advertising the feature is the point, matching how
// CollectionControls shows its steppers' sign-in nudge). The signed-in vs signed-out split is
// the dialog's job — this stays a plain trigger.
defineProps<{
  game: string
  targetKind: AlertTargetKind
  externalId: string
  name: string
  // The finishes the target is priced in — forwarded to the dialog, which offers only these
  // and hides the picker when there's just one. See CardDetailContent / ProductDetailContent.
  finishes: AlertFinish[]
}>()

const open = ref(false)
</script>

<template>
  <div>
    <Button variant="outline" size="sm" type="button" class="w-full" @click="open = true">
      <Bell class="size-4" />
      Set price alert
    </Button>
    <CreateAlertDialog
      v-model:open="open"
      :game="game"
      :target-kind="targetKind"
      :external-id="externalId"
      :name="name"
      :finishes="finishes"
    />
  </div>
</template>
