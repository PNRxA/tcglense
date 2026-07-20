import { createRouter, createWebHistory, type RouteLocationNormalized } from 'vue-router'
import { safeInternalPath } from '@/lib/utils'
import { reloadOnChunkError } from './reloadOnChunkError'
import { useAuthStore } from '@/stores/auth'
// HomeView stays eager (it's the landing page — lazy-loading it just adds a chunk RTT to
// the most common first paint). The auth/profile/email views are lazy: they're reached
// rarely and don't belong in the initial bundle.
import HomeView from '@/views/HomeView.vue'

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
    // (sibling to /cards, /collection, /wishlist) — an all-games landing, a per-game set-tile
    // landing, a flat browse/filter grid (every product, or scoped to one set), and a
    // per-product detail page with a price-history chart, mirroring the card views. Public,
    // like the catalog.
    {
      path: '/sealed',
      name: 'sealed',
      component: () => import('@/views/SealedGamesView.vue'),
    },
    {
      path: '/sealed/:game',
      name: 'game-sealed',
      component: () => import('@/views/SealedGameView.vue'),
      props: true,
    },
    // The flat filterable grid of a game's sealed products, or scoped to one set — the
    // click-through target of the landing's set tiles, mirroring the catalog's
    // /cards/:game/cards and /cards/:game/sets/:code split. Both static-lead paths outrank the
    // `/sealed/:game/:id` product-detail route below in vue-router's specificity ranking (a
    // static segment scores above a param at the same depth, regardless of declaration order),
    // so a `products` or `sets` slug is never captured as a product id.
    {
      path: '/sealed/:game/products',
      name: 'game-sealed-products',
      component: () => import('@/views/SealedBrowseView.vue'),
      props: true,
    },
    {
      path: '/sealed/:game/sets/:code',
      name: 'game-sealed-set',
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
    // The owned sealed products in a game, or scoped to one set — the sealed mirror of the
    // collection's card browse grids, clicked into from the landing's set tiles. Same public
    // route pattern (no requiresAuth; the view prompts a signed-out visitor to sign in).
    {
      path: '/collection/:game/products',
      name: 'game-collection-products',
      component: () => import('@/views/ProductHoldingsBrowseView.vue'),
      props: (route) => ({ game: route.params.game, list: 'collection' }),
    },
    {
      path: '/collection/:game/products/sets/:code',
      name: 'game-collection-product-set',
      component: () => import('@/views/ProductHoldingsBrowseView.vue'),
      props: (route) => ({ game: route.params.game, code: route.params.code, list: 'collection' }),
    },
    // Camera/webcam card scanner — OCRs a physical card and rapid-adds it to the
    // collection. Auth-gated (it writes holdings) and lazy-loaded: the on-device OCR
    // engine (tesseract.js) is a heavy payload that must not weigh down the app bundle,
    // so it only loads when this view — and then only when a scan starts — is reached.
    {
      path: '/scan',
      name: 'scan',
      component: () => import('@/views/ScanView.vue'),
      meta: { requiresAuth: true },
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
    // The wanted sealed products in a game, or scoped to one set — the wish-list twin of the
    // collection product browse grids, clicked into from the landing's set tiles. Same public
    // route pattern (no requiresAuth; the view prompts a signed-out visitor to sign in).
    {
      path: '/wishlist/:game/products',
      name: 'wishlist-products',
      component: () => import('@/views/ProductHoldingsBrowseView.vue'),
      props: (route) => ({ game: route.params.game, list: 'wishlist' }),
    },
    {
      path: '/wishlist/:game/products/sets/:code',
      name: 'wishlist-product-set',
      component: () => import('@/views/ProductHoldingsBrowseView.vue'),
      props: (route) => ({ game: route.params.game, code: route.params.code, list: 'wishlist' }),
    },
    // Per-user decks (issue #363): build and organise decks of cards into user-orderable
    // sections (Archidekt-style categories) and folders, with per-deck public sharing. Same
    // public-route pattern as the collection — a signed-out visitor gets an in-view sign-in
    // prompt rather than a bounce to /login.
    {
      path: '/decks',
      name: 'decks',
      component: () => import('@/views/DecksView.vue'),
    },
    {
      path: '/decks/:game',
      name: 'game-decks',
      component: () => import('@/views/GameDecksView.vue'),
      props: true,
    },
    // Static `needed` outranks the dynamic `:id` below in vue-router, so a deck id can't
    // shadow the cards-needed list (issue #499). Declared first for clarity too.
    {
      path: '/decks/:game/needed',
      name: 'deck-needed',
      component: () => import('@/views/DeckNeededView.vue'),
      props: true,
    },
    {
      path: '/decks/:game/:id',
      name: 'deck',
      component: () => import('@/views/DeckView.vue'),
      props: true,
    },
    // Public, shareable collections (issues #361/#362): a user's profile and the read-only
    // view of a game collection they've made public, addressed by their handle
    // (`{username}-{discriminator}`). No requiresAuth — anyone can view — and indexable, so
    // shared links preview and rank. Lazy-loaded like the rest.
    {
      path: '/u/:handle',
      name: 'public-profile',
      component: () => import('@/views/PublicProfileView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/:game',
      name: 'public-collection',
      component: () => import('@/views/PublicCollectionView.vue'),
      props: true,
    },
    // The read-only card grids for a public collection — every card, or scoped to one set
    // (mirrors the authed collection's /cards and /sets/:code split).
    {
      path: '/u/:handle/:game/cards',
      name: 'public-collection-cards',
      component: () => import('@/views/PublicCollectionBrowseView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/:game/sets/:code',
      name: 'public-collection-set',
      component: () => import('@/views/PublicCollectionBrowseView.vue'),
      props: true,
    },
    // The read-only sealed-product grids for a public collection — every owned product, or
    // scoped to one set — clicked into from the public landing's sealed set tiles (the public
    // mirror of the authed /collection/:game/products split). `products` is a static 3rd
    // segment, distinct from the `cards`/`sets` siblings above.
    {
      path: '/u/:handle/:game/products',
      name: 'public-collection-products',
      component: () => import('@/views/PublicProductBrowseView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/:game/products/sets/:code',
      name: 'public-collection-product-set',
      component: () => import('@/views/PublicProductBrowseView.vue'),
      props: true,
    },
    // Public, shareable wish lists (issue #493): the read-only mirror of the public collection
    // views above, under a static `wishlist` segment that outranks `/u/:handle/:game` in
    // vue-router (like the `decks` segment). Landing + card grids + sealed-product grids, all
    // read-only and indexable. `PublicProductBrowseView` is reused with `list: 'wishlist'`.
    {
      path: '/u/:handle/wishlist/:game',
      name: 'public-wishlist',
      component: () => import('@/views/PublicWishlistView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/wishlist/:game/cards',
      name: 'public-wishlist-cards',
      component: () => import('@/views/PublicWishlistBrowseView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/wishlist/:game/sets/:code',
      name: 'public-wishlist-set',
      component: () => import('@/views/PublicWishlistBrowseView.vue'),
      props: true,
    },
    {
      path: '/u/:handle/wishlist/:game/products',
      name: 'public-wishlist-products',
      component: () => import('@/views/PublicProductBrowseView.vue'),
      props: (route) => ({
        handle: route.params.handle,
        game: route.params.game,
        list: 'wishlist',
      }),
    },
    {
      path: '/u/:handle/wishlist/:game/products/sets/:code',
      name: 'public-wishlist-product-set',
      component: () => import('@/views/PublicProductBrowseView.vue'),
      props: (route) => ({
        handle: route.params.handle,
        game: route.params.game,
        code: route.params.code,
        list: 'wishlist',
      }),
    },
    // A shared public deck (issue #363), addressed by handle + deck id. The static `decks`
    // segment outranks the `/u/:handle/:game` public-collection routes, so no game slug can
    // ever be mistaken for it. Public + indexable, like the shared collections.
    {
      path: '/u/:handle/decks/:id',
      name: 'public-deck',
      component: () => import('@/views/PublicDeckView.vue'),
      props: true,
    },
    // Interactive public-API reference (issue #284). Public and indexable; linked from
    // the homepage, nav, and footer. Lazy-loaded — the Scalar bundle is heavy and must
    // stay out of the app's initial payload.
    { path: '/docs', name: 'docs', component: () => import('@/views/DocsView.vue') },
    // Legal pages, linked from the site footer. Public and indexable.
    { path: '/terms', name: 'terms', component: () => import('@/views/TermsView.vue') },
    { path: '/privacy', name: 'privacy', component: () => import('@/views/PrivacyPolicyView.vue') },
    {
      path: '/profile',
      name: 'profile',
      component: () => import('@/views/ProfileView.vue'),
      meta: { requiresAuth: true },
    },
    // Personal display preferences (card size, bulk threshold). Signed-in only, like the
    // profile page — the bulk threshold shapes the collection value the account owns.
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/SettingsView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { requiresGuest: true },
    },
    // CLI browser (loopback) sign-in consent page (RFC 8252 + PKCE). `tcglense
    // login` opens this with the loopback redirect + PKCE challenge in the query;
    // the user approves here and the browser relays a one-time code back to the
    // CLI. requiresAuth so a signed-out user is bounced to /login and returned.
    {
      path: '/cli-login',
      name: 'cli-login',
      component: () => import('@/views/CliLoginView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterView.vue'),
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
      component: () => import('@/views/CompleteRegistrationView.vue'),
    },
    {
      path: '/forgot-password',
      name: 'forgot-password',
      component: () => import('@/views/ForgotPasswordView.vue'),
    },
    {
      path: '/reset-password',
      name: 'reset-password',
      component: () => import('@/views/ResetPasswordView.vue'),
    },
    {
      path: '/verify-email',
      name: 'verify-email',
      component: () => import('@/views/VerifyEmailView.vue'),
    },
    // Catch-all 404. Kept last so every explicit route above wins first; the greedy
    // `/:pathMatch(.*)*` matches any otherwise-unrouted path (its param holds the
    // segments, if a future view wants to echo the attempted path). Public and
    // lazy-loaded; the view sets `noindex` so soft-404s stay out of the index.
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('@/views/NotFoundView.vue'),
    },
  ],
  // Restore the saved scroll position on back/forward; otherwise start a new page at the
  // top. A query/hash-only change to the SAME path (pagination via router.replace, the
  // ?card= dialog) must NOT scroll — returning false leaves the user's scroll alone.
  scrollBehavior(to, from, savedPosition) {
    if (savedPosition) return savedPosition
    if (to.path === from.path) return false
    return { top: 0 }
  },
})

let restorePromise: Promise<unknown> | null = null
let registrationCompletionPrepared = false

// Throttle for re-attempting a TRANSIENTLY-failed restore (see the guard body):
// at most one retry per window, so a redirect chain (or rapid navigation) can't
// fire a refresh POST per hop while the API is down.
const RESTORE_RETRY_COOLDOWN_MS = 5_000
let restoreRetryAt = 0

// Exported so the router-guard spec can register the real guard on a memory-history router.
export async function authGuard(to: RouteLocationNormalized) {
  const auth = useAuthStore()

  // The completion endpoint replaces the browser's refresh cookie with the new
  // account's session. Do not start (or await) a background restore for a possibly
  // different existing account on this route: those two Set-Cookie responses can
  // otherwise race. The store also invalidates any older in-flight restore's local
  // result and resolves chrome to the signed-out posture while the form is open.
  if (to.name === 'complete-registration') {
    restorePromise = null
    // The token scrub is a same-route router.replace; prepare only once for this
    // continuous visit so that replace cannot invalidate a submit started nearby.
    if (!registrationCompletionPrepared) {
      auth.prepareForRegistrationCompletion()
      registrationCompletionPrepared = true
    }
    return true
  }
  registrationCompletionPrepared = false

  // Kick off the one-time session restore on the first navigation and cache the PROMISE
  // (not a post-await boolean): concurrent initial navigations and every later navigation
  // then share this single restore instead of re-attempting the refresh. tryRestore()
  // uses the httpOnly refresh cookie and never throws.
  //
  // Tradeoff: only BLOCK on it for auth-gated routes. A public route must paint
  // immediately, so its chunk fetch and the session restore race in parallel rather than
  // the restore's RTT stalling every navigation on a high-latency link. The
  // flash-of-protected-UI this await used to prevent is now handled per component via
  // `auth.sessionResolved` (see UserMenu / HomeView / CollectionControls).
  // A restore that failed TRANSIENTLY (offline, a 5xx from the cold prod DB —
  // the refresh cookie may well still be valid) is re-attempted on a later
  // navigation instead of pinning the whole SPA session to one bad boot attempt;
  // re-arming BEFORE the kick-off lets the retried restore rescue THIS
  // navigation. Definitive failures (hard 401: the cookie is gone) stay cached,
  // so signed-out visitors never pay a refresh POST per navigation. Re-arming a
  // still-in-flight restore is harmless — tryRestore() single-flights.
  if (
    restorePromise &&
    !auth.isAuthenticated &&
    auth.restoreRecoverable &&
    Date.now() >= restoreRetryAt
  ) {
    restoreRetryAt = Date.now() + RESTORE_RETRY_COOLDOWN_MS
    restorePromise = null
  }
  restorePromise ??= auth.tryRestore()
  if (to.meta.requiresAuth || to.meta.requiresGuest) {
    await restorePromise
  }

  if (to.meta.requiresAuth && !auth.isAuthenticated) {
    // Remember where they were headed so login can send them back there.
    return { name: 'login', query: { redirect: to.fullPath } }
  }

  if (to.meta.requiresGuest && auth.isAuthenticated) {
    // Already signed in: honour a ?redirect= back to where they came from, else home.
    return safeInternalPath(to.query.redirect) ?? '/'
  }

  return true
}

router.beforeEach(authGuard)

// Recover from a lazy route chunk that went missing after a deploy: hard-navigate to the
// intended URL so a stale tab lands on the latest build instead of a dead click.
reloadOnChunkError(router)

export default router
