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
use tracing_subscriber::EnvFilter;

use crate::{
    catalog::images::ImageCache,
    config::Config,
    handlers::{
        auth::{login, logout, me, refresh, register},
        catalog::{
            card_image, get_card, get_set, ingest_status, list_cards, list_games, list_set_cards,
            list_sets, set_icon,
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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    let host = config.host.clone();
    let port = config.port;
    let database_url = config.database_url.clone();
    let sync_on_startup = config.sync_on_startup;
    let sync_interval_hours = config.sync_interval_hours;
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

    // Import card data from each provider in the background so the server is
    // available immediately (the SPA shows import progress via the status route),
    // then re-import on a fixed interval to pick up Scryfall's newer prices/sets.
    // The import is idempotent and version-gated, so a tick with no upstream change
    // is cheap (a small bulk-data catalog check, no ~500 MB download).
    if sync_on_startup {
        let db = state.db.clone();
        let http = http.clone();
        tokio::spawn(async move {
            if sync_interval_hours == 0 {
                // Periodic refresh disabled: import once on startup only.
                catalog::refresh_all(&db, &http).await;
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
            }
        });
    } else {
        tracing::info!("SYNC_ON_STARTUP disabled; skipping card-data import");
    }

    // CORS: allow the Vite dev origin with the required methods and headers.
    // allow_credentials is required because the browser sends the refresh cookie
    // (credentials: 'include') on cross-origin refresh/logout; it is valid here
    // because the origin is an explicit value, never a wildcard.
    let cors = CorsLayer::new()
        .allow_origin(
            "http://localhost:5173"
                .parse::<HeaderValue>()
                .expect("valid CORS origin"),
        )
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_credentials(true);

    let app = Router::new()
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
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("TCGLense API listening on http://{host}:{port}");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
