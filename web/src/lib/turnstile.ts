// Cloudflare Turnstile loader + minimal typings.
//
// The widget is only used when the API reports a Turnstile site key; otherwise the
// auth forms skip it entirely (dev, and any deploy that runs the API with CAPTCHA
// disabled). The site key is fetched from GET /api/config at runtime (not baked in
// at build time), so the published bundle needs no rebuild to change it. The API
// script is injected once, lazily, on first use.

import { getConfig } from '@/lib/api'

const SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'

let siteKeyPromise: Promise<string | undefined> | null = null

/**
 * Fetch the Turnstile site key from the API once, lazily, and cache it (mirroring
 * `loadTurnstile`'s cached-promise idiom). A successful response is cached —
 * including a legitimately-null key when CAPTCHA is disabled server-side, which
 * resolves `undefined` and skips the widget.
 *
 * A *failed* fetch is NOT cached: it resolves `undefined` for this attempt but
 * resets the cache so the next call retries. Otherwise a single transient blip
 * (API cold start, brief network error) while CAPTCHA is enabled would leave the
 * widget permanently unrendered for the session — every auth submit sending no
 * token and the server rejecting each with a 400 until a full page reload.
 */
export function turnstileSiteKey(): Promise<string | undefined> {
  siteKeyPromise ??= getConfig()
    .then((c) => c.turnstile_site_key ?? undefined)
    .catch(() => {
      siteKeyPromise = null
      return undefined
    })
  return siteKeyPromise
}

export interface TurnstileRenderOptions {
  sitekey: string
  callback?: (token: string) => void
  'error-callback'?: () => void
  'expired-callback'?: () => void
  'timeout-callback'?: () => void
  execution?: 'render' | 'execute'
  appearance?: 'always' | 'execute' | 'interaction-only'
  size?: 'normal' | 'flexible' | 'compact'
}

export interface TurnstileApi {
  render(container: HTMLElement, options: TurnstileRenderOptions): string
  execute(container: HTMLElement | string, options?: Partial<TurnstileRenderOptions>): void
  reset(widgetId?: string): void
  remove(widgetId: string): void
  getResponse(widgetId?: string): string | undefined
}

declare global {
  interface Window {
    turnstile?: TurnstileApi
    __turnstileOnload?: () => void
  }
}

let loadPromise: Promise<TurnstileApi> | null = null

/** Load the Turnstile script once and resolve with the global API. */
export function loadTurnstile(): Promise<TurnstileApi> {
  if (window.turnstile) return Promise.resolve(window.turnstile)
  loadPromise ??= new Promise<TurnstileApi>((resolve, reject) => {
    window.__turnstileOnload = () => {
      if (window.turnstile) resolve(window.turnstile)
      else reject(new Error('Turnstile loaded without an API'))
    }
    const script = document.createElement('script')
    script.src = `${SCRIPT_SRC}&onload=__turnstileOnload`
    script.async = true
    script.defer = true
    script.onerror = () => reject(new Error('failed to load Turnstile'))
    document.head.appendChild(script)
  })
  return loadPromise
}
