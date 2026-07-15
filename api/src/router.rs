//! HTTP router assembly: all routes plus the shared middleware stack and state.
//! Kept out of `main` so integration tests can drive the exact same router (CORS,
//! error mapping, auth, cache headers) in-process via `tower`'s `oneshot`.

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method},
    middleware::{from_fn, from_fn_with_state, map_response},
    routing::{any, delete, get, post, put},
    Router,
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    error::AppError,
    handlers::{
        api_keys::{create_api_key, list_api_keys, revoke_api_key},
        auth::{
            complete_registration, forgot_password, login, logout, me, refresh, register,
            resend_verification, reset_password, set_currency, set_username, username_available,
            verify_email,
        },
        cache::{
            conditional_request_layer, no_store_layer, public_cache_layer,
            public_holdings_cache_layer,
        },
        catalog::{
            card_image, card_names, card_prices, card_prints, card_sealed, get_card, get_product,
            get_set, ingest_status, list_cards, list_games, list_products, list_set_cards,
            list_set_drops, list_set_subtypes, list_sets, product_card_sections, product_cards,
            product_containers, product_contents, product_facets, product_image, product_prices,
            scan_cards, set_icon,
        },
        collection::{
            collection_movers, collection_set_drops, collection_set_subtypes, collection_sets,
            collection_summary, collection_value_history, delete_collection_source,
            export_collection, get_collection_entry, get_collection_source, get_import_job,
            import_collection, import_collection_csv, list_collection, owned_counts,
            save_collection_source, set_collection_entry, sync_collection_source,
            MAX_CSV_UPLOAD_BYTES,
        },
        config::public_config,
        currency::currency_rates,
        decks::{
            MAX_DECK_UPLOAD_BYTES, create_deck, create_folder, create_section, delete_deck,
            delete_folder, delete_section, export_deck, get_deck, import_deck, list_decks,
            list_folders, move_deck_card, move_deck_to_folder, reorder_sections, set_deck_card,
            set_deck_visibility, update_deck, update_folder, update_section,
        },
        health::{health, maintenance, maintenance_ready, ready},
        mirror::{
            fingerprint_index, mtgjson_all_printings, scryfall_bulk_data, scryfall_file,
            scryfall_sets, tcgcsv_proxy,
        },
        openapi::openapi_json,
        sharing::{
            get_collection_visibility, public_deck, public_decks, public_list, public_owned_counts,
            public_profile, public_set_drops, public_set_subtypes, public_sets, public_summary,
            set_collection_visibility,
        },
        sitemap::{sitemap_child, sitemap_index},
        wishlist::{
            get_wishlist_entry, get_wishlist_product_entry, list_wishlist, list_wishlist_products,
            set_wishlist_entry, set_wishlist_product_entry, wishlist_counts,
            wishlist_product_counts, wishlist_product_summary, wishlist_set_drops,
            wishlist_set_subtypes, wishlist_sets, wishlist_summary,
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

/// Minimal router used during planned maintenance. It deliberately does not merge
/// the application routes or static-SPA fallback: only process liveness stays up,
/// readiness drains the instance, and every other request gets a non-cacheable 503.
fn build_maintenance_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/ready", get(maintenance_ready))
        // The cached SPA's boot-time maintenance check. Keep this one application
        // endpoint live; it carries `maintenance_mode: true` and remains no-store.
        .route("/api/config", get(public_config))
        .fallback(maintenance)
        .layer(map_response(no_store_layer))
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .layer(from_fn(security_headers_middleware))
        .with_state(state)
}

/// Build the application router: all routes plus the shared middleware stack and
/// state. Split out of `main` so integration tests can drive the exact same
/// router (CORS, error mapping, auth) in-process via `tower`'s `oneshot`.
pub fn build_router(state: AppState) -> Router {
    if state.config.maintenance_mode {
        tracing::warn!(
            "MAINTENANCE_MODE enabled; serving liveness only and rejecting application traffic"
        );
        return build_maintenance_router(state);
    }

    // Per-user, live, and side-effecting routes: auth (access tokens + Set-Cookie)
    // and the import-status route the SPA polls for live progress. These must never
    // be stored by the browser or a shared cache, so every response gets
    // `Cache-Control: no-store` (see `handlers::cache`).
    let private = Router::new()
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        // Public runtime config for the SPA (the Turnstile site key). no-store: it
        // only changes on redeploy and must not be cached per-user/stale.
        .route("/api/config", get(public_config))
        // Daily reference rates used for display-only conversion. The service caches one
        // upstream snapshot process-wide and serves the last good copy through outages.
        .route("/api/currencies", get(currency_rates))
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
        // Set/change the username + a live availability check for the "choose a
        // username" dialog (issue #362). Both authed; the setter is `WritableUser`
        // (a read-only API key can't claim a handle).
        .route("/api/auth/username", put(set_username))
        .route("/api/auth/currency", put(set_currency))
        .route("/api/auth/username/available", get(username_available))
        // API-key management for the public API (issue #284). Session-only (a key
        // cannot manage keys — SessionUser rejects an API-key credential), so these
        // sit in the private group for `no-store` + the per-user rate limit.
        .route(
            "/api/auth/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route("/api/auth/api-keys/{id}", delete(revoke_api_key))
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
        // Per-game public-sharing toggle (issues #361/#362): read the current state and
        // enable/disable. `visibility` is a new static sibling under
        // `/api/collection/{game}` (static wins over `/cards/{id}` in axum). The setter is
        // `WritableUser` (a read-only key is 403); enabling without a username is a 409.
        .route(
            "/api/collection/{game}/visibility",
            get(get_collection_visibility).put(set_collection_visibility),
        )
        // The user's total collection value over time (add-date-clamped, re-priced from
        // historic snapshots) — the collection's answer to the per-card price chart.
        .route(
            "/api/collection/{game}/value-history",
            get(collection_value_history),
        )
        // The biggest day / week / month gain & loss movements across the user's owned
        // cards (per-unit price change × copies held) — top gainers and losers per window.
        .route("/api/collection/{game}/movers", get(collection_movers))
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
        // The same owned set grouped by card sub-type (treatment) — the collection mirror
        // of the catalog's set-subtypes endpoint, scoped to what the user owns.
        .route(
            "/api/collection/{game}/sets/{code}/subtypes",
            get(collection_set_subtypes),
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
        // The same wanted set grouped by card sub-type (treatment) — the wish-list mirror
        // of the catalog's set-subtypes endpoint, scoped to what the user wants.
        .route(
            "/api/wishlist/{game}/sets/{code}/subtypes",
            get(wishlist_set_subtypes),
        )
        // Batch wanted-counts lookup for the browse-grid badges/ghosts (external ids
        // in, wanted counts out). POST so a big page's id list can't blow the URL
        // length. `/counts`, not `/owned` — a wish list doesn't track ownership.
        .route("/api/wishlist/{game}/counts", post(wishlist_counts))
        .route(
            "/api/wishlist/{game}/cards/{id}",
            get(get_wishlist_entry).put(set_wishlist_entry),
        )
        // Sealed-product wants (issue #364): the wish list also holds sealed products, in
        // its own table and routes. The collection deliberately has no sealed surface.
        // `products` is a new static segment — no conflict with `/cards/{id}`, `/summary`,
        // `/sets`, or `/counts`.
        .route("/api/wishlist/{game}/products", get(list_wishlist_products))
        // Static siblings of `/products/{id}` — static segments win in axum (same guarantee
        // as the catalog's `/products/facets`), so neither is ever swallowed by a product id:
        // the sealed wish-list summary, and the batch wanted-counts lookup for the
        // product-tile badges (POST like `/counts`; absent ids mean "not wanted").
        .route(
            "/api/wishlist/{game}/products/summary",
            get(wishlist_product_summary),
        )
        .route(
            "/api/wishlist/{game}/products/counts",
            post(wishlist_product_counts),
        )
        .route(
            "/api/wishlist/{game}/products/{id}",
            get(get_wishlist_product_entry).put(set_wishlist_product_entry),
        )
        // Per-user decks (issues #363/#389): a user has many named decks per game, organised
        // into folders (deck level) and sections (card level), so the routes nest a
        // `{deck_id}` deeper than the flat collection/wishlist. Authenticated (AuthUser
        // reads, WritableUser writes) and no-store. Static segments (`folders`, `sections`,
        // `reorder`, `folder`, `visibility`, `move`) win over the dynamic ids in axum, so
        // none collide — same guarantee as the catalog's `/products/facets`.
        .route("/api/decks/{game}", get(list_decks).post(create_deck))
        // A deck import is inline (one provider object or one uploaded file), but the
        // JSON body may carry a sizeable CSV/text export, so give only this route the
        // same bounded 16 MiB ceiling as collection CSV upload.
        .route(
            "/api/decks/{game}/import",
            post(import_deck).layer(DefaultBodyLimit::max(MAX_DECK_UPLOAD_BYTES)),
        )
        .route(
            "/api/decks/{game}/folders",
            get(list_folders).post(create_folder),
        )
        .route(
            "/api/decks/{game}/folders/{folder_id}",
            put(update_folder).delete(delete_folder),
        )
        .route(
            "/api/decks/{game}/{deck_id}",
            get(get_deck).put(update_deck).delete(delete_deck),
        )
        .route(
            "/api/decks/{game}/{deck_id}/folder",
            put(move_deck_to_folder),
        )
        .route(
            "/api/decks/{game}/{deck_id}/visibility",
            put(set_deck_visibility),
        )
        .route(
            "/api/decks/{game}/{deck_id}/export",
            get(export_deck),
        )
        .route(
            "/api/decks/{game}/{deck_id}/sections",
            post(create_section),
        )
        .route(
            "/api/decks/{game}/{deck_id}/sections/reorder",
            put(reorder_sections),
        )
        .route(
            "/api/decks/{game}/{deck_id}/sections/{section_id}",
            put(update_section).delete(delete_section),
        )
        .route("/api/decks/{game}/{deck_id}/cards/{id}", put(set_deck_card))
        .route(
            "/api/decks/{game}/{deck_id}/cards/{id}/move",
            put(move_deck_card),
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
        .route(
            "/api/games/{game}/sets/{code}/subtypes",
            get(list_set_subtypes),
        )
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
        // The reverse composition relation: sealed products that directly contain this
        // product (for example, the boxes and bundles containing a booster pack).
        .route(
            "/api/games/{game}/products/{id}/containers",
            get(product_containers),
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
        // Public API documentation (issue #284): the machine-readable OpenAPI 3.1
        // document plus the interactive Scalar "try it out" UI. Both are the same for
        // every visitor and change only on redeploy, so they ride the shared CDN cache
        // like the rest of the catalog (each error still marked `no-store` by the layer).
        .route("/api/openapi.json", get(openapi_json))
        .layer(map_response(public_cache_layer))
        .layer(from_fn(conditional_request_layer));

    // Public, handle-keyed collection sharing (issues #361/#362): a read-only view of a
    // user's owned cards for a game they've made public, addressed by their handle
    // (`/api/u/{username}-{disc}/...`). Unauthenticated and keyed entirely by the URL, so
    // — unlike the per-user authed collection routes (`no-store`) — these are
    // CDN-cacheable, under a shorter-lived `PUBLIC_HOLDINGS_CACHE` (a collection changes
    // more often than the daily catalog). `conditional_request_layer` adds ETags/304s like
    // the catalog group; a private/unknown handle 404s to `no-store` (never CDN-pinned).
    let public_holdings = Router::new()
        .route("/api/u/{handle}", get(public_profile))
        .route("/api/u/{handle}/{game}", get(public_list))
        .route("/api/u/{handle}/{game}/summary", get(public_summary))
        .route("/api/u/{handle}/{game}/sets", get(public_sets))
        .route(
            "/api/u/{handle}/{game}/sets/{code}/drops",
            get(public_set_drops),
        )
        .route(
            "/api/u/{handle}/{game}/sets/{code}/subtypes",
            get(public_set_subtypes),
        )
        // Per-deck public sharing (issue #363): a user's public decks and one public deck's
        // full detail, addressed by handle. `decks` is a static sibling of `{game}` (static
        // wins in axum), so it never collides with a game slug. Deck ids are globally unique,
        // so no game segment is needed. Same CDN-cache + ETag layers as the reads above.
        .route("/api/u/{handle}/decks", get(public_decks))
        .route("/api/u/{handle}/decks/{deck_id}", get(public_deck))
        .layer(map_response(public_holdings_cache_layer))
        .layer(from_fn(conditional_request_layer));

    // The public show-ghosts owned-counts overlay (issue #361/#362 follow-up): which of the
    // posted catalog card ids the owner holds. A POST (the id list can be long), so — unlike
    // the GET public-holdings reads above — it is NOT CDN-cacheable: the response varies by
    // the request body, which a shared cache can't key on. Kept in its own group *outside*
    // the `public_holdings` cache/ETag layers and marked `no-store`, so it's never
    // shared-cached (mirrors why the authed `/api/collection/{game}/owned` twin is no-store).
    let public_holdings_owned = Router::new()
        .route("/api/u/{handle}/{game}/owned", post(public_owned_counts))
        .layer(map_response(no_store_layer));

    let mut app = Router::new()
        .merge(private)
        .merge(public)
        .merge(public_holdings)
        .merge(public_holdings_owned);

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
            // The visual-scanner fingerprint index — served from this origin's in-memory
            // index (no upstream), so self-hosts import it instead of hashing images.
            .route("/api/mirror/fingerprints/{game}", get(fingerprint_index))
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
        .layer(from_fn(security_headers_middleware))
        .with_state(state)
}

/// Browser hardening shared by API-only and combined-image deployments. The edge
/// proxy repeats these defensively, but applying them here also covers App Platform
/// and anyone exposing the API image directly. Existing route-specific headers are
/// preserved.
async fn security_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("strict-transport-security", "max-age=31536000"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "base-uri 'self'; object-src 'none'; frame-ancestors 'none'",
        ),
        (
            "permissions-policy",
            "geolocation=(), microphone=(), camera=(self)",
        ),
    ] {
        insert_header_if_missing(headers, name, value);
    }
    response
}

fn insert_header_if_missing(headers: &mut HeaderMap, name: &'static str, value: &'static str) {
    if !headers.contains_key(name) {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
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
