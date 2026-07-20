import { useQueryClient } from '@tanstack/vue-query'
import {
  createAlert,
  deleteAlert,
  getAlertChannels,
  getAlerts,
  setAlertChannels,
  testAlertChannels,
  updateAlert,
} from '@/lib/api/alerts'
import type {
  AlertChannels,
  AlertTestResponse,
  ApiError,
  CreateAlertRequest,
  PriceAlert,
  SetAlertChannelsRequest,
  UpdateAlertRequest,
} from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// ---------- Price-alert query + mutation composables (issue #525) ----------
//
// Alerts are a per-user, cross-game surface managed only from a real session, so they live
// beside the collection/wish-list factories (not through them) and mirror `useDecks`'s
// idioms: every key head-starts with `alert(s)` so `useAuthCacheReset` wipes them on an
// identity change, and each option object is an intermediate variable with explicitly typed
// callbacks so TanStack's reactive types don't trip excess-property checks through the
// `useAuthed*` wrappers.

// ----- Reads -----

/** The signed-in user's price alerts (all games), newest edit first. */
export function useAlertsQuery() {
  const options = {
    queryKey: ['alerts'],
    queryFn: (token: string) => getAlerts(token),
    // Alerts carry a live target price; keep them fresh on remount rather than stale.
    staleTime: 0,
  }
  return useAuthedQuery<{ data: PriceAlert[] }>(options)
}

/** The user's notification delivery settings. */
export function useAlertChannelsQuery() {
  const options = {
    queryKey: ['alert-channels'],
    queryFn: (token: string) => getAlertChannels(token),
  }
  return useAuthedQuery<AlertChannels>(options)
}

// ----- Invalidation -----

function invalidateAlerts(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ['alerts'] })
}

// ----- Mutation variable shapes -----

export interface UpdateAlertVars {
  id: number
  body: UpdateAlertRequest
}

// ----- Mutations -----

export function useCreateAlertMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, body: CreateAlertRequest) => createAlert(token, body),
    onSettled: () => invalidateAlerts(qc),
  }
  return useAuthedMutation<PriceAlert, CreateAlertRequest>(options)
}

export function useUpdateAlertMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: UpdateAlertVars) => updateAlert(token, vars.id, vars.body),
    onSettled: () => invalidateAlerts(qc),
  }
  return useAuthedMutation<PriceAlert, UpdateAlertVars>(options)
}

export function useDeleteAlertMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, id: number) => deleteAlert(token, id),
    onSettled: () => invalidateAlerts(qc),
  }
  return useAuthedMutation<void, number>(options)
}

export function useSetAlertChannelsMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, body: SetAlertChannelsRequest) => setAlertChannels(token, body),
    onSettled: (data: AlertChannels | undefined) => {
      // Seed the cache with the saved settings so the form doesn't flash the pre-save value.
      if (data) qc.setQueryData(['alert-channels'], data)
      qc.invalidateQueries({ queryKey: ['alert-channels'] })
    },
  }
  return useAuthedMutation<AlertChannels, SetAlertChannelsRequest>(options)
}

/** Send a test notification. No cache effect — the result is shown transiently. */
export function useTestAlertChannelsMutation() {
  const options = {
    mutationFn: (token: string) => testAlertChannels(token),
    onSettled: (_d: AlertTestResponse | undefined, _e: ApiError | null) => {},
  }
  return useAuthedMutation<AlertTestResponse, void>(options)
}
