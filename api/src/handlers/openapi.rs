//! Serve the OpenAPI document (issue #284).
//!
//! `GET /api/openapi.json` returns the raw OpenAPI 3.1 JSON, a public, CDN-cacheable GET
//! wired into the `public` group in [`crate::router`]. The spec is materialized from
//! [`crate::openapi::ApiDoc`]. The interactive reference is rendered by the SPA at the
//! `/docs` route (`web/src/views/DocsView.vue`, `@scalar/api-reference`), which embeds
//! this same document.

use axum::{Json, response::IntoResponse};
use utoipa::OpenApi;

use crate::openapi::ApiDoc;

/// `GET /api/openapi.json` -> the machine-readable OpenAPI 3.1 description of the API.
pub async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}
