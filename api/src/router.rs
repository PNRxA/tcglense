//! HTTP router assembly: all routes plus the shared middleware stack and state.
//! Kept out of `main` so integration tests can drive the exact same router (CORS,
//! error mapping, auth, cache headers) in-process via `tower`'s `oneshot`.

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    middleware::map_response,
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    handlers::{
        auth::{login, logout, me, refresh, register},
        cache::{no_store_layer, public_cache_layer},
        catalog::{
            card_image, card_prices, card_prints, get_card, get_set, ingest_status, list_cards,
            list_games, list_set_cards, list_set_drops, list_sets, set_icon,
        },
        health::health,
        sitemap::{sitemap_child, sitemap_index},
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
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
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
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/games/{game}/status", get(ingest_status))
        .layer(map_response(no_store_layer));

    // Public, game-agnostic card catalog: the same for every visitor and changing
    // at most daily, so successful reads are browser- + CDN-cacheable
    // (`public, max-age=…, s-maxage=…, stale-while-revalidate=…`). The image/icon
    // routes set their own longer `immutable` header, which the layer preserves;
    // error responses are marked `no-store`.
    let public = Router::new()
        .route("/api/games", get(list_games))
        .route("/api/games/{game}/sets", get(list_sets))
        .route("/api/games/{game}/sets/{code}", get(get_set))
        .route("/api/games/{game}/sets/{code}/icon", get(set_icon))
        .route("/api/games/{game}/sets/{code}/cards", get(list_set_cards))
        .route("/api/games/{game}/sets/{code}/drops", get(list_set_drops))
        .route("/api/games/{game}/cards", get(list_cards))
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
        .layer(map_response(public_cache_layer));

    Router::new()
        .merge(private)
        .merge(public)
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
