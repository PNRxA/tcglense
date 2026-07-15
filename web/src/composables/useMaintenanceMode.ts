import { computed, onScopeDispose, ref, watch } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { getConfig } from '@/lib/api'
import { onMaintenanceDetected } from '@/lib/maintenance'

/**
 * Global maintenance state. The uncached config read covers a newly loaded (even
 * CDN-cached) SPA, while the API-client signal covers a tab that was already open.
 * Vue Query rechecks when the tab becomes visible so the screen can both enter and leave
 * maintenance without a reload; there is deliberately no interval/timer poll.
 */
export function useMaintenanceMode() {
  const responseDetected = ref(false)
  const configQuery = useQuery({
    queryKey: ['public-config'],
    queryFn: getConfig,
    staleTime: Infinity,
    retry: false,
    refetchOnWindowFocus: 'always',
    refetchInterval: false,
  })

  const unsubscribe = onMaintenanceDetected(() => {
    responseDetected.value = true
  })
  onScopeDispose(unsubscribe)

  // A later successful config read is newer than any prior 503 signal. This is
  // what dismisses the maintenance screen after the flag is turned off and the
  // tab becomes visible again.
  watch(configQuery.dataUpdatedAt, () => {
    if (configQuery.data.value?.maintenance_mode === false) {
      responseDetected.value = false
    }
  })

  return computed(() => responseDetected.value || configQuery.data.value?.maintenance_mode === true)
}
