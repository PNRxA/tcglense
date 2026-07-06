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

export const useAuthStore = defineStore('auth', () => {
  // Access token lives in memory only. The refresh token is an httpOnly cookie
  // (tcglense_refresh) and is invisible to JS.
  const accessToken = ref<string | null>(null)
  const user = ref<User | null>(null)

  const isAuthenticated = computed(() => Boolean(accessToken.value || user.value))

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

  async function logout() {
    try {
      await apiLogout()
    } catch {
      // Best effort: revoke server-side if possible, but always clear locally.
    }
    accessToken.value = null
    user.value = null
  }

  // Single-flight guards: concurrent callers share one in-flight request so the
  // rotating, single-use refresh cookie is never submitted in parallel (which the
  // server treats as token reuse and would revoke the whole session).
  let refreshInFlight: Promise<boolean> | null = null
  let restoreInFlight: Promise<boolean> | null = null

  /** Mint a fresh access token from the refresh cookie. Clears state on failure. */
  function refresh(): Promise<boolean> {
    refreshInFlight ??= doRefresh().finally(() => {
      refreshInFlight = null
    })
    return refreshInFlight
  }

  async function doRefresh(): Promise<boolean> {
    try {
      const response = await apiRefresh()
      accessToken.value = response.access_token
      return true
    } catch {
      accessToken.value = null
      user.value = null
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
   * 401 after refreshing, and logs out if the retry still fails. Reusable by other
   * stores for their own protected calls.
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
          await logout()
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
    login,
    setSession,
    logout,
    refresh,
    fetchMe,
    tryRestore,
    authFetch,
  }
})
