use axum::{
    Json,
    extract::{
        FromRequest, FromRequestParts, Path as AxumPath, Query as AxumQuery, Request,
        rejection::{JsonRejection, PathRejection},
    },
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// JSON request-body extractor that returns rejections in our standard JSON
/// error shape (`{ "error": ... }`) instead of axum's default text/plain body,
/// so every error a client receives is parseable JSON.
pub struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(rejection) => {
                // Log the detailed serde/axum reason server-side, but return a
                // fixed message (no parser internals echoed) with the HTTP status
                // that matches the failure kind instead of collapsing all to 422.
                tracing::debug!(rejection = %rejection.body_text(), "rejected JSON request body");
                Err(match rejection {
                    JsonRejection::JsonSyntaxError(_) => {
                        AppError::BadRequest("request body is not valid JSON".to_string())
                    }
                    JsonRejection::MissingJsonContentType(_) => AppError::UnsupportedMediaType(
                        "Content-Type must be application/json".to_string(),
                    ),
                    JsonRejection::JsonDataError(_) => AppError::Validation(
                        "request body does not match the expected schema".to_string(),
                    ),
                    JsonRejection::BytesRejection(_) => {
                        AppError::BadRequest("could not read request body".to_string())
                    }
                    _ => AppError::BadRequest("invalid request body".to_string()),
                })
            }
        }
    }
}

/// URL query-string extractor that returns rejections in our standard JSON error
/// shape (`{ "error": ... }`) instead of axum's default text/plain body, which echoes
/// the raw `serde_urlencoded` parser reason (e.g. "Failed to deserialize query string:
/// invalid digit found in string"). A malformed or mistyped query yields a fixed 400
/// message; the detailed reason is only logged.
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match AxumQuery::<T>::from_request_parts(parts, state).await {
            Ok(AxumQuery(value)) => Ok(Query(value)),
            Err(rejection) => {
                tracing::debug!(rejection = %rejection.body_text(), "rejected query string");
                Err(AppError::BadRequest("invalid query parameters".to_string()))
            }
        }
    }
}

/// URL path-parameter extractor that returns rejections in our standard JSON error
/// shape instead of axum's default text/plain body, which echoes the parameter name and
/// target type (e.g. "Cannot parse `job_id` with value `abc` to a `u64`"). A mistyped
/// segment yields a fixed 400; a missing-params rejection is a route/handler mismatch
/// (server bug), so it maps to a masked 500 rather than a client-facing message.
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match AxumPath::<T>::from_request_parts(parts, state).await {
            Ok(AxumPath(value)) => Ok(Path(value)),
            Err(rejection) => {
                let detail = rejection.body_text();
                tracing::debug!(rejection = %detail, "rejected path parameters");
                Err(match rejection {
                    PathRejection::FailedToDeserializePathParams(_) => {
                        AppError::BadRequest("invalid path parameter".to_string())
                    }
                    _ => AppError::Internal(format!("path extraction failed: {detail}")),
                })
            }
        }
    }
}
