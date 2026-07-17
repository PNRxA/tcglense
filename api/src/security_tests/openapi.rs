//! HTTP-level tests for the served OpenAPI spec (issue #284).
//!
//! The `openapi::tests::openapi_spec_builds` unit test proves `ApiDoc::openapi()`
//! doesn't panic; this proves the public `/api/openapi.json` route is actually wired,
//! serves, and is CDN-cacheable (in the public group, not swallowed by any fallback).
//! The interactive reference is rendered by the SPA at `/docs`, not the API.

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
        body["openapi"]
            .as_str()
            .unwrap_or_default()
            .starts_with("3."),
        "carries an openapi version: {body:?}"
    );
    assert!(body["paths"].is_object(), "documents some paths");
    assert!(
        body["components"]["securitySchemes"]["api_key"].is_object(),
        "registers the api_key security scheme"
    );

    // Public, CDN-cacheable (in the public router group).
    let cc = cache_control(&headers).unwrap_or_default();
    assert!(
        cc.contains("public"),
        "spec should be shared-cacheable: {cc:?}"
    );
}
