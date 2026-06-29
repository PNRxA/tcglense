use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

/// `GET /api/health` -> `200 { "status": "ok" }`.
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}
