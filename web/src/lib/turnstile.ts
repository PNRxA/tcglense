// Cloudflare Turnstile loader + minimal typings.
//
// The widget is only used when `VITE_TURNSTILE_SITE_KEY` is set; otherwise the
// auth forms skip it entirely (dev, and any deploy that runs the API with
// CAPTCHA disabled). The API script is injected once, lazily, on first use.

const SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'

export const TURNSTILE_SITE_KEY: string | undefined = import.meta.env.VITE_TURNSTILE_SITE_KEY

/** Whether CAPTCHA is configured on the client (a site key is present). */
export const turnstileEnabled = Boolean(TURNSTILE_SITE_KEY)

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
