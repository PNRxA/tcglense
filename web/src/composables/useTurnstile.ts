import { onUnmounted, type Ref } from 'vue'
import { TURNSTILE_SITE_KEY, loadTurnstile, turnstileEnabled } from '@/lib/turnstile'

/** How long to wait for a Turnstile token before giving up on one request. */
const EXECUTE_TIMEOUT_MS = 20_000

/**
 * Manage one invisible Cloudflare Turnstile widget mounted in `container`, and
 * mint a fresh single-use token on demand. `execute()` resolves with the token,
 * or `null` when CAPTCHA is disabled (no site key) or the challenge fails/times
 * out — callers pass the token through as `captcha_token` (the server enforces
 * whether it's actually required, so `null` is safe when CAPTCHA is off).
 *
 * Pass the element ref of a `<div>` the view renders; the widget is invisible
 * (`execution: 'execute'`) and only surfaces a visible challenge if Turnstile
 * decides one is needed.
 */
export function useTurnstile(container: Ref<HTMLElement | undefined>) {
  let widgetId: string | undefined
  let pending: ((token: string | null) => void) | null = null

  function settle(token: string | null) {
    if (pending) {
      pending(token)
      pending = null
    }
  }

  async function ensureWidget(): Promise<boolean> {
    if (!turnstileEnabled || !TURNSTILE_SITE_KEY || !container.value) return false
    if (widgetId !== undefined) return true
    const api = await loadTurnstile()
    // Guard against a re-entrant call that rendered while we awaited.
    if (widgetId !== undefined) return true
    widgetId = api.render(container.value, {
      sitekey: TURNSTILE_SITE_KEY,
      execution: 'execute',
      appearance: 'interaction-only',
      callback: (token) => settle(token),
      'error-callback': () => settle(null),
      'expired-callback': () => settle(null),
      'timeout-callback': () => settle(null),
    })
    return true
  }

  async function execute(): Promise<string | null> {
    let ready = false
    try {
      ready = await ensureWidget()
    } catch {
      // Script failed to load: treat as no token. If the server requires one it
      // returns a clear error the view surfaces; if not, the flow proceeds.
      return null
    }
    if (!ready || !window.turnstile || widgetId === undefined) return null

    const api = window.turnstile
    const id = widgetId
    // Each call gets a fresh single-use token.
    api.reset(id)
    return new Promise<string | null>((resolve) => {
      const timer = setTimeout(() => settle(null), EXECUTE_TIMEOUT_MS)
      pending = (token: string | null) => {
        clearTimeout(timer)
        resolve(token)
      }
      api.execute(id)
    })
  }

  onUnmounted(() => {
    settle(null)
    if (widgetId !== undefined && window.turnstile) {
      window.turnstile.remove(widgetId)
      widgetId = undefined
    }
  })

  return { execute }
}
