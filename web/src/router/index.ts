import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import DashboardView from '@/views/DashboardView.vue'
import LoginView from '@/views/LoginView.vue'
import RegisterView from '@/views/RegisterView.vue'

declare module 'vue-router' {
  interface RouteMeta {
    requiresAuth?: boolean
    requiresGuest?: boolean
  }
}

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: DashboardView,
      meta: { requiresAuth: true },
    },
    {
      path: '/login',
      name: 'login',
      component: LoginView,
      meta: { requiresGuest: true },
    },
    {
      path: '/register',
      name: 'register',
      component: RegisterView,
      meta: { requiresGuest: true },
    },
  ],
})

let restorePromise: Promise<unknown> | null = null

router.beforeEach(async (to) => {
  const auth = useAuthStore()

  // Restore the session exactly once, before the first routing decision, so we do
  // not briefly flash protected UI. Cache the promise (not a post-await boolean) so
  // concurrent initial navigations share a single restore instead of racing into
  // parallel refreshes. tryRestore() uses the httpOnly refresh cookie and never
  // throws, so the guard is safe even when the API is unreachable.
  restorePromise ??= auth.tryRestore()
  await restorePromise

  if (to.meta.requiresAuth && !auth.isAuthenticated) {
    return { name: 'login' }
  }

  if (to.meta.requiresGuest && auth.isAuthenticated) {
    return { name: 'dashboard' }
  }

  return true
})

export default router
