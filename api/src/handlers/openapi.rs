//! Serve the OpenAPI document (issue #284).
//!
//! `GET /api/openapi.json` returns the raw OpenAPI 3.1 JSON, a public, CDN-cacheable GET
//! wired into the `public` group in [`crate::router`]. The spec is materialized from
//! [`crate::openapi::ApiDoc`]. The interactive reference is rendered by the SPA at the
//! `/docs` route (`web/src/views/DocsView.vue`, `@scalar/api-reference`), which embeds
//! this same document.

use std::sync::LazyLock;

use axum::body::Bytes;
use axum::http::header;
use axum::response::IntoResponse;
use utoipa::OpenApi;

use crate::openapi::ApiDoc;

/// The document is compile-time static (it changes only on redeploy), so build and
/// serialize it exactly once; every request thereafter is a cheap refcount clone of
/// the bytes instead of a full utoipa reconstruction + serde serialization (#413).
/// The `expect` is effectively at startup semantics: the first hit materializes it,
/// and the same serialization already backs the OpenAPI unit tests.
static OPENAPI_JSON: LazyLock<Bytes> = LazyLock::new(|| {
    serde_json::to_vec(&ApiDoc::openapi())
        .expect("serialize the static OpenAPI document")
        .into()
});

/// `GET /api/openapi.json` -> the machine-readable OpenAPI 3.1 description of the API.
/// Returns pre-serialized bytes, so the `Content-Type` is set explicitly (that's what
/// `Json` did before).
pub async fn openapi_json() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        OPENAPI_JSON.clone(),
    )
}
