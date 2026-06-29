<script setup lang="ts">
import { LogOut } from '@lucide/vue'
import { RouterLink, RouterView, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'

// Session restore happens once in the router guard (see router/index.ts).
const auth = useAuthStore()
const router = useRouter()

async function onLogout() {
  await auth.logout()
  await router.push('/login')
}
</script>

<template>
  <div class="bg-background text-foreground min-h-screen">
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-5xl items-center justify-between px-4">
        <RouterLink to="/" class="text-lg font-semibold tracking-tight">TCGLense</RouterLink>
        <div v-if="auth.isAuthenticated" class="flex items-center gap-3">
          <span class="text-muted-foreground hidden text-sm sm:inline">
            {{ auth.user?.display_name ?? auth.user?.email }}
          </span>
          <Button variant="outline" size="sm" @click="onLogout">
            <LogOut />
            Sign out
          </Button>
        </div>
      </div>
    </header>
    <main>
      <RouterView />
    </main>
  </div>
</template>
