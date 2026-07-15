// A narrow process-local signal from the shared API client to the root app. The
// runtime config query is the source of truth; this event closes the gap for an
// already-open tab by switching the shell as soon as any request receives the
// machine-readable maintenance response.

const MAINTENANCE_EVENT = 'maintenance'
const events = new EventTarget()

export const MAINTENANCE_ERROR_CODE = 'maintenance'

export function announceMaintenance(): void {
  events.dispatchEvent(new Event(MAINTENANCE_EVENT))
}

export function onMaintenanceDetected(listener: () => void): () => void {
  events.addEventListener(MAINTENANCE_EVENT, listener)
  return () => events.removeEventListener(MAINTENANCE_EVENT, listener)
}
