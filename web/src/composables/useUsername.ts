import type { Ref } from 'vue'
import { checkUsername, setUsername } from '@/lib/api'
import type { User, UsernameAvailability } from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'

// Server state for the opt-in username (issue #362). Setting one pushes the updated user
// into the auth store so every `auth.user` consumer (ProfileView, the visibility card's
// share link) repaints without a `/me` round-trip. The availability check is authed (the
// dialog is only reachable while signed in) and validation-only — it allocates nothing.

/** Set or change the signed-in user's username; the returned `User` carries the assigned
 * `#XXXX` discriminator and handle, which we cache into the auth store. */
export function useSetUsernameMutation() {
  const auth = useAuthStore()
  const options = {
    mutationFn: (token: string, vars: { username: string }) => setUsername(token, vars.username),
    onSuccess: (user: User) => auth.setUser(user),
  }
  return useAuthedMutation<User, { username: string }>(options)
}

/** Live availability/validity for the "choose a username" dialog. `username` is the
 * debounced candidate; `enabled` gates the request on a client-side validity pre-check
 * (length/charset) so it doesn't fire on every keystroke. */
export function useUsernameAvailabilityQuery(username: Ref<string>, enabled: Ref<boolean>) {
  const options = {
    queryKey: ['username-available', username],
    queryFn: (token: string) => checkUsername(token, username.value),
    enabled,
    staleTime: 30_000,
  }
  return useAuthedQuery<UsernameAvailability>(options)
}
