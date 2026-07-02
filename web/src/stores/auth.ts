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

  async function login(payload: LoginPayload) {
    const response = await apiLogin(payload)
    accessToken.value = response.access_token
    user.value = response.user
  }

  // NOTE: there is deliberately no `register` action. Registration mints no
  // session (the account must verify its email before it can sign in), so the
  // view calls the API fn directly; writing the returned user into this store
  // would flip `isAuthenticated` and bounce the visitor off the guest routes.

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
    })
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
    login,
    logout,
    refresh,
    fetchMe,
    tryRestore,
    authFetch,
  }
})
