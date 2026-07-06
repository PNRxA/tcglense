// Shared accessor for the public runtime config (GET /api/config): the Turnstile
// site key, and whether new signups are currently accepted.
//
// The value only changes on a redeploy, so the fetch is done once per page load
// and the promise cached. A *failed* fetch is NOT cached — it rethrows for this
// attempt but resets the cache so the next caller retries. Otherwise a single
// transient blip (API cold start, brief network error) would wedge everything
// keyed off the config (the CAPTCHA widget, the signup form) for the whole
// session, until a full page reload.

import { getConfig, type PublicConfig } from '@/lib/api'

let configPromise: Promise<PublicConfig> | null = null

export function publicConfig(): Promise<PublicConfig> {
  configPromise ??= getConfig().catch((err) => {
    configPromise = null
    throw err
  })
  return configPromise
}
