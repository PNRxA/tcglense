mod auth;
mod catalog;
mod config;
mod db;
mod entities;
mod error;
mod extract;
mod handlers;
mod migrator;
mod scryfall;
mod state;

#[cfg(test)]
mod security_tests;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    routing::{get, post},
};
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{
    EnvFilter, Layer, filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::{
    catalog::images::ImageCache,
    config::Config,
    handlers::{
        auth::{login, logout, me, refresh, register},
        catalog::{
            card_image, card_prices, get_card, get_set, ingest_status, list_cards, list_games,
            list_set_cards, list_sets, set_icon,
        },
        health::health,
    },
    migrator::Migrator,
    state::AppState,
};

#[tokio::main]
async fn main() {
    // Load .env (best-effort; absence is fine).
    dotenvy::dotenv().ok();

    // Initialise tracing before reading config so config warnings are captured.
    // Log output is routed through `tracing-indicatif` so the card-import
    // progress bar (see `scryfall::progress`) never clobbers concurrent log
    // lines. Logs go to stdout (matching the prior `fmt()` default) while the bar
    // draws on stderr; `get_stdout_writer` keeps the two from colliding. The env
    // filter is attached to the fmt layer only — not globally — so a quieter
    // `RUST_LOG` (e.g. `warn`) still shows the one-shot import bar while
    // suppressing routine log lines. The indicatif layer is scoped to the import
    // span so unrelated spans (e.g. the per-request HTTP spans at debug level)
    // don't each sprout a bar; when stderr is not a TTY the bar renders nothing,
    // leaving logs untouched.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let indicatif_layer = IndicatifLayer::new();
    let log_writer = indicatif_layer.get_stdout_writer();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(log_writer)
                .with_filter(env_filter),
        )
        .with(indicatif_layer.with_filter(filter_fn(|meta| {
            meta.name() == crate::scryfall::PROGRESS_SPAN_NAME
        })))
        .init();

    let config = Config::from_env();
    let host = config.host.clone();
    let port = config.port;
    let database_url = config.database_url.clone();
    let sync_on_startup = config.sync_on_startup;
    let sync_interval_hours = config.sync_interval_hours;
    let seed_dummy_data = config.seed_dummy_data;
    let image_dir = config.data_dir.join("images");

    // Connect to the database (with SQLite WAL + cache pragmas; see `db`) and run
    // migrations.
    let db = Database::connect(db::connect_options(database_url))
        .await
        .expect("failed to connect to the database");
    Migrator::up(&db, None)
        .await
        .expect("failed to run database migrations");

    // Shared HTTP client for outbound provider calls (Scryfall data + images).
    // No overall timeout: the bulk download streams for a while. A read timeout
    // guards against a stalled connection.
    let http = reqwest::Client::builder()
        .user_agent(config.scryfall_user_agent.clone())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build the HTTP client");

    // The image proxy fetches with redirects disabled so a stored image URL can't
    // bounce the request to an unexpected host (the bulk download on `http` does
    // redirect to a storage CDN, so that client keeps the default redirect policy).
    let image_http = reqwest::Client::builder()
        .user_agent(config.scryfall_user_agent.clone())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to build the image HTTP client");

    // Precompute the timing-equalization dummy hash once (panics here at startup
    // are acceptable; a request-path hash failure must never silently disable it).
    let dummy_password_hash: Arc<str> = auth::password::hash_password("tcglense-timing-equalizer")
        .expect("hashing the timing-equalizer constant must succeed")
        .into();

    let state = AppState {
        db,
        config: Arc::new(config),
        dummy_password_hash,
        images: Arc::new(ImageCache::new(image_dir, image_http)),
    };

    // Periodically prune expired refresh tokens so the table can't grow unbounded.
    // The first tick fires immediately, then every 6 hours.
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
            loop {
                ticker.tick().await;
                match crate::auth::refresh::prune_expired(&db).await {
                    Ok(n) if n > 0 => tracing::info!("pruned {n} expired refresh tokens"),
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to prune expired refresh tokens")
                    }
                }
            }
        });
    }

    // SEED_DUMMY_DATA takes precedence over SYNC_ON_STARTUP / SYNC_INTERVAL_HOURS:
    // seed a small offline dummy catalog and perform NO network sync (no startup
    // import, no periodic refresh). We await it here rather than spawning — unlike
    // the ~500 MB real import it's a handful of local inserts, so the catalog is
    // present before the first request (handy for CI/e2e). A seed error is logged but
    // does not abort startup. Never enable this outside dev/CI/test.
    if seed_dummy_data {
        tracing::warn!(
            "SEED_DUMMY_DATA enabled: seeding a dummy offline catalog and skipping all \
             network card-data sync. Never enable this in production."
        );
        catalog::seed_all(&state.db).await;
    } else if sync_on_startup {
        // Import card data from each provider in the background so the server is
        // available immediately (the SPA shows import progress via the status route),
        // then re-import on a fixed interval to pick up Scryfall's newer prices/sets.
        // The import is idempotent and version-gated, so a tick with no upstream change
        // is cheap (a small bulk-data catalog check, no ~500 MB download).
        let db = state.db.clone();
        let http = http.clone();
        tokio::spawn(async move {
            if sync_interval_hours == 0 {
                // Periodic refresh disabled: import once on startup only.
                catalog::refresh_all(&db, &http).await;
                // Capture today's snapshot from the freshly-imported cards.
                catalog::snapshot_all(&db).await;
                return;
            }
            // saturating_mul so an absurd SYNC_INTERVAL_HOURS can't overflow the
            // u64: an overflow panics in debug and, worse, can wrap to a zero period
            // in release — which tokio::time::interval itself panics on, slipping
            // past the `== 0` guard above.
            let period = Duration::from_secs(sync_interval_hours.saturating_mul(60 * 60));
            let mut ticker = tokio::time::interval(period);
            // If a refresh ever runs long, skip the ticks it overran rather than
            // firing them back-to-back (the default Burst behaviour would).
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                // The first tick fires immediately (the startup import), then every
                // `sync_interval_hours` thereafter.
                ticker.tick().await;
                catalog::refresh_all(&db, &http).await;
                // Always capture a daily snapshot, even when the import above was
                // version-gated and skipped — keeps the price series continuous.
                catalog::snapshot_all(&db).await;
            }
        });
    } else {
        tracing::info!("SYNC_ON_STARTUP disabled; skipping card-data import");
    }

    let app = build_router(state);

    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("TCGLense API listening on http://{host}:{port}");

    axum::serve(listener, app).await.expect("server error");
}

