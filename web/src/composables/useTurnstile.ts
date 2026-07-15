import { onUnmounted, type Ref } from 'vue'
import { loadTurnstile, turnstileSiteKey } from '@/lib/turnstile'

/** Whole-operation deadline: runtime config + script/widget preparation + challenge. */
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
  let executionTail: Promise<void> = Promise.resolve()
  let unmounted = false

  function settle(token: string | null) {
    if (pending) {
      pending(token)
      pending = null
    }
  }

  async function ensureWidget(): Promise<boolean> {
    if (!container.value) return false
    if (widgetId !== undefined) return true
    // Site key comes from GET /api/config (cached); undefined = CAPTCHA disabled.
    const sitekey = await turnstileSiteKey()
    if (!sitekey || !container.value) return false
    const api = await loadTurnstile()
    // Guard against a re-entrant call that rendered while we awaited.
    if (widgetId !== undefined) return true
    if (unmounted || !container.value) return false
    widgetId = api.render(container.value, {
      sitekey,
      // Siteverify validates this expected action so a token minted for some
      // other widget/context cannot be replayed against an auth endpoint.
      action: 'auth',
      execution: 'execute',
      appearance: 'interaction-only',
      callback: (token) => settle(token),
      'error-callback': () => settle(null),
      'expired-callback': () => settle(null),
      'timeout-callback': () => settle(null),
    })
    return true
  }

  async function executeOnce(): Promise<string | null> {
    if (unmounted) return null
    let timedOut = false
    let timeoutId: ReturnType<typeof setTimeout> | undefined
    const timeout = new Promise<null>((resolve) => {
      timeoutId = setTimeout(() => {
        timedOut = true
        // If the widget is already awaiting a callback, release that promise too.
        settle(null)
        resolve(null)
      }, EXECUTE_TIMEOUT_MS)
    })

    const operation = (async (): Promise<string | null> => {
      let ready = false
      try {
        ready = await ensureWidget()
      } catch {
        // A failed script load is retryable on the next execute (loadTurnstile drops
        // its rejected cache); the server still decides whether a token is required.
        return null
      }
      // The timeout may have won while config/script preparation was still running.
      if (timedOut || unmounted || !ready || !window.turnstile || widgetId === undefined) {
        return null
      }

      const api = window.turnstile
      const id = widgetId
      const token = new Promise<string | null>((resolve) => {
        pending = resolve
      })
      try {
        // Each real execution gets a fresh single-use token. Install the resolver
        // first in case the provider reports a reset/execute error synchronously.
        api.reset(id)
        api.execute(id)
      } catch {
        settle(null)
      }
      return token
    })()

    try {
      return await Promise.race([operation, timeout])
    } finally {
      if (timeoutId !== undefined) clearTimeout(timeoutId)
    }
  }

  function execute(): Promise<string | null> {
    // Turnstile tokens are single-use. Queue callers that reach the shared widget
    // together so each gets a distinct reset/execute/callback cycle; sharing one
    // promise would hand the same token to two auth requests and make one fail.
    const result = executionTail.then(executeOnce, executeOnce)
    executionTail = result.then(
      () => undefined,
      () => undefined,
    )
    return result
  }

  onUnmounted(() => {
    unmounted = true
    settle(null)
    if (widgetId !== undefined && window.turnstile) {
      window.turnstile.remove(widgetId)
      widgetId = undefined
    }
  })

  return { execute }
}
