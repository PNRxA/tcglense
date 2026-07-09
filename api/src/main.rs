mod auth;
mod captcha;
mod catalog;
mod client_ip;
mod collection_import;
mod config;
mod datasets;
mod db;
mod email;
mod entities;
mod error;
mod extract;
mod handlers;
mod migrator;
mod mtgjson;
mod openapi;
mod phash;
mod ratelimit;
mod router;
mod scryfall;
mod state;
mod tasks;
mod tcgcsv;

#[cfg(test)]
mod integration_pg;
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod test_support;

use std::net::SocketAddr;
use std::time::Duration;

use sea_orm::{ConnectionTrait, Database};
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{
    EnvFilter, Layer, filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::{config::Config, migrator::Migrator, state::AppState};

// Re-export so `crate::build_router` keeps resolving for the integration tests.
pub use router::build_router;

#[tokio::main]
async fn main() {
    // Load .env (best-effort; absence is fine).
    dotenvy::dotenv().ok();

    // Pin the rustls crypto provider for the whole process before any TLS config is
    // built. reqwest (HTTPS to the card-data providers), SeaORM/sqlx (Postgres TLS),
    // and redis (`rediss://`, e.g. Upstash) all share one process-wide rustls, and
    // 0.23 refuses to pick a provider when more than one is linked. aws-lc-rs is the
    // backend the build already compiles (aws-lc-sys); install it as the default. An
    // `Err` just means a default is already installed, so the result is ignored.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialise tracing before reading config so config warnings are captured.
    // Log output is routed through `tracing-indicatif` so the startup progress
    // bars (the Scryfall card import — see `scryfall::progress` — and the TCGCSV
    // product sweep + historic price backfill — see `tcgcsv::progress`) never
    // clobber concurrent log lines. Logs go to stdout (matching the prior `fmt()`
    // default) while the bar draws on stderr; `get_stdout_writer` keeps the two
    // from colliding. The env filter is attached to the fmt layer only — not
    // globally — so a quieter `RUST_LOG` (e.g. `warn`) still shows the one-shot
    // import bars while suppressing routine log lines. The indicatif layer is
    // scoped to those import spans so unrelated spans (e.g. the per-request HTTP
    // spans at debug level) don't each sprout a bar; when stderr is not a TTY the
    // bar renders nothing, leaving logs untouched.
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
            let name = meta.name();
            name == crate::scryfall::PROGRESS_SPAN_NAME
                || name == crate::tcgcsv::PROGRESS_SPAN_NAME
                || name == crate::mtgjson::PROGRESS_SPAN_NAME
        })))
        .init();

    let config = Config::from_env();
    // Loud startup warnings for an insecure production posture (plaintext refresh
    // cookie / no CAPTCHA / in-memory rate limiter on an internet-facing deploy).
    // Advisory only — never changes behaviour; no-op in a local-dev posture.
    config.warn_insecure_production_posture();
    let host = config.host.clone();
    let port = config.port;
    let database_url = config.database_url.clone();

    // Connect to the database (with SQLite WAL + cache pragmas; see `db`) and run
    // migrations.
    let db = Database::connect(db::connect_options(database_url))
        .await
        .expect("failed to connect to the database");
    // Report the backend the DATABASE_URL scheme selected (both sqlx-sqlite and
    // sqlx-postgres are compiled in; sea-orm dispatched on the scheme). MySQL isn't
    // compiled in, but the enum is non-exhaustive so it's matched for completeness.
    let backend = match db.get_database_backend() {
        sea_orm::DatabaseBackend::Postgres => "PostgreSQL",
        sea_orm::DatabaseBackend::Sqlite => "SQLite",
        sea_orm::DatabaseBackend::MySql => "MySQL",
    };
    tracing::info!("connected to {backend} database");
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

    // Optional Redis backing the rate limiters. When REDIS_URL is set but Redis is
    // unreachable at boot we start DEGRADED (in-memory) with a warning rather than
    // crash-looping a rolling deploy — rate limiting is abuse protection, not
    // integrity, so it fails open (ConnectionManager also auto-reconnects once up).
    let redis = match config.redis_url.as_deref() {
        Some(url) => match ratelimit::connect_redis(url).await {
            Ok(conn) => {
                tracing::info!("connected to Redis; rate limiters are distributed");
                Some(conn)
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "REDIS_URL is set but connecting to Redis failed at startup; \
                     rate limiting will run in-memory (per-process) until restart"
                );
                None
            }
        },
        None => None,
    };

    // Precomputing the timing-equalization dummy hash can fail; panicking here at
    // startup is acceptable (a request-path hash failure must never silently disable
    // it), so `.expect` stays at this call site per the "expect only in main.rs" rule.
    let state = AppState::new(config, db, http.clone(), image_http, redis)
        .expect("failed to assemble application state");

    // Spawn background maintenance (refresh-token pruning) and either the offline
    // dummy-catalog seed or the periodic card-data sync, per config.
    tasks::start(&state, &http).await;

    let app = build_router(state);

    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("TCGLense API listening on http://{host}:{port}");

    // `into_make_service_with_connect_info` surfaces the socket peer address as a
    // `ConnectInfo<SocketAddr>` extension so the auth rate limiter can key on the
    // client IP (see `ratelimit` / `client_ip`).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server error");
}
