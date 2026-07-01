<script setup lang="ts">
import { computed } from 'vue'
import { ChevronDown, LogIn, LogOut, User } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

const displayLabel = computed(() => auth.user?.display_name ?? auth.user?.email ?? 'Account')

// Sign-in returns the user to wherever they were (via ?redirect=). On an auth page
// itself, link plainly so the redirect never loops back to the login/register form —
// a direct visit to /login then just lands on the homepage after signing in.
const loginTo = computed(() => {
  if (route.name === 'login' || route.name === 'register') return '/login'
  return { path: '/login', query: { redirect: route.fullPath } }
})

async function onSignOut() {
  await auth.logout()
  await router.push('/')
}
</script>

<template>
  <!-- Signed out: the profile selector collapses to a single sign-in action. -->
  <RouterLink
    v-if="!auth.isAuthenticated"
    :to="loginTo"
    :class="buttonVariants({ variant: 'outline', size: 'sm' })"
  >
    <LogIn />
    Sign in
  </RouterLink>

  <!-- Signed in: a profile dropdown. Profile is the first item, Sign out the last. -->
  <DropdownMenu v-else>
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="sm" class="gap-2">
        <User />
        <span class="sr-only">Account menu</span>
        <span class="hidden max-w-[12rem] truncate sm:inline">{{ displayLabel }}</span>
        <ChevronDown class="size-4 opacity-60" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-56">
      <DropdownMenuLabel class="flex flex-col gap-0.5">
        <span>Signed in</span>
        <span class="text-muted-foreground truncate text-xs font-normal">
          {{ auth.user?.email }}
        </span>
      </DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuItem @select="() => router.push('/profile')">
        <User />
        Profile
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <DropdownMenuItem variant="destructive" @select="onSignOut">
        <LogOut />
        Sign out
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
