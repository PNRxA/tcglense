import { setAccent, type User } from '@/lib/api'
import { useAuthedMutation } from '@/lib/queries'
import type { Accent } from '@/lib/accent'
import { useAuthStore } from '@/stores/auth'

/** Persist the account's accent preset and replace the auth store's user with the
 * returned row — the accent store's computed then repaints `data-accent` immediately,
 * the same replace-don't-refetch shape as `useSetCurrencyMutation`. */
export function useSetAccentMutation() {
  const auth = useAuthStore()
  const options = {
    mutationFn: (token: string, vars: { accent: Accent }) => setAccent(token, vars.accent),
    onSuccess: (user: User) => auth.setUser(user),
  }
  return useAuthedMutation<User, { accent: Accent }>(options)
}
