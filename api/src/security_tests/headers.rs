//! Browser-facing security header contract.

use super::harness::*;

#[tokio::test]
async fn every_response_carries_browser_security_headers() {
    let app = test_app().await;

    for uri in ["/api/health", "/api/not-a-real-route"] {
        let (_, headers, _) = send(&app, get(uri)).await;
        for (name, expected) in [
            ("strict-transport-security", "max-age=31536000"),
            ("referrer-policy", "no-referrer"),
            ("x-content-type-options", "nosniff"),
            ("x-frame-options", "DENY"),
            (
                "content-security-policy",
                "base-uri 'self'; object-src 'none'; frame-ancestors 'none'",
            ),
            (
                "permissions-policy",
                "geolocation=(), microphone=(), camera=(self)",
            ),
        ] {
            assert_eq!(
                headers.get(name).and_then(|value| value.to_str().ok()),
                Some(expected),
                "missing or incorrect {name} on {uri}"
            );
        }
    }
}
