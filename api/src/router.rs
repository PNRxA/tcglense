//! HTTP router assembly: all routes plus the shared middleware stack and state.
//! Kept out of `main` so integration tests can drive the exact same router (CORS,
//! error mapping, auth, cache headers) in-process via `tower`'s `oneshot`.

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header},
    middleware::{from_fn, from_fn_with_state, map_response},
    routing::{any, get, post},
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    error::AppError,
    handlers::{
        auth::{
            complete_registration, forgot_password, login, logout, me, refresh, register,
            resend_verification, reset_password, verify_email,
        },
        cache::{conditional_request_layer, no_store_layer, public_cache_layer},
        catalog::{
            card_image, card_names, card_prices, card_prints, card_sealed, get_card, get_product,
            get_set, ingest_status, list_cards, list_games, list_products, list_set_cards,
            list_set_drops, list_sets, product_card_sections, product_cards, product_contents,
            product_facets, product_image, product_prices, scan_cards, set_icon,
        },
        collection::{
            MAX_CSV_UPLOAD_BYTES, collection_set_drops, collection_sets, collection_summary,
            delete_collection_source, export_collection, get_collection_entry,
            get_collection_source, get_import_job, import_collection, import_collection_csv,
            list_collection, owned_counts, save_collection_source, set_collection_entry,
            sync_collection_source,
        },
        config::public_config,
        health::health,
        mirror::{
            mtgjson_all_printings, scryfall_bulk_data, scryfall_file, scryfall_sets, tcgcsv_proxy,
        },
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
        // Public runtime config for the SPA (the Turnstile site key). no-store: it
        // only changes on redeploy and must not be cached per-user/stale.
        .route("/api/config", get(public_config))
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
        .route("/api/auth/resend-verification", post(resend_verification))
        .route("/api/auth/forgot-password", post(forgot_password))
        .route("/api/auth/reset-password", post(reset_password))
        .route("/api/games/{game}/status", get(ingest_status))
        // Visual card scanner: identify a photographed card from its client-computed
        // perceptual hash (only the 32-byte fingerprint is uploaded, never the image).
        // Auth-gated + no-store; a per-request POST body, so never CDN-cacheable.
        .route("/api/games/{game}/scan", post(scan_cards))
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
        // Download the whole collection as a provider-shaped CSV (Archidekt or Moxfield)
        // — the inverse of the CSV upload, and a re-importable round trip.
        .route("/api/collection/{game}/export", get(export_collection))
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
        .layer(from_fn_with_state(
            state.clone(),
            crate::ratelimit::rate_limit,
        ))
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
        // The sealed products this card is found in / can be pulled from (issue: card
        // sealed-product membership). A static-suffix sibling of `/prices` + `/prints`.
        .route("/api/games/{game}/cards/{id}/sealed", get(card_sealed))
        // Sealed products (booster boxes, bundles, decks, …) from TCGCSV. `facets`
        // is a static sibling of `/products/{id}` (static segments win in axum), so
        // it never collides with a product id.
        .route("/api/games/{game}/products", get(list_products))
        .route("/api/games/{game}/products/facets", get(product_facets))
        .route("/api/games/{game}/products/{id}", get(get_product))
        .route("/api/games/{game}/products/{id}/image", get(product_image))
        .route(
            "/api/games/{game}/products/{id}/prices",
            get(product_prices),
        )
        // The structural composition — "what's in the box" (packs, decks, promos, extras),
        // linking the sub-products it contains. A static-suffix sibling of `/prices`.
        .route(
            "/api/games/{game}/products/{id}/contents",
            get(product_contents),
        )
        // The cards this product is found to contain / can be pulled from (issue #204,
        // the reverse of `/cards/{id}/sealed`). A static-suffix sibling of `/prices`.
        .route("/api/games/{game}/products/{id}/cards", get(product_cards))
        // The non-empty display sections of those cards (+ counts), so the SPA can paginate
        // each section independently (issue #224). A deeper static sibling of `/cards`.
        .route(
            "/api/games/{game}/products/{id}/cards/sections",
            get(product_card_sections),
        )
        // DB-backed sitemaps for crawlers: an index plus its child sitemaps
        // (pages / sets / chunked cards / chunked sealed products). Canonically at
        // the site root (issue #294) so the sitemap-protocol scope rule holds; the
        // `/api/` aliases keep previously submitted URLs working. Explicit routes,
        // so they win over the combined image's SPA fallback; split deploys must
        // proxy the root paths to the API (see deploy/*.Caddyfile + the Vite dev
        // proxy). Shared-cacheable like the rest of the catalog; each success sets
        // its own longer `Cache-Control`, which the layer preserves, and a bad
        // chunk 404s to `no-store`.
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemaps/{name}", get(sitemap_child))
        .route("/api/sitemap.xml", get(sitemap_index))
        .route("/api/sitemaps/{name}", get(sitemap_child))
        .layer(map_response(public_cache_layer))
        .layer(from_fn(conditional_request_layer));

    let mut app = Router::new().merge(private).merge(public);

    // Optional dataset mirror (see `Config::mirror_enabled` + `handlers::mirror`). When
    // enabled, this instance re-serves the raw provider datasets so other TCGLense
    // instances can pull them from here (offloading Scryfall / MTGJSON / TCGCSV and
    // riding this origin's CDN). Off by default so an ordinary self-host isn't an open
    // proxy to the upstreams. Each handler sets its own CDN-cacheable `Cache-Control`,
    // which `public_cache_layer` preserves (and it marks any error `no-store`). Merged
    // before the `web_root` catch-all below so the mirror routes win over the SPA
    // fallback.
    if state.config.mirror_enabled {
        let mirror = Router::new()
            .route("/api/mirror/scryfall/bulk-data", get(scryfall_bulk_data))
            .route("/api/mirror/scryfall/sets", get(scryfall_sets))
            .route("/api/mirror/scryfall/file/{kind}", get(scryfall_file))
            .route(
                "/api/mirror/mtgjson/AllPrintings.json.gz",
                get(mtgjson_all_printings),
            )
            .route("/api/mirror/tcgcsv/{*path}", get(tcgcsv_proxy))
            .layer(map_response(public_cache_layer));
        app = app.merge(mirror);
    }

    // Optional static-SPA fallback (see `Config::web_root`). When `WEB_ROOT` is set,
    // any request the `/api/...` routes above didn't match is served from that
    // directory: a real file (e.g. `/assets/index-abc.js`) is returned directly, and
    // any unknown path falls back to `index.html` so client-side SPA routes resolve.
    // This is what lets the single-process "combined" Docker image serve both the API
    // and the SPA from one binary. Unset (the default) adds no fallback, so the API
    // keeps returning its normal JSON 404 for unmatched routes (existing API-only
    // deployments are untouched).
    //
    // `ServeDir::fallback` (not `not_found_service`, which force-overrides the status
    // to 404) so a deep-linked SPA route like `/collection/mtg` serves `index.html`
    // with a real `200` — a 404 would break crawlers, CDN caching, and monitoring.
    // Both `ServeDir`/`ServeFile` are infallible, so they slot in as the fallback.
    if let Some(web_root) = state.config.web_root.clone() {
        // A lowest-priority `/api/*` catch-all so an *unknown* API path stays a real
        // JSON 404 instead of being swallowed by the SPA fallback below (matching the
        // split deployment). Registered API routes — and their handler-level 404s
        // (unknown game/set/card) — are more specific and still take precedence.
        let serve_spa =
            ServeDir::new(&web_root).fallback(ServeFile::new(web_root.join("index.html")));
        let serve_spa = Router::new()
            .fallback_service(serve_spa)
            .layer(from_fn(spa_headers_middleware));

        app = app
            .route(
                "/api/{*rest}",
                any(|| async { AppError::NotFound("resource not found".to_string()) }),
            )
            .fallback_service(serve_spa);
    }

    app.layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Middleware to explicitly set Cache-Control headers for the static SPA assets
/// and HTML page fallbacks. This overrides any default/implicit 'private' caching
/// headers injected by host environments (like DigitalOcean App Platform).
async fn spa_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;

    let status = response.status();
    if status.is_success() || status == axum::http::StatusCode::NOT_MODIFIED {
        let headers = response.headers_mut();
        if path.starts_with("/assets/") {
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        } else {
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, no-cache"),
            );
        }
    }
    response
}
