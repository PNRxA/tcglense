import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type { LoginPayload, User } from '@/lib/api'
import {
  ApiError,
  login as apiLogin,
  logout as apiLogout,
  me as apiMe,
  refresh as apiRefresh,
} from '@/lib/api'

// Cross-tab refresh coordination.
//
// The access token lives in memory (per tab), but the rotating, single-use
// refresh cookie is shared by the whole browser. When two tabs refresh at once —
// a browser session-restore opening several tabs, a `refetchOnReconnect` that
// fires in *every* open tab on a network blip, or just two tabs whose 15-minute
// access tokens expired together — they submit the *same* not-yet-rotated cookie
// in parallel. The server can only rotate it for the winner; the loser gets a
// 401 it would otherwise treat as a dead session (clearing local state, bouncing
// to /login). Serialize refreshes across tabs with the Web Locks API so they
// rotate the shared cookie one at a time: each waiter re-reads the cookie the
// previous holder just set and succeeds in turn, so no tab is spuriously logged
// out. Feature-detected — where Web Locks is unavailable (older Safari) we fall
// back to an unsynchronized refresh, which the server keeps race-safe (it no
// longer clears a live cookie on a benign concurrent double-submit).
const REFRESH_LOCK_NAME = 'tcglense-refresh'
// Bound how long a sibling waits for the lock before giving up and refreshing
// unsynchronized. The refresh POST has its own 15s timeout (see `refresh` in
// lib/api/auth.ts), but a serialized queue of slow refreshes (a session restore
// opening several tabs against a cold prod DB) can still exceed one request's
// worth of waiting.
const REFRESH_LOCK_WAIT_MS = 5_000

// Delay before the single retry of a 401-rejected refresh. The 401 may be the
// benign answer handed to the LOSER of a concurrent double-submit (a sibling
// tab won the rotation); by the retry the winner's Set-Cookie has landed in the
// jar, so the retry succeeds and no tab is spuriously signed out. A genuinely
// dead session 401s again immediately — that first 401 already cleared the
// cookie — so the retry is bounded and cannot loop.
const REFRESH_RETRY_DELAY_MS = 400
const REFRESH_SUPERSEDED_ERROR = 'refresh token superseded'

function isSupersededRefreshError(error: unknown): error is ApiError {
  return (
    error instanceof ApiError && error.status === 401 && error.message === REFRESH_SUPERSEDED_ERROR
  )
}

function withRefreshLock<T>(run: () => Promise<T>): Promise<T> {
  const locks = typeof navigator !== 'undefined' && 'locks' in navigator ? navigator.locks : null
  if (!locks) return run()

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), REFRESH_LOCK_WAIT_MS)
  return locks
    .request(REFRESH_LOCK_NAME, { signal: controller.signal }, run)
    .catch((error: unknown) => {
      // We waited too long for the lock (a sibling's refresh hung): give up on
      // coordination and refresh anyway — the server keeps concurrent refreshes
      // race-safe. Any other rejection is the refresh itself failing; rethrow it.
      if (error instanceof DOMException && error.name === 'AbortError') return run()
      throw error
    })
    .finally(() => clearTimeout(timer))
}

