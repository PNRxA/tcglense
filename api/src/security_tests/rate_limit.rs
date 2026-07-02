//! Rate limiting on the wire. Per-IP auth limiting: the 429 after a burst, per-IP
//! isolation, that a spoofed `X-Forwarded-For` is ignored unless the proxy is
//! trusted, and that the limiter is inert for the rest of the suite (no resolvable
//! client IP). Per-user limiting (issue #168): an authenticated user's expensive
//! import class is throttled after its burst — 429 + `Retry-After` + `no-store` —
//! while a second user and the same user's general read budget are unaffected.
//! (Unlike per-IP, per-user keys on the access-token user id, which the in-process
//! harness *does* carry, so it's genuinely exercised here.)

use super::harness::*;

#[tokio::test]
async fn register_is_rate_limited_per_ip_when_behind_a_trusted_proxy() {
    let app = test_app_trusting_proxy().await;
    let ip = "198.51.100.7";

    // The register quota allows a burst of 5 from one IP... (email-first
    // registration answers a generic 200).
    for i in 0..5 {
        let (status, _, body) = send(
            &app,
            json_post_from(
                "/api/auth/register",
                ip,
                json!({ "email": format!("rl{i}@example.com") }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "request {i} within burst: {body:?}");
    }

    // ...the 6th is throttled with a 429 carrying Retry-After.
    let (status, headers, body) = send(
        &app,
        json_post_from(
            "/api/auth/register",
            ip,
            json!({ "email": "rl-over@example.com" }),
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
            json!({ "email": "rl-other@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn complete_registration_is_rate_limited_per_ip_when_behind_a_trusted_proxy() {
    let app = test_app_trusting_proxy().await;
    let ip = "198.51.100.30";

    // Completion rides the looser Token class (20/min, burst 20) — the token
    // itself is garbage, so each attempt is a 401 until the burst is spent.
    for i in 0..20 {
        let (status, _, _) = send(
            &app,
            json_post_from(
                "/api/auth/complete-registration",
                ip,
                json!({ "token": "deadbeef", "password": "password123" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {i} within burst");
    }

    // The 21st from that IP is throttled with a 429 carrying Retry-After — the
    // limiter fires ahead of the token check, so it's a 429, not another 401.
    let (status, headers, body) = send(
        &app,
        json_post_from(
            "/api/auth/complete-registration",
            ip,
            json!({ "token": "deadbeef", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.get("retry-after").is_some(), "429 carries Retry-After");
    assert!(body["error"].as_str().is_some());

    // A different IP has its own Token budget and still reaches the 401.
    let (status, _, _) = send(
        &app,
        json_post_from(
            "/api/auth/complete-registration",
            "198.51.100.31",
            json!({ "token": "deadbeef", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
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
                json!({ "email": format!("ind{i}@example.com") }),
            ),
        )
        .await;
    }
    let (over, _, _) = send(
        &app,
        json_post_from(
            "/api/auth/register",
            ip,
            json!({ "email": "ind-over@example.com" }),
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
                json!({ "email": format!("spoof{i}@example.com") }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "request {i} must not be limited when the proxy header is untrusted"
        );
    }
}

// ---- Per-user rate limiting (the authenticated collection surface, issue #168) ----

/// A `POST .../import/csv` (the tight per-user *import* class) with a bearer token and
/// a tiny valid Archidekt-shaped CSV of one real dummy-catalog card, so each request
/// reconciles a `200` up to the limit.
fn csv_import(token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/collection/mtg/import/csv?mode=overwrite")
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "text/csv")
        .body(Body::from("Scryfall ID,Finish,Quantity\ndummy-dmb-0001,Normal,1\n"))
        .unwrap()
}

#[tokio::test]
async fn authenticated_imports_are_rate_limited_per_user() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "rl-import@example.com", "password123").await;

    // The per-user import class allows a burst of 10 imports from one account...
    for i in 0..10 {
        let (status, _, body) = send(&app, csv_import(&alice)).await;
        assert_eq!(status, StatusCode::OK, "import {i} within the burst: {body:?}");
    }

    // ...the 11th is throttled with a 429 carrying Retry-After, and — being per-user
    // data — the response must never be shared-cached.
    let (status, headers, body) = send(&app, csv_import(&alice)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.get("retry-after").is_some(), "429 carries Retry-After");
    assert!(body["error"].as_str().is_some());
    assert_eq!(cache_control(&headers), Some("no-store"));

    // A different account has an entirely independent import budget.
    let (bob, _) = register(&app, "rl-import-2@example.com", "password123").await;
    let (status, _, body) = send(&app, csv_import(&bob)).await;
    assert_eq!(status, StatusCode::OK, "a second user is unaffected: {body:?}");

    // And the *general* class is a separate budget for Alice: a plain collection read
    // still succeeds even though her import class is exhausted.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &alice)).await;
    assert_eq!(status, StatusCode::OK, "the general class is a separate budget");
}

#[tokio::test]
async fn a_missing_or_invalid_bearer_is_not_per_user_limited() {
    // The per-user limiter keys on a valid access token; a request with no bearer (or
    // a garbage one) has no user to key on, so it passes straight through to the
    // handler's own auth rejection — the limiter never turns a 401 into a 429.
    let app = test_app_with_catalog().await;

    for _ in 0..15 {
        let (status, _, _) = send(&app, get("/api/collection/mtg")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "no bearer -> 401, never 429");

        let (status, _, _) =
            send(&app, get_with_bearer("/api/collection/mtg", "not-a-real-token")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "bad bearer -> 401, never 429");
    }
}
