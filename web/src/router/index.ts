import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import DashboardView from '@/views/DashboardView.vue'
import HomeView from '@/views/HomeView.vue'
import LoginView from '@/views/LoginView.vue'
import ProfileView from '@/views/ProfileView.vue'
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
      // Public landing page — shown to everyone, signed in or not.
      path: '/',
      name: 'home',
      component: HomeView,
    },
    // Public card catalog. Browsing card data needs no account; collection
    // features (future) will. Views are lazy-loaded so they don't weigh down the
    // initial auth bundle. `props: true` passes route params straight in.
    {
      path: '/cards',
      name: 'cards',
      component: () => import('@/views/CardsView.vue'),
    },
    {
      path: '/cards/:game',
      name: 'game',
      component: () => import('@/views/GameView.vue'),
      props: true,
    },
    {
      path: '/cards/:game/cards',
      name: 'game-cards',
      component: () => import('@/views/CardsBrowseView.vue'),
      props: true,
    },
    {
      path: '/cards/:game/cards/:id',
      name: 'card',
      component: () => import('@/views/CardDetailView.vue'),
      props: true,
    },
    {
      path: '/cards/:game/sets/:code',
      name: 'set',
      component: () => import('@/views/SetView.vue'),
      props: true,
    },
    {
      path: '/dashboard',
      name: 'dashboard',
      component: DashboardView,
      meta: { requiresAuth: true },
    },
    {
      path: '/profile',
      name: 'profile',
      component: ProfileView,
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
    // Remember where they were headed so login can send them back there.
    return { name: 'login', query: { redirect: to.fullPath } }
  }

  if (to.meta.requiresGuest && auth.isAuthenticated) {
    return { name: 'dashboard' }
  }

  return true
})

export default router
