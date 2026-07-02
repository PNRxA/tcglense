//! Per-IP auth rate limiting: the 429 after a burst, per-IP isolation, that a
//! spoofed `X-Forwarded-For` is ignored unless the proxy is trusted, and that the
//! limiter is inert for the rest of the suite (no resolvable client IP).

use super::harness::*;

#[tokio::test]
async fn register_is_rate_limited_per_ip_when_behind_a_trusted_proxy() {
    let app = test_app_trusting_proxy().await;
    let ip = "198.51.100.7";

    // The register quota allows a burst of 5 from one IP...
    for i in 0..5 {
        let (status, _, body) = send(
            &app,
            json_post_from(
                "/api/auth/register",
                ip,
                json!({ "email": format!("rl{i}@example.com"), "password": "password123" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "request {i} within burst: {body:?}");
    }

    // ...the 6th is throttled with a 429 carrying Retry-After.
    let (status, headers, body) = send(
        &app,
        json_post_from(
            "/api/auth/register",
            ip,
            json!({ "email": "rl-over@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.get("retry-after").is_some(), "429 carries Retry-After");
    assert!(body["error"].as_str().is_some());
    // Rate-limit responses must never be shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));

    // A different IP has its own budget and is unaffected.
    let (status, _, _) = send(
        &app,
        json_post_from(
            "/api/auth/register",
            "198.51.100.8",
            json!({ "email": "rl-other@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn login_and_register_limits_are_independent() {
    let app = test_app_trusting_proxy().await;
    let ip = "198.51.100.20";

    // Exhaust the register burst for this IP.
    for i in 0..5 {
        let _ = send(
            &app,
            json_post_from(
                "/api/auth/register",
                ip,
                json!({ "email": format!("ind{i}@example.com"), "password": "password123" }),
            ),
        )
        .await;
    }
    let (over, _, _) = send(
        &app,
        json_post_from(
            "/api/auth/register",
            ip,
            json!({ "email": "ind-over@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(over, StatusCode::TOO_MANY_REQUESTS);

    // Login from the SAME IP has a separate budget — a wrong-credentials 401,
    // not a 429.
    let (login, _, _) = send(
        &app,
        json_post_from(
            "/api/auth/login",
            ip,
            json!({ "email": "ind0@example.com", "password": "wrong" }),
        ),
    )
    .await;
    assert_eq!(login, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_spoofed_forwarded_for_is_ignored_without_a_trusted_proxy() {
    // The default app does NOT trust proxy headers and has no socket peer, so the
    // client IP is unresolvable and the limiter fails open — a spoofed XFF can't
    // trip (or evade) limits. This also proves the limiter stays inert for the
    // rest of the in-process suite.
    let app = test_app().await;
    for i in 0..8 {
        let (status, _, _) = send(
            &app,
            json_post_from(
                "/api/auth/register",
                "203.0.113.9",
                json!({ "email": format!("spoof{i}@example.com"), "password": "password123" }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "request {i} must not be limited when the proxy header is untrusted"
        );
    }
}
