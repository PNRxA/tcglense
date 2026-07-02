//! CAPTCHA gate on the auth endpoints: an enabled verifier rejects a request
//! with a missing/invalid token (uniformly, before any account work) and lets a
//! valid one through.

use super::harness::*;

#[tokio::test]
async fn register_requires_a_valid_captcha_when_enabled() {
    let app = test_app_requiring_captcha().await;

    // No token -> 400, before any account is created.
    let (missing, _, body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cap@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().is_some());

    // Wrong token -> 400.
    let (wrong, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cap@example.com", "password": "password123", "captcha_token": "nope" }),
        ),
    )
    .await;
    assert_eq!(wrong, StatusCode::BAD_REQUEST);

    // Valid token -> the account is created (no session, per the verify-first flow).
    let (ok, _, ok_body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cap@example.com", "password": "password123", "captcha_token": "good-token" }),
        ),
    )
    .await;
    assert_eq!(ok, StatusCode::CREATED, "valid captcha registers: {ok_body:?}");
    assert_eq!(ok_body["user"]["email"], "cap@example.com");
}

#[tokio::test]
async fn login_requires_a_valid_captcha_when_enabled() {
    let app = test_app_requiring_captcha().await;

    // The captcha check precedes credential handling, so even a would-be valid
    // login is rejected with 400 (not 401) when the token is missing.
    let (missing, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "who@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn email_endpoints_require_captcha_before_any_account_lookup() {
    let app = test_app_requiring_captcha().await;

    // forgot-password / resend-verification normally answer a generic 204; with
    // captcha enabled a tokenless call is a uniform 400 regardless of whether the
    // address exists, so it stays non-enumerable.
    for path in ["/api/auth/forgot-password", "/api/auth/resend-verification"] {
        let (status, _, _) = send(
            &app,
            json_post(path, json!({ "email": "nobody@example.com" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path} without a captcha token");
    }
}
