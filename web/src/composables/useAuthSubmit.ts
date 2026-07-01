import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ApiError } from '@/lib/api'
import { safeInternalPath } from '@/lib/utils'

/**
 * Shared submit flow for the login + register forms: owns the error + loading state
 * and runs the supplied auth-store `action`, mapping a failure to a user-facing
 * message and, on success, redirecting to the sanitized `?redirect=` path — i.e. back
 * to wherever the user was when they signed in — or the homepage when they came
 * straight to the login/register page. `action` is passed at call time so each form
 * supplies its own payload.
 */
export function useAuthSubmit() {
  const router = useRouter()
  const route = useRoute()

  const error = ref<string | null>(null)
  const loading = ref(false)

  async function submit(action: () => Promise<unknown>) {
    error.value = null
    loading.value = true
    try {
      await action()
      await router.push(safeInternalPath(route.query.redirect) ?? '/')
    } catch (err) {
      error.value =
        err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
    } finally {
      loading.value = false
    }
  }

  return { error, loading, submit }
}
