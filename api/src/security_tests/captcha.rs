//! CAPTCHA gate on the auth endpoints: an enabled verifier rejects a request
//! with a missing/invalid token (uniformly, before any account work) and lets a
//! valid one through.

use super::harness::*;

#[tokio::test]
async fn register_requires_a_valid_captcha_when_enabled() {
    let app = test_app_requiring_captcha().await;

    // No token -> 400, before any account is touched.
    let (missing, _, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "cap@example.com" })),
    )
    .await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().is_some());

    // Wrong token -> 400.
    let (wrong, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cap@example.com", "captcha_token": "nope" }),
        ),
    )
    .await;
    assert_eq!(wrong, StatusCode::BAD_REQUEST);

    // The captcha gate fires BEFORE any account work: neither rejected attempt
    // above may have mailed a completion link or created a user row (the gate is
    // the first thing the handler does, so a failed captcha leaks nothing and
    // leaves no side effect).
    {
        use crate::entities::{prelude::User, user};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        assert!(
            delivered_emails(&app).await.is_empty(),
            "a rejected captcha must not send any email"
        );
        let planted = User::find()
            .filter(user::Column::Email.eq("cap@example.com"))
            .one(&app.state.db)
            .await
            .expect("query");
        assert!(
            planted.is_none(),
            "a rejected captcha must not create an account"
        );
    }

    // Valid token -> the generic email-first 200 with no account echo (the
    // completion link rides only in the email, never the response body).
    let (ok, _, ok_body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cap@example.com", "captcha_token": "good-token" }),
        ),
    )
    .await;
    assert_eq!(ok, StatusCode::OK, "valid captcha registers: {ok_body:?}");
    assert!(ok_body["completion_token"].is_null());
    assert!(ok_body.get("user").is_none(), "no user echo: {ok_body}");
}

#[tokio::test]
async fn complete_registration_requires_a_valid_captcha_when_enabled() {
    let app = test_app_requiring_captcha().await;

    // The captcha check precedes ALL of the completion work — password
    // validation, consuming the single-use token, setting the credential — so a
    // missing/wrong token is a uniform 400 regardless of whether the completion
    // token would have been valid (here it's garbage; the captcha gate fires
    // first, so the token is never even looked at).
    let (missing, _, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": "deadbeef", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().is_some());

    let (wrong, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": "deadbeef", "password": "password123", "captcha_token": "nope" }),
        ),
    )
    .await;
    assert_eq!(wrong, StatusCode::BAD_REQUEST);
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

/// The remaining two of the seven auth mutation endpoints — reset-password and
/// verify-email — are CAPTCHA-gated too. Both carry a `token` (not an email), so the
/// body must be well-formed for the extractor before the handler's captcha check
/// runs; a well-formed body missing the `captcha_token` is a uniform 400, before the
/// token is consumed or any account work happens. reset-password is the most
/// sensitive of the set (it re-hashes the password and revokes every refresh token),
/// so a dropped captcha check there would silently remove the abuse gate.
#[tokio::test]
async fn reset_and_verify_endpoints_require_captcha() {
    let app = test_app_requiring_captcha().await;

    // reset-password: a well-formed {token, password} body with no captcha_token -> 400
    // (not a 401 for the bogus token — the captcha gate fires first).
    let (reset, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": "deadbeef", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(reset, StatusCode::BAD_REQUEST);

    // verify-email: a well-formed {token} body with no captcha_token -> 400.
    let (verify, _, _) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": "deadbeef" })),
    )
    .await;
    assert_eq!(verify, StatusCode::BAD_REQUEST);
}
