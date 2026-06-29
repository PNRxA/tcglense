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
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection: JsonRejection| AppError::Validation(rejection.body_text()))?;
        Ok(JsonBody(value))
    }
}
