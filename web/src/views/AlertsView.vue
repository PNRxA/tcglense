<script setup lang="ts">
import { computed } from 'vue'
import { Bell } from '@lucide/vue'
import { Skeleton } from '@/components/ui/skeleton'
import AlertChannelsCard from '@/components/alerts/AlertChannelsCard.vue'
import AlertRow from '@/components/alerts/AlertRow.vue'
import { useAlertsQuery } from '@/composables/useAlerts'
import { usePageMeta } from '@/lib/seo'

// App-only account page, so keep it out of search indexes (like the profile/settings pages).
usePageMeta({ title: 'Price alerts', canonicalPath: '/alerts', noindex: true })

const alertsQuery = useAlertsQuery()
const alerts = computed(() => alertsQuery.data.value?.data ?? [])
</script>

<template>
  <div class="mx-auto max-w-2xl px-4 py-12">
    <div class="mb-8">
      <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
        <Bell class="size-7" /> Price alerts
      </h1>
      <p class="text-muted-foreground mt-2">
        Get notified when a card or sealed product crosses a price you set. Add an alert from any
        card or sealed-product page.
      </p>
    </div>

    <div class="grid gap-6">
      <AlertChannelsCard />

      <!-- Alerts list -->
      <section>
        <h2 class="mb-3 text-lg font-semibold">Your alerts</h2>

        <div v-if="alertsQuery.isPending.value" class="grid gap-2">
          <Skeleton v-for="n in 3" :key="n" class="h-20 w-full rounded-lg" />
        </div>

        <p
          v-else-if="alertsQuery.isError.value"
          class="text-destructive rounded-lg border p-4 text-sm"
        >
          Could not load your alerts. Please try again.
        </p>

        <div
          v-else-if="alerts.length === 0"
          class="text-muted-foreground rounded-lg border border-dashed p-8 text-center text-sm"
        >
          <Bell class="mx-auto mb-2 size-6 opacity-60" />
          <p>No alerts yet.</p>
          <p class="mt-1">
            Open a card or sealed-product page and choose
            <span class="font-medium">Set price alert</span>
            to add one.
          </p>
        </div>

        <div v-else class="grid gap-2">
          <AlertRow v-for="alert in alerts" :key="alert.id" :alert="alert" />
        </div>
      </section>
    </div>
  </div>
</template>
