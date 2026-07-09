//! HTTP-level tests for the served OpenAPI spec + Scalar docs UI (issue #284).
//!
//! The `openapi::tests::openapi_spec_builds` unit test proves `ApiDoc::openapi()`
//! doesn't panic; these prove the two public doc routes are actually wired, serve,
//! and are CDN-cacheable (in the public group, not swallowed by any fallback).

use super::harness::*;

#[tokio::test]
async fn openapi_json_serves_a_cacheable_spec() {
    let app = test_app().await;
    let (status, headers, body) = send(&app, get("/api/openapi.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type(&headers),
        Some("application/json"),
        "the spec is JSON"
    );
    // A well-formed OpenAPI 3.x document with our info + the api-key security scheme.
    assert!(
        body["openapi"].as_str().unwrap_or_default().starts_with("3."),
        "carries an openapi version: {body:?}"
    );
    assert!(body["paths"].is_object(), "documents some paths");
    assert!(
        body["components"]["securitySchemes"]["api_key"].is_object(),
        "registers the api_key security scheme"
    );

    // Public, CDN-cacheable (in the public router group).
    let cc = cache_control(&headers).unwrap_or_default();
    assert!(cc.contains("public"), "spec should be shared-cacheable: {cc:?}");
}

#[tokio::test]
async fn scalar_docs_ui_serves_html() {
    let app = test_app().await;
    let (status, headers, body) = send_text(&app, get("/api/docs")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type(&headers).unwrap_or_default().contains("text/html"),
        "the docs console is HTML"
    );
    // The Scalar viewer HTML references its own script and points at the spec.
    let lower = body.to_lowercase();
    assert!(
        lower.contains("scalar") || lower.contains("openapi"),
        "looks like the Scalar viewer page"
    );
}
