mod alerts;
mod analytics_cache;
mod auth;
mod captcha;
mod catalog;
mod client_ip;
mod collection_import;
mod config;
mod currency;
mod datasets;
mod db;
mod db_lock;
mod deck_import;
mod email;
mod entities;
mod error;
mod extract;
mod handlers;
mod migrator;
mod mtgjson;
mod notifications;
mod openapi;
mod phash;
mod ratelimit;
mod release_alerts;
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
use std::sync::atomic::Ordering;
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

    // Connect to the database (with SQLite WAL + cache pragmas; see `db`). Migrations
    // run later, in the background — see the startup task below.
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

    // Serve liveness immediately: bind the listener and run the boot migrations in the
    // background so `/api/health` answers within the platform's health-check window even
    // when a large migration takes minutes (an App Platform deploy is marked failed if
    // `/api/health` stays unreachable past the window). Close the startup gate for that
    // window so the router drains `/api/ready` and 503s application traffic until the
    // schema is ready — no handler runs against a half-migrated DB (see `build_router`).
    state.migrations_complete.store(false, Ordering::SeqCst);

    let app = build_router(state.clone());

    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("TCGLense API listening on http://{host}:{port}");

    // Run migrations, then open the gate and start background jobs. Spawned so the
    // listener above starts answering health checks right away; a migration *failure*
    // exits the process (rather than only aborting this task) so the deploy rolls back.
    let startup_state = state.clone();
    let startup_http = http.clone();
    tokio::spawn(async move { run_startup(startup_state, startup_http).await });

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

/// Run the boot-time schema migrations, then open the startup gate and kick off the
/// background jobs. Invoked on a spawned task so `axum::serve` can answer `/api/health`
/// while a long migration runs (see the startup gate in `build_router`).
async fn run_startup(state: AppState, http: reqwest::Client) {
    // Serialise migrations across simultaneously booting replicas: DDL races on a
    // shared Postgres otherwise (`seaql_migrations` is bookkeeping, not a lock). A
    // second booter *waits* here until the first finishes, then finds every
    // migration applied and no-ops. No-op on SQLite; fails open on lock errors.
    tracing::info!("acquiring the migration lock (waits if another replica is migrating)");
    let migration_lock =
        db_lock::AdvisoryLock::acquire(&state.db, &state.config.database_url, db_lock::MIGRATIONS)
            .await;
    if let Err(error) = Migrator::up(&state.db, None).await {
        // Exit the whole process (not just this task) so the orchestrator sees the boot
        // fail and rolls the deploy back, rather than serving 503s forever.
        tracing::error!(%error, "failed to run database migrations; shutting down");
        migration_lock.release().await;
        std::process::exit(1);
    }
    migration_lock.release().await;

    // Open the gate: the schema is ready, so the router may serve application traffic.
    state.migrations_complete.store(true, Ordering::SeqCst);
    tracing::info!("database migrations complete; serving application traffic");

    // A maintenance boot exists to apply migrations while the instance is drained;
    // don't start cleanup, catalog sync, or fingerprint jobs until normal service
    // resumes. Migrations have completed above.
    if state.config.maintenance_mode {
        tracing::warn!(
            "MAINTENANCE_MODE enabled; migrations completed and background tasks are disabled"
        );
    } else {
        // Spawn background maintenance (refresh-token pruning) and either the offline
        // dummy-catalog seed or the periodic card-data sync, per config.
        tasks::start(&state, &http).await;
    }
}
