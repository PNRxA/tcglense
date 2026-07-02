//! HTTP router assembly: all routes plus the shared middleware stack and state.
//! Kept out of `main` so integration tests can drive the exact same router (CORS,
//! error mapping, auth, cache headers) in-process via `tower`'s `oneshot`.

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header},
    middleware::{from_fn, from_fn_with_state, map_response},
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    handlers::{
        auth::{
            complete_registration, forgot_password, login, logout, me, refresh, register,
            resend_verification, reset_password, verify_email,
        },
        cache::{conditional_request_layer, no_store_layer, public_cache_layer},
        catalog::{
            card_image, card_names, card_prices, card_prints, get_card, get_set, ingest_status,
            list_cards, list_games, list_set_cards, list_set_drops, list_sets, set_icon,
        },
        collection::{
            MAX_CSV_UPLOAD_BYTES, collection_set_drops, collection_sets, collection_summary,
            delete_collection_source, get_collection_entry, get_collection_source, get_import_job,
            import_collection, import_collection_csv, list_collection, owned_counts,
            save_collection_source, set_collection_entry, sync_collection_source,
        },
        health::health,
        sitemap::{sitemap_child, sitemap_index},
        wishlist::{
            get_wishlist_entry, list_wishlist, set_wishlist_entry, wishlist_counts,
            wishlist_set_drops, wishlist_sets, wishlist_summary,
        },
    },
    state::AppState,
};

/// CORS layer: allow the Vite dev origin with the required methods and headers.
/// `allow_credentials` is required because the browser sends the refresh cookie
/// (`credentials: 'include'`) on cross-origin refresh/logout; it is valid here
/// because the origin is an explicit value, never a wildcard.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(HeaderValue::from_static("http://localhost:5173"))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_credentials(true)
}

