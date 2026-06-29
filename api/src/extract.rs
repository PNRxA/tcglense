use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
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
