use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::json;

use crate::state::AppState;

pub const MAINTENANCE_MESSAGE: &str = "service is under maintenance";

/// `GET /api/health` -> `200 { "status": "ok" }`.
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

/// `GET /api/ready` while maintenance mode is enabled.
///
/// Liveness stays healthy so an orchestrator does not restart the process, but
/// readiness fails explicitly so load balancers drain it before schema work.
pub async fn maintenance_ready() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "status": "maintenance" })),
    )
}

/// Non-probe response while maintenance mode is enabled.
pub async fn maintenance() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": MAINTENANCE_MESSAGE })),
    )
}

/// `GET /api/ready` -> a database-backed readiness result.
///
/// The query is deliberately portable across SQLite and Postgres. Database details
/// are logged server-side, while the response stays generic so infrastructure errors
/// cannot leak connection or schema information to an unauthenticated caller.
pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    let backend = state.db.get_database_backend();
    match state
        .db
        .query_one(Statement::from_string(backend, "SELECT 1"))
        .await
    {
        Ok(Some(_)) => (StatusCode::OK, Json(json!({ "status": "ready" }))),
        Ok(None) => {
            tracing::warn!("database readiness query returned no row");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "unavailable" })),
            )
        }
        Err(error) => {
            tracing::warn!(error = %error, "database readiness query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "unavailable" })),
            )
        }
    }
}
