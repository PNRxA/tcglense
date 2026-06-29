mod auth;
mod config;
mod entities;
mod error;
mod extract;
mod handlers;
mod migrator;
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
    config::Config,
    handlers::{
        auth::{login, logout, me, refresh, register},
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

    // Connect to the database and run migrations.
    let db = Database::connect(&database_url)
        .await
        .expect("failed to connect to the database");
    Migrator::up(&db, None)
        .await
        .expect("failed to run database migrations");

    // Precompute the timing-equalization dummy hash once (panics here at startup
    // are acceptable; a request-path hash failure must never silently disable it).
    let dummy_password_hash: Arc<str> = auth::password::hash_password("tcglense-timing-equalizer")
        .expect("hashing the timing-equalizer constant must succeed")
        .into();

    let state = AppState {
        db,
        config: Arc::new(config),
        dummy_password_hash,
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
