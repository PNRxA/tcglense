import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ApiError } from '@/lib/api'
import { safeInternalPath } from '@/lib/utils'

/**
 * Shared submit flow for the sign-in form: owns the error + loading state and runs
 * the supplied auth-store `action`, mapping a failure to a user-facing message and,
 * on success, redirecting to the sanitized `?redirect=` path — i.e. back to wherever
 * the user was when they signed in — or the homepage when they came straight to the
 * login page. `action` is passed at call time so each form supplies its own payload.
 *
 * `errorStatus` carries the failed response's HTTP status (null for non-API
 * failures) so a view can branch on it — e.g. login's 403 "email not verified"
 * offers a resend link, which the message string alone can't signal reliably.
 */
export function useAuthSubmit() {
  const router = useRouter()
  const route = useRoute()

  const error = ref<string | null>(null)
  const errorStatus = ref<number | null>(null)
  const loading = ref(false)

  async function submit(action: () => Promise<unknown>) {
    error.value = null
    errorStatus.value = null
    loading.value = true
    try {
      await action()
      await router.push(safeInternalPath(route.query.redirect) ?? '/')
    } catch (err) {
      error.value =
        err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
      errorStatus.value = err instanceof ApiError ? err.status : null
    } finally {
      loading.value = false
    }
  }

  return { error, errorStatus, loading, submit }
}
