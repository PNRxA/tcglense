import { createRouter, createWebHistory } from 'vue-router'
import { safeInternalPath } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'
import CompleteRegistrationView from '@/views/CompleteRegistrationView.vue'
import ForgotPasswordView from '@/views/ForgotPasswordView.vue'
import HomeView from '@/views/HomeView.vue'
import LoginView from '@/views/LoginView.vue'
import ProfileView from '@/views/ProfileView.vue'
import RegisterView from '@/views/RegisterView.vue'
import ResetPasswordView from '@/views/ResetPasswordView.vue'
import VerifyEmailView from '@/views/VerifyEmailView.vue'

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
    // Sealed products (booster boxes, bundles, decks): a top-level section of its own
    // (sibling to /cards, /collection, /wishlist) — an all-games landing, a per-game
    // browse/filter grid, and a per-product detail page with a price-history chart,
    // mirroring the card views. Public, like the catalog.
    {
      path: '/sealed',
      name: 'sealed',
      component: () => import('@/views/SealedGamesView.vue'),
    },
    {
      path: '/sealed/:game',
      name: 'game-sealed',
      component: () => import('@/views/SealedBrowseView.vue'),
      props: true,
    },
    {
      path: '/sealed/:game/:id',
      name: 'sealed-product',
      component: () => import('@/views/SealedProductView.vue'),
      props: true,
    },
    // Per-user collections. Public routes (no requiresAuth) so a signed-out visitor
    // can reach them; the game view prompts them to sign in / sign up rather than
    // bouncing to /login. Lazy-loaded like the catalog views.
    {
      path: '/collection',
      name: 'collection',
      component: () => import('@/views/CollectionsView.vue'),
    },
    {
      path: '/collection/:game',
      name: 'game-collection',
      component: () => import('@/views/GameCollectionView.vue'),
      props: true,
    },
    // Every owned card in a game, or scoped to one set — the collection's browse grids,
    // mirroring the catalog's /cards/:game/cards and /cards/:game/sets/:code split.
    {
      path: '/collection/:game/cards',
      name: 'game-collection-cards',
      component: () => import('@/views/CollectionBrowseView.vue'),
      props: true,
    },
    {
      path: '/collection/:game/sets/:code',
      name: 'game-collection-set',
      component: () => import('@/views/CollectionBrowseView.vue'),
      props: true,
    },
    // Per-user wish lists (issue #167) — the collection's twin for cards the user wants
    // to buy. Same public-route pattern: signed-out visitors get an in-view sign-in
    // prompt rather than a bounce to /login.
    {
      path: '/wishlist',
      name: 'wishlists',
      component: () => import('@/views/WishlistsView.vue'),
    },
    {
      path: '/wishlist/:game',
      name: 'game-wishlist',
      component: () => import('@/views/GameWishlistView.vue'),
      props: true,
    },
    // Every wishlisted card in a game, or scoped to one set — the wish list's browse
    // grids, mirroring the collection's /collection/:game/cards and .../sets/:code split.
    {
      path: '/wishlist/:game/cards',
      name: 'wishlist-cards',
      component: () => import('@/views/WishlistBrowseView.vue'),
      props: true,
    },
    {
      path: '/wishlist/:game/sets/:code',
      name: 'wishlist-set',
      component: () => import('@/views/WishlistBrowseView.vue'),
      props: true,
    },
    // Legal pages, linked from the site footer. Public and indexable.
    { path: '/terms', name: 'terms', component: () => import('@/views/TermsView.vue') },
    { path: '/privacy', name: 'privacy', component: () => import('@/views/PrivacyPolicyView.vue') },
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
    // Emailed-link + recovery routes. Deliberately public (no requiresGuest): a
    // signed-in user clicking an emailed complete-registration / verify / reset
    // link must reach the view — the guest guard would bounce them to '/' before
    // the token is consumed.
    {
      // Step two of the email-first registration: the emailed link lands here with
      // ?token=… to choose a password and sign in.
      path: '/complete-registration',
      name: 'complete-registration',
      component: CompleteRegistrationView,
    },
    {
      path: '/forgot-password',
      name: 'forgot-password',
      component: ForgotPasswordView,
    },
    {
      path: '/reset-password',
      name: 'reset-password',
      component: ResetPasswordView,
    },
    {
      path: '/verify-email',
      name: 'verify-email',
      component: VerifyEmailView,
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
    // Already signed in: honour a ?redirect= back to where they came from, else home.
    return safeInternalPath(to.query.redirect) ?? '/'
  }

  return true
})

export default router
