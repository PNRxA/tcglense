//! Serve the OpenAPI document and the interactive Scalar UI (issue #284).
//!
//! Both are public, CDN-cacheable GET routes wired into the `public` group in
//! [`crate::router`]: `GET /api/openapi.json` returns the raw OpenAPI 3.1 JSON, and
//! `GET /api/docs` returns a self-contained Scalar "try it out" page that embeds that
//! same spec. The spec is materialized from [`crate::openapi::ApiDoc`].

use axum::{Json, response::Html, response::IntoResponse};
use utoipa::OpenApi;

use crate::openapi::ApiDoc;

/// Custom Scalar HTML template. Mirrors utoipa-scalar's default page but carries the
/// same Scalar configuration we use on OpenPosterDB: no HTTP-client / MCP / agent
/// buttons, no dev tools, forced light mode with the dark-mode toggle hidden, and all
/// operation tags expanded by default. The `$spec` placeholder is replaced by
/// [`utoipa_scalar::Scalar::to_html`] with the embedded OpenAPI JSON.
const SCALAR_HTML: &str = r#"<!doctype html>
<html>
<head>
    <title>TCGLense API Reference</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
<script
    id="api-reference"
    type="application/json"
    data-configuration='{"hideClientButton":true,"showDeveloperTools":"never","mcp":{"disabled":true},"agent":{"disabled":true},"forceDarkModeState":"light","hideDarkModeToggle":true,"defaultOpenAllTags":true}'>
    $spec
</script>
<script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
</body>
</html>"#;

/// `GET /api/openapi.json` -> the machine-readable OpenAPI 3.1 description of the API.
pub async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

/// `GET /api/docs` -> the interactive Scalar API reference, with the spec embedded so
/// the page renders without a second round-trip. Scalar's own assets load from its CDN.
pub async fn scalar_ui() -> impl IntoResponse {
    Html(
        utoipa_scalar::Scalar::new(ApiDoc::openapi())
            .custom_html(SCALAR_HTML)
            .to_html(),
    )
}
