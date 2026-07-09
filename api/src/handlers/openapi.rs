//! Serve the OpenAPI document and the interactive Scalar UI (issue #284).
//!
//! Both are public, CDN-cacheable GET routes wired into the `public` group in
//! [`crate::router`]: `GET /api/openapi.json` returns the raw OpenAPI 3.1 JSON, and
//! `GET /api/docs` returns a self-contained Scalar "try it out" page that embeds that
//! same spec. The spec is materialized from [`crate::openapi::ApiDoc`].

use axum::{Json, response::Html, response::IntoResponse};
use utoipa::OpenApi;

use crate::openapi::ApiDoc;

/// `GET /api/openapi.json` -> the machine-readable OpenAPI 3.1 description of the API.
pub async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

/// `GET /api/docs` -> the interactive Scalar API reference, with the spec embedded so
/// the page renders without a second round-trip. Scalar's own assets load from its CDN.
pub async fn scalar_ui() -> impl IntoResponse {
    Html(utoipa_scalar::Scalar::new(ApiDoc::openapi()).to_html())
}