/// Build the application router: all routes plus the shared middleware stack and
/// state. Split out of `main` so integration tests can drive the exact same
/// router (CORS, error mapping, auth) in-process via `tower`'s `oneshot`.
pub fn build_router(state: AppState) -> Router {
    // Per-user, live, and side-effecting routes: auth (access tokens + Set-Cookie)
    // and the import-status route the SPA polls for live progress. These must never
    // be stored by the browser or a shared cache, so every response gets
    // `Cache-Control: no-store` (see `handlers::cache`).
    let private = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/register", post(register))
        // Finishes an email-first registration: consumes the emailed completion
        // token, sets the first password, and signs the account in.
        .route(
            "/api/auth/complete-registration",
            post(complete_registration),
        )
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        // Email verification + password reset: single-use emailed tokens. All
        // unauthenticated POSTs; the lookup-by-email ones answer generically.
        .route("/api/auth/verify-email", post(verify_email))
        .route(
            "/api/auth/resend-verification",
            post(resend_verification),
        )
        .route("/api/auth/forgot-password", post(forgot_password))
        .route("/api/auth/reset-password", post(reset_password))
        .route("/api/games/{game}/status", get(ingest_status))
        // Per-user card collections: reads + upserts of how many copies a signed-in
        // user owns, per game. Authenticated (via AuthUser) and no-store.
        .route("/api/collection/{game}", get(list_collection))
        .route("/api/collection/{game}/summary", get(collection_summary))
        // The sets a user owns cards in — the collection's per-set landing (mirrors the
        // catalog's game -> sets view), each dressed with catalog metadata + owned counts.
        .route("/api/collection/{game}/sets", get(collection_sets))
        // A drop-grouped owned set (e.g. Secret Lair) broken into Secret Lair drops,
        // paginated by drop — the collection mirror of the catalog's set-drops endpoint,
        // scoped to what the user owns.
        .route(
            "/api/collection/{game}/sets/{code}/drops",
            get(collection_set_drops),
        )
        // Batch owned-counts lookup for the browse-grid badges (external ids in, owned
        // counts out). POST so a big page's id list can't blow the URL length.
        .route("/api/collection/{game}/owned", post(owned_counts))
        // Import / sync a collection from an external provider (Archidekt or Moxfield):
        // a one-off import, a saved link (GET/PUT/DELETE), and a re-sync.
        .route("/api/collection/{game}/import", post(import_collection))
        // CSV upload: the raw file is the request body, so this route overrides axum's
        // default 2 MB body limit with our own (larger, but still bounded) cap. The limit
        // is layered on just this method-router so no other route's body ceiling changes.
        .route(
            "/api/collection/{game}/import/csv",
            post(import_collection_csv).layer(DefaultBodyLimit::max(MAX_CSV_UPLOAD_BYTES)),
        )
        .route(
            "/api/collection/{game}/import/jobs/{job_id}",
            get(get_import_job),
        )
        .route(
            "/api/collection/{game}/source",
            get(get_collection_source)
                .put(save_collection_source)
                .delete(delete_collection_source),
        )
        .route("/api/collection/{game}/sync", post(sync_collection_source))
        .route(
            "/api/collection/{game}/cards/{id}",
            get(get_collection_entry).put(set_collection_entry),
        )
        // Per-user wish lists: the collection's "want" twin (same holding shape and
        // routes, minus import/sync — a wish list has nothing to import). Authenticated
        // (via AuthUser) and no-store. Registered before the rate-limit layers below so
        // the per-user limiter wraps these routes too.
        .route("/api/wishlist/{game}", get(list_wishlist))
        .route("/api/wishlist/{game}/summary", get(wishlist_summary))
        .route("/api/wishlist/{game}/sets", get(wishlist_sets))
        .route(
            "/api/wishlist/{game}/sets/{code}/drops",
            get(wishlist_set_drops),
        )
        // Batch wanted-counts lookup for the browse-grid badges/ghosts (external ids
        // in, wanted counts out). POST so a big page's id list can't blow the URL
        // length. `/counts`, not `/owned` — a wish list doesn't track ownership.
        .route("/api/wishlist/{game}/counts", post(wishlist_counts))
        .route(
            "/api/wishlist/{game}/cards/{id}",
            get(get_wishlist_entry).put(set_wishlist_entry),
        )
        // Rate limiting, two complementary middlewares (each picks a quota by path
        // and no-ops for the rest): per-IP for the unauthenticated auth endpoints,
        // and per-user (keyed by the access-token user id) for the authenticated
        // collection + wishlist surfaces + `me`. Added before `no_store_layer` so a
        // 429 that either one short-circuits still gets `Cache-Control: no-store`
        // from that outer layer (a CDN must never pin a rate-limit response).
        .layer(from_fn_with_state(state.clone(), crate::ratelimit::rate_limit))
        .layer(from_fn_with_state(
            state.clone(),
            crate::ratelimit::user_rate_limit,
        ))
        .layer(map_response(no_store_layer));

    // Public, game-agnostic card catalog: the same for every visitor and changing
    // at most daily, so successful reads are browser- + CDN-cacheable
    // (`public, max-age=…, s-maxage=…, stale-while-revalidate=…`). The image/icon
    // routes set their own longer `immutable` header, which the layer preserves;
    // error responses are marked `no-store`. On top of that freshness policy,
    // `conditional_request_layer` adds an `ETag` to cacheable successes and answers a
    // matching `If-None-Match` with `304 Not Modified`, so a stale-cache revalidation
    // transfers headers instead of the whole body. It's layered *outside*
    // `public_cache_layer` so it can read the `Cache-Control` that layer set.
    let public = Router::new()
        .route("/api/games", get(list_games))
        .route("/api/games/{game}/sets", get(list_sets))
        .route("/api/games/{game}/sets/{code}", get(get_set))
        .route("/api/games/{game}/sets/{code}/icon", get(set_icon))
        .route("/api/games/{game}/sets/{code}/cards", get(list_set_cards))
        .route("/api/games/{game}/sets/{code}/drops", get(list_set_drops))
        .route("/api/games/{game}/cards", get(list_cards))
        // Distinct card-name autocomplete for the collection quick-add box. A sibling
        // of `/cards` (not `/cards/{name}`) so it never collides with `/cards/{id}`.
        .route("/api/games/{game}/card-names", get(card_names))
        .route("/api/games/{game}/cards/{id}", get(get_card))
        .route("/api/games/{game}/cards/{id}/image", get(card_image))
        .route("/api/games/{game}/cards/{id}/prices", get(card_prices))
        .route("/api/games/{game}/cards/{id}/prints", get(card_prints))
        // DB-backed sitemaps for crawlers: an index plus its child sitemaps
        // (pages / sets / chunked cards). Shared-cacheable like the rest of the
        // catalog; each success sets its own longer `Cache-Control`, which the
        // layer preserves, and a bad chunk 404s to `no-store`.
        .route("/api/sitemap.xml", get(sitemap_index))
        .route("/api/sitemaps/{name}", get(sitemap_child))
        .layer(map_response(public_cache_layer))
        .layer(from_fn(conditional_request_layer));

    Router::new()
        .merge(private)
        .merge(public)
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