/// CORS layer: allow the Vite dev origin with the required methods and headers.
/// `allow_credentials` is required because the browser sends the refresh cookie
/// (`credentials: 'include'`) on cross-origin refresh/logout; it is valid here
/// because the origin is an explicit value, never a wildcard.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(
            "http://localhost:5173"
                .parse::<HeaderValue>()
                .expect("valid CORS origin"),
        )
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_credentials(true)
}

/// Build the application router: all routes plus the shared middleware stack and
/// state. Split out of `main` so integration tests can drive the exact same
/// router (CORS, error mapping, auth) in-process via `tower`'s `oneshot`.
fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        // Public, game-agnostic card catalog.
        .route("/api/games", get(list_games))
        .route("/api/games/{game}/status", get(ingest_status))
        .route("/api/games/{game}/sets", get(list_sets))
        .route("/api/games/{game}/sets/{code}", get(get_set))
        .route("/api/games/{game}/sets/{code}/icon", get(set_icon))
        .route("/api/games/{game}/sets/{code}/cards", get(list_set_cards))
        .route("/api/games/{game}/cards", get(list_cards))
        .route("/api/games/{game}/cards/{id}", get(get_card))
        .route("/api/games/{game}/cards/{id}/image", get(card_image))
        .route("/api/games/{game}/cards/{id}/prices", get(card_prices))
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
