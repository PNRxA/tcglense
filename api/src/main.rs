mod auth;
mod config;
mod entities;
mod error;
mod extract;
mod handlers;
mod migrator;
mod state;

use std::sync::Arc;

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
    let port = config.port;
    let database_url = config.database_url.clone();

    // Connect to the database and run migrations.
    let db = Database::connect(&database_url)
        .await
        .expect("failed to connect to the database");
    Migrator::up(&db, None)
        .await
        .expect("failed to run database migrations");

    let state = AppState {
        db,
        config: Arc::new(config),
    };

    // CORS: allow the Vite dev origin with the required methods and headers.
    let cors = CorsLayer::new()
        .allow_origin(
            "http://localhost:5173"
                .parse::<HeaderValue>()
                .expect("valid CORS origin"),
        )
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

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

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("TCGLense API listening on http://0.0.0.0:{port}");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
