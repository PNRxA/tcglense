//! CORS contract (plus the public health probe).

use super::harness::*;

#[tokio::test]
async fn cors_preflight_allows_dev_origin_with_credentials() {
    let app = test_app().await;
    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("OPTIONS")
            .uri("/api/auth/login")
            .header(ORIGIN, "http://localhost:5173")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert!(status.is_success(), "preflight status was {status}");
    let allow_origin = headers
        .get(ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok());
    // Echoes the explicit origin (never the wildcard, which is illegal with creds).
    assert_eq!(allow_origin, Some("http://localhost:5173"));
    assert_ne!(allow_origin, Some("*"));
    assert_eq!(
        headers
            .get(ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
}

#[tokio::test]
async fn cors_does_not_authorize_foreign_or_near_miss_origins() {
    let app = test_app().await;
    // A near-miss (right host, wrong port) and a look-alike host prove the
    // allow-list is an exact match, not a prefix/substring one.
    for origin in [
        "https://evil.example.com",
        "http://localhost:5174",
        "http://localhost.evil.com",
    ] {
        let (_, headers, _) = send(
            &app,
            Request::builder()
                .method("GET")
                .uri("/api/health")
                .header(ORIGIN, origin)
                .body(Body::empty())
                .unwrap(),
        )
        .await;

        // The allow-list is a single pinned origin, so `Access-Control-Allow-Origin`
        // is ALWAYS that exact value — never the requesting (foreign) origin and
        // never `*`. The browser compares its origin to this header and blocks the
        // cross-origin response on the mismatch. Asserting it stays pinned also
        // catches a regression to `mirror_request` or a widened allow-list (either
        // would echo the foreign origin here).
        let allow_origin = headers
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok());
        assert_eq!(
            allow_origin,
            Some("http://localhost:5173"),
            "ACAO must stay pinned to the one allowed origin for {origin}"
        );
        assert_ne!(
            allow_origin,
            Some(origin),
            "must never echo the foreign origin"
        );
        assert_ne!(allow_origin, Some("*"));
    }
}

#[tokio::test]
async fn health_is_public() {
    let app = test_app().await;
    let (status, _, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}
