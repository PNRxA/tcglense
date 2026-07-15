// Cloudflare Turnstile loader + minimal typings.
//
// The widget is only used when the API reports a Turnstile site key; otherwise the
// auth forms skip it entirely (dev, and any deploy that runs the API with CAPTCHA
// disabled). The site key is fetched from GET /api/config at runtime (not baked in
// at build time), so the published bundle needs no rebuild to change it. The API
// script is injected once, lazily, on first use.

import { publicConfig } from '@/lib/config'

const SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'
const SCRIPT_LOAD_TIMEOUT_MS = 15_000
const SCRIPT_MARKER = 'data-tcglense-turnstile'

/**
 * The Turnstile site key from the runtime config, or `undefined` to skip the
 * widget. Reads the shared (once-per-page, cached) `publicConfig()`, so a legit
 * null key when CAPTCHA is disabled server-side resolves `undefined`.
 *
 * A *failed* config fetch resolves `undefined` for this attempt (rather than
 * throwing): a single transient blip must not leave the widget permanently
 * unrendered — `publicConfig()` already dropped its cache, so the next call
 * retries. Otherwise every auth submit would send no token and the server would
 * reject each with a 400 until a full page reload.
 */
export function turnstileSiteKey(): Promise<string | undefined> {
  return publicConfig()
    .then((c) => c.turnstile_site_key ?? undefined)
    .catch(() => undefined)
}

export interface TurnstileRenderOptions {
  sitekey: string
  action?: string
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
  if (loadPromise) return loadPromise

  loadPromise = new Promise<TurnstileApi>((resolve, reject) => {
    const script = document.createElement('script')
    script.src = `${SCRIPT_SRC}&onload=__turnstileOnload`
    script.async = true
    script.defer = true
    script.setAttribute(SCRIPT_MARKER, '')

    let settled = false
    const cleanup = () => {
      clearTimeout(timer)
      script.onerror = null
      if (window.__turnstileOnload === onload) delete window.__turnstileOnload
    }
    const fail = (error: Error) => {
      if (settled) return
      settled = true
      cleanup()
      script.remove()
      loadPromise = null
      reject(error)
    }
    const onload = () => {
      if (settled) return
      if (!window.turnstile) {
        fail(new Error('Turnstile loaded without an API'))
        return
      }
      settled = true
      cleanup()
      resolve(window.turnstile)
    }

    window.__turnstileOnload = onload
    script.onerror = () => fail(new Error('failed to load Turnstile'))
    const timer = setTimeout(
      () => fail(new Error('timed out loading Turnstile')),
      SCRIPT_LOAD_TIMEOUT_MS,
    )
    document.head.appendChild(script)
  })
  return loadPromise
}
