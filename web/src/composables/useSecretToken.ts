import { ref, watch, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

/**
 * Capture an emailed one-time token from `?token=…` into memory, then immediately
 * replace the visible URL without that credential. Other query state (notably the
 * post-registration `redirect`) is preserved. Watching also handles a second email
 * link opened into an already-mounted route component.
 */
export function useSecretToken(): Ref<string | null> {
  const route = useRoute()
  const router = useRouter()
  const token = ref<string | null>(null)

  watch(
    () => route.query.token,
    (raw) => {
      const hasTokenQuery = Object.prototype.hasOwnProperty.call(route.query, 'token')
      if (!hasTokenQuery) return

      // An explicitly present but malformed/empty value must replace any token
      // captured from an older link in this mounted component. The subsequent
      // scrub has no `token` key, so it deliberately leaves a valid capture alone.
      token.value = typeof raw === 'string' && raw ? raw : null

      const query = { ...route.query }
      delete query.token
      // Scrubbing is best-effort navigation hygiene; a duplicate/superseded replace
      // must not turn a valid captured token into an unhandled rejection.
      void router.replace({ path: route.path, query, hash: route.hash }).catch(() => {})
    },
    { immediate: true },
  )

  return token
}
