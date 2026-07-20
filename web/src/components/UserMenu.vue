<script setup lang="ts">
import { computed } from 'vue'
import { Bell, LogIn, LogOut, Settings, User } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
} from '@/components/ui/navigation-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

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
  <!-- Session not yet resolved and no token: a neutral placeholder sized like the
       Sign-in button, so we don't flash "Sign in" at a user who's about to resolve
       signed in. -->
  <Skeleton v-if="!auth.sessionResolved && !auth.isAuthenticated" class="h-8 w-24 rounded-md" />

  <!-- Signed out (resolved): the profile selector collapses to a single sign-in action. -->
  <RouterLink
    v-else-if="!auth.isAuthenticated"
    :to="loginTo"
    :class="buttonVariants({ variant: 'outline', size: 'sm' })"
  >
    <LogIn />
    Sign in
  </RouterLink>

  <!-- Signed in: a profile menu mirroring the Cards/Collection nav triggers (same
       trigger style + rotating chevron + popover animation). It's its own
       NavigationMenu root — it sits on the opposite side of the header from MainNav,
       so there's no shared directional swipe to preserve — and viewport=false anchors
       the content to the trigger so it can right-align (end-0) instead of overflowing
       the screen edge. -->
  <NavigationMenu v-else :viewport="false" aria-label="Account">
    <NavigationMenuList>
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <User class="size-4" aria-hidden="true" />
          <span class="sr-only">Account menu</span>
        </NavigationMenuTrigger>
        <!-- The shared NavigationMenuContent only becomes `absolute md:w-auto` at the
             md breakpoint (below it, reka's default renders the panel inline/full-width
             for a stacked mobile nav). This menu is a floating dropdown at every width,
             so force `absolute top-full w-auto` here — otherwise on a phone it lays out
             static at the page top, clipping above the viewport and squashing the header.
             end-0 keeps it anchored to the trigger's right edge. -->
        <NavigationMenuContent class="absolute top-full left-auto end-0 w-auto">
          <ul class="grid w-56 gap-1">
            <li class="flex flex-col gap-0.5 px-2 py-1.5">
              <span class="text-sm font-medium">Signed in</span>
              <span class="text-muted-foreground truncate text-xs">{{ auth.user?.email }}</span>
            </li>
            <li>
              <!-- Override on the wrapper so cn()/tailwind-merge resolves the
                   flex-col→flex-row + gap conflict deterministically (not via CSS order). -->
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/profile">
                  <User aria-hidden="true" />
                  Profile
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/alerts">
                  <Bell aria-hidden="true" />
                  Price alerts
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/settings">
                  <Settings aria-hidden="true" />
                  Settings
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li>
              <NavigationMenuLink
                as-child
                class="text-destructive hover:text-destructive focus:text-destructive hover:bg-destructive/10 focus:bg-destructive/10 dark:hover:bg-destructive/20 dark:focus:bg-destructive/20 [&_svg:not([class*=text-])]:text-destructive w-full flex-row items-center gap-2 font-medium"
              >
                <button type="button" @click="onSignOut">
                  <LogOut aria-hidden="true" />
                  Sign out
                </button>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>
    </NavigationMenuList>
  </NavigationMenu>
</template>
