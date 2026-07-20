import { request } from './client'
import type {
  AlertChannels,
  AlertTestResponse,
  CreateAlertRequest,
  PriceAlert,
  SetAlertChannelsRequest,
  UpdateAlertRequest,
} from './generated'

// ---------- Price alerts (per-user, session-only) ----------
//
// A price alert (issue #525) notifies the signed-in user when a card or sealed product
// crosses a below/above price threshold, over their configured channels (Discord webhook /
// Telegram bot / optional email). Unlike the collection/wish list, alerts span all games in
// one flat list, so there's no `makeHoldingApi` factory — a small hand-written client, like
// the API-keys surface. Every call is session-only (a real sign-in, never an API key), so it
// rides the auth store's `authFetch` token exactly like the deck calls.

export type {
  AlertChannels,
  AlertTarget,
  AlertTestResponse,
  AlertTestResult,
  CreateAlertRequest,
  PriceAlert,
  SetAlertChannelsRequest,
  UpdateAlertRequest,
} from './generated'

/** A card/product finish an alert can watch. */
export type AlertFinish = 'nonfoil' | 'foil' | 'etched'
/** Which way the price must move to fire. */
export type AlertDirection = 'below' | 'above'
/** What an alert targets. */
export type AlertTargetKind = 'card' | 'product'

// ----- Alerts CRUD -----

/** The signed-in user's price alerts (all games), most-recently-updated first. */
export function getAlerts(token: string): Promise<{ data: PriceAlert[] }> {
  return request<{ data: PriceAlert[] }>('/api/alerts', { token })
}

/** Create a price alert on a card or sealed product. */
export function createAlert(token: string, body: CreateAlertRequest): Promise<PriceAlert> {
  return request<PriceAlert>('/api/alerts', { method: 'POST', body, token })
}

/** Change an alert's finish / direction / threshold / active flag (absent = unchanged). */
export function updateAlert(
  token: string,
  id: number,
  body: UpdateAlertRequest,
): Promise<PriceAlert> {
  return request<PriceAlert>(`/api/alerts/${id}`, { method: 'PUT', body, token })
}

/** Delete an alert. */
export function deleteAlert(token: string, id: number): Promise<void> {
  return request<void>(`/api/alerts/${id}`, { method: 'DELETE', token })
}

// ----- Notification channels -----

/** The user's notification delivery settings (prefills the settings form). */
export function getAlertChannels(token: string): Promise<AlertChannels> {
  return request<AlertChannels>('/api/alerts/channels', { token })
}

/** Save the user's notification delivery settings. */
export function setAlertChannels(
  token: string,
  body: SetAlertChannelsRequest,
): Promise<AlertChannels> {
  return request<AlertChannels>('/api/alerts/channels', { method: 'PUT', body, token })
}

/** Send a test notification to every configured channel and report the per-channel outcome. */
export function testAlertChannels(token: string): Promise<AlertTestResponse> {
  return request<AlertTestResponse>('/api/alerts/channels/test', { method: 'POST', token })
}