export const useAuthStore = defineStore('auth', () => {
  // Access token lives in memory only. The refresh token is an httpOnly cookie
  // (tcglense_refresh) and is invisible to JS.
  const accessToken = ref<string | null>(null)
  const user = ref<User | null>(null)

  const isAuthenticated = computed(() => Boolean(accessToken.value || user.value))

  // True while a signed-out state might still be rescued by retrying the refresh
  // cookie: the last refresh failed TRANSIENTLY (offline, a 5xx from the cold
  // prod DB, the client timeout) rather than with a definitive 401. The router
  // guard uses it to re-arm its cached one-time restore, so one bad boot attempt
  // doesn't pin the whole SPA session to signed-out (issue #417).
  const restoreRecoverable = ref(false)

  // One-way latch: false until the FIRST session restore settles (success OR failure)
  // or a session is adopted directly. It answers "has the initial session question been
  // answered?" — NOT "is the user signed in" — so it is set once and NEVER reset, not
  // even on logout. Consumers gate flash-of-wrong-state UI on it (see UserMenu,
  // HomeView, CollectionControls). Deliberately NOT derived from restoreInFlight:
  // authFetch re-invokes tryRestore on later 401s, so a derived flag would flap.
  const sessionResolved = ref(false)

  // How long to wait for the first restore before assuming the signed-out posture. The
  // refresh is a POST, which (unlike GETs) has no client timeout, so a half-open/captive-
  // portal socket can leave doRestore() pending forever. Without this watchdog the
  // sessionResolved-gated chrome (UserMenu, HomeView CTAs, CollectionControls, the
  // collection/wishlist prompts) would stay skeleton indefinitely. The latch is one-way,
  // so a restore that eventually succeeds still flips isAuthenticated and re-renders.
  const RESTORE_WATCHDOG_MS = 10_000

  async function login(payload: LoginPayload) {
    const response = await apiLogin(payload)
    accessToken.value = response.access_token
    user.value = response.user
  }

  // NOTE: there is deliberately no `register` action. Registration is email-first
  // and never mints a session (register only sends the completion link; the
  // session is minted by POST /api/auth/complete-registration), so the views call
  // the API fns directly — CompleteRegistrationView then adopts the returned
  // session via `setSession`. A register action writing into this store would
  // flip `isAuthenticated` and bounce the visitor off the guest routes.

  /** Adopt a session obtained outside `login` (registration completion): the
   * access token lives in memory, the refresh cookie was set by the server. */
  function setSession(token: string, u: User) {
    accessToken.value = token
    user.value = u
    sessionResolved.value = true
  }

  /** Replace the cached current user (e.g. after setting a username), so every
   * `auth.user` consumer repaints without a `/me` round-trip. */
  function setUser(u: User) {
    user.value = u
  }

  async function logout() {
    try {
      await apiLogout()
    } catch {
      // Best effort: revoke server-side if possible, but always clear locally.
    }
    accessToken.value = null
    user.value = null
    restoreRecoverable.value = false
  }

  // Single-flight guards: concurrent callers share one in-flight request so the
  // rotating, single-use refresh cookie is never submitted in parallel (which the
  // server treats as token reuse and would revoke the whole session).
  let refreshInFlight: Promise<boolean> | null = null
  let restoreInFlight: Promise<boolean> | null = null

  /** Mint a fresh access token from the refresh cookie. Clears state on failure. */
  function refresh(): Promise<boolean> {
    // Two guards, inner then outer: `refreshInFlight` coalesces concurrent callers
    // *within this tab* into one rotation of the single-use cookie; `withRefreshLock`
    // serializes rotations *across tabs* so siblings never submit the same cookie in
    // parallel (which the server can only honour for one of them).
    refreshInFlight ??= withRefreshLock(doRefresh).finally(() => {
      refreshInFlight = null
    })
    return refreshInFlight
  }

  async function doRefresh(): Promise<boolean> {
    if (await attemptRefresh()) return true

    // Transient failure (network blip, 5xx, timeout): the refresh cookie is
    // still valid server-side, so KEEP the local session and report failure —
    // clearing it here is what painted signed-out chrome (and bounced
    // requiresAuth routes to /login) for a hiccup that the next attempt
    // recovers from on its own. Only a 401 speaks to the session's validity.
    if (!(lastRefreshError instanceof ApiError) || lastRefreshError.status !== 401) {
      restoreRecoverable.value = true
      return false
    }

    // 401: retry once after a short delay (see REFRESH_RETRY_DELAY_MS — the
    // benign loser-of-a-concurrent-rotation case succeeds here).
    await new Promise((resolve) => setTimeout(resolve, REFRESH_RETRY_DELAY_MS))
    if (await attemptRefresh()) return true

    if (
      lastRefreshError instanceof ApiError &&
      lastRefreshError.status === 401 &&
      !isSupersededRefreshError(lastRefreshError)
    ) {
      // Definitively dead (the server cleared the cookie): clear local state.
      accessToken.value = null
      user.value = null
      restoreRecoverable.value = false
    } else {
      // The winner's Set-Cookie is not ordered against this tab's response. If
      // the retry was also Superseded, its request may simply have captured the
      // old cookie before the winner's response landed. Keep the session posture
      // and let a later refresh/router restore pick up the winner's live cookie.
      restoreRecoverable.value = true
    }
    return false
  }

  // The error behind the latest failed attemptRefresh(). Module-local mutable
  // state (not a ref): only doRefresh reads it, immediately after an attempt.
  let lastRefreshError: unknown = null

  async function attemptRefresh(): Promise<boolean> {
    try {
      const response = await apiRefresh()
      accessToken.value = response.access_token
      restoreRecoverable.value = false
      return true
    } catch (error) {
      lastRefreshError = error
      return false
    }
  }

  async function fetchMe() {
    const response = await authFetch((token) => apiMe(token))
    user.value = response.user
  }

  /**
   * Restore a session on app start. If an access token is already in memory we are
   * done; otherwise try the refresh cookie. Always resolves (never throws) so it is
   * safe to call from the router guard even when the API is unreachable. Single-
   * flighted so concurrent callers share one restore.
   */
  function tryRestore(): Promise<boolean> {
    restoreInFlight ??= doRestore().finally(() => {
      restoreInFlight = null
      // The initial session question is now answered (either way). Idempotent — the
      // latch only ever goes true, so re-settling on a later authFetch restore is a no-op.
      sessionResolved.value = true
    })
    // Watchdog: if the restore hasn't settled by the ceiling, degrade to signed-out so
    // the gated UI stops showing skeletons. Only armed once (while still unresolved).
    if (!sessionResolved.value) {
      setTimeout(() => {
        sessionResolved.value = true
      }, RESTORE_WATCHDOG_MS)
    }
    return restoreInFlight
  }

  async function doRestore(): Promise<boolean> {
    if (accessToken.value) {
      return true
    }
    try {
      if (await refresh()) {
        await fetchMe()
      }
    } catch {
      return false
    }
    return isAuthenticated.value
  }

  /**
   * Run an authenticated API call, transparently refreshing an expired access token.
   * Ensures a token exists (restoring via the cookie if needed), retries once on a
   * 401 after refreshing, and drops the local session if the retry still fails.
   * Reusable by other stores for their own protected calls.
   */
  async function authFetch<T>(call: (token: string) => Promise<T>): Promise<T> {
    if (!accessToken.value) {
      await tryRestore()
    }
    if (!accessToken.value) {
      throw new ApiError('Not authenticated', 401)
    }

    try {
      return await call(accessToken.value)
    } catch (error) {
      if (!(error instanceof ApiError) || error.status !== 401) {
        throw error
      }

      // Access token rejected: mint a fresh one once, then retry.
      if (!(await refresh()) || !accessToken.value) {
        throw error
      }

      try {
        return await call(accessToken.value)
      } catch (retryError) {
        if (retryError instanceof ApiError && retryError.status === 401) {
          // A freshly-minted access token was rejected. Drop the in-memory
          // session, but deliberately do NOT call logout(): that POSTs
          // /api/auth/logout, revoking the refresh cookie server-side — which
          // turns an infra-injected 401 (a proxy/WAF blip, a mid-deploy
          // mismatch) into a permanent, browser-wide logout. The refresh above
          // just SUCCEEDED, so the cookie is valid and the session is
          // recoverable: mark it so the router guard re-attempts a restore on a
          // later navigation (self-healing once the infra blip clears) instead
          // of stranding the user on signed-out chrome until a manual reload.
          accessToken.value = null
          user.value = null
          restoreRecoverable.value = true
        }
        throw retryError
      }
    }
  }

  return {
    accessToken,
    user,
    isAuthenticated,
    sessionResolved,
    restoreRecoverable,
    login,
    setSession,
    setUser,
    logout,
    refresh,
    fetchMe,
    tryRestore,
    authFetch,
  }
})
