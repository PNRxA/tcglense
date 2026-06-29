use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

/// Application-wide error type. Every variant maps to a specific HTTP status
/// and a JSON body of the shape `{ "error": "<message>" }`.
#[derive(Debug, Error)]
pub enum AppError {
    /// Input validation failed -> 422 Unprocessable Entity.
    #[error("{0}")]
    Validation(String),

    /// A conflicting resource already exists (e.g. duplicate email) -> 409.
    #[error("{0}")]
    Conflict(String),

    /// Login credentials did not match -> 401. Deliberately generic so it never
    /// reveals whether the email or the password was wrong.
    #[error("invalid email or password")]
    InvalidCredentials,

    /// Missing / malformed / invalid / expired token -> 401.
    #[error("{0}")]
    Unauthorized(String),

    /// Resource not found -> 404. Part of the required error surface; reserved
    /// for upcoming collection/set endpoints.
    #[allow(dead_code)]
    #[error("{0}")]
    NotFound(String),

    /// Unexpected internal failure -> 500. The detail is logged but never sent
    /// to the client.
    #[error("internal server error")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid email or password".to_string(),
            ),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Internal(detail) => {
                // Log the real detail server-side, return a generic message.
                tracing::error!(error = %detail, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(err: sea_orm::DbErr) -> Self {
        AppError::Internal(format!("database error: {err}"))
    }
}
