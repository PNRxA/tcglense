//! Password reset — the emailed single-use token, generic (anti-enumeration)
//! responses, and the revoke-everything semantics of a completed reset. Also
//! pins the cross-purpose isolation between reset tokens and the email-first
//! registration-completion tokens (neither can be spent as the other).

use chrono::Utc;
use sea_orm::{ActiveModelTrait, Set};

use super::harness::*;
use crate::auth::password::hash_password;
use crate::entities::user;

/// The token from the most recent email to `to` whose subject contains `needle`.
/// A test that triggers both a registration and a reset mails two `token=` links
/// to the same address, and the spawned sends can land in either order, so it
/// must disambiguate by subject rather than trust "the newest".
async fn token_for_subject(app: &TestApp, to: &str, needle: &str) -> String {
    let emails = delivered_emails(app).await;
    let email = emails
        .iter()
        .rev()
        .find(|e| e.to == to && e.subject.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} email delivered to {to}"));
    let after = email
        .text
        .split_once("token=")
        .expect("email text carries a token link")
        .1;
    let token: String = after.chars().take_while(char::is_ascii_hexdigit).collect();
    assert!(!token.is_empty(), "token link is empty: {}", email.text);
    token
}

#[tokio::test]
async fn forgot_password_is_generic_and_reset_rotates_the_password() {
    let app = test_app().await;
    let (_, old_refresh) = register(&app, "reset@example.com", "password123").await;

    // A known and an unknown address answer identically — no existence oracle.
    let (known, _, known_body) = send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "reset@example.com" }),
        ),
    )
    .await;
    let (unknown, _, unknown_body) = send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "ghost@example.com" }),
        ),
    )
    .await;
    assert_eq!(known, StatusCode::NO_CONTENT);
    assert_eq!(unknown, StatusCode::NO_CONTENT);
    assert_eq!(known_body, unknown_body);
    assert!(
        delivered_emails(&app)
            .await
            .iter()
            .all(|e| e.to != "ghost@example.com")
    );

    // Spend the emailed token on a new password.
    let token = latest_email_token(&app, "reset@example.com").await;
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "reset failed: {body:?}");

    // The old password is dead, the new one works.
    let (old_pw, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "reset@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(old_pw, StatusCode::UNAUTHORIZED);
    login(&app, "reset@example.com", "new-password-456").await;

    // Every pre-reset session was revoked: the old refresh cookie won't rotate.
    let (refresh_status, _, _) =
        send(&app, post_with_cookie("/api/auth/refresh", &old_refresh)).await;
    assert_eq!(refresh_status, StatusCode::UNAUTHORIZED);

    // The reset token is single-use.
    let (replay, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "another-pass-789" }),
        ),
    )
    .await;
    assert_eq!(replay, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn reset_password_enforces_password_rules_before_spending_the_token() {
    let app = test_app().await;
    register(&app, "rules@example.com", "password123").await;
    send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "rules@example.com" }),
        ),
    )
    .await;
    let token = latest_email_token(&app, "rules@example.com").await;

    // A too-weak replacement is a 422 — and must not burn the token.
    let (weak, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "short" }),
        ),
    )
    .await;
    assert_eq!(weak, StatusCode::UNPROCESSABLE_ENTITY);

    let (ok, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "long-enough-pass" }),
        ),
    )
    .await;
    assert_eq!(ok, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_registration_completion_token_cannot_reset_a_password() {
    let app = test_app().await;
    // Start (but don't complete) an email-first registration: the emailed token
    // is a registration-COMPLETION token (purpose complete_registration).
    send(
        &app,
        json_post("/api/auth/register", json!({ "email": "cross@example.com" })),
    )
    .await;
    let completion = latest_email_token(&app, "cross@example.com").await;

    // Presenting it to the reset endpoint must fail: the purpose is enforced in
    // the consuming UPDATE, so a completion link can't double as a password reset.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": completion, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // And the cross-purpose rejection didn't burn it: the same token still
    // completes the registration for its own purpose.
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": completion, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "completion failed: {body:?}");
    assert!(body["access_token"].as_str().is_some());
    assert!(refresh_token_from(&headers).is_some());
}

#[tokio::test]
async fn a_reset_token_cannot_complete_a_registration() {
    let app = test_app().await;
    // A pending (password-less) registration...
    send(
        &app,
        json_post("/api/auth/register", json!({ "email": "swap@example.com" })),
    )
    .await;

    // ...that then asks for a password reset (forgot-password activates a pending
    // account too). Both mails carry a `token=` link, so grab the RESET one by
    // subject rather than trust ordering.
    send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "swap@example.com" }),
        ),
    )
    .await;
    let reset = token_for_subject(&app, "swap@example.com", "Reset your").await;

    // The reset token cannot be spent at complete-registration (purpose mismatch,
    // same generic 401).
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": reset, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // But it still works at reset-password, which sets the password AND verifies
    // the (previously pending) account — login with the new password succeeds.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": reset, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    login(&app, "swap@example.com", "new-password-456").await;
}

#[tokio::test]
async fn completing_a_reset_verifies_an_unverified_account() {
    let app = test_app().await;
    // A grandfathered (pre-#176) account: it has a password but was never
    // verified — the only kind for which "unverified" is still a reachable state
    // (email-first sign-ups verify the moment they complete). Planted directly,
    // bypassing the handler, like the case-insensitivity fixture in
    // `registration.rs`.
    let now = Utc::now();
    user::ActiveModel {
        email: Set("lost@example.com".to_string()),
        password_hash: Set(Some(hash_password("password123").expect("hash"))),
        display_name: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        email_verified_at: Set(None),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("insert grandfathered unverified account");

    // The reset flow still works for the unverified account...
    send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "lost@example.com" }),
        ),
    )
    .await;
    let token = latest_email_token(&app, "lost@example.com").await;
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // ...and completing it proved mailbox ownership, so login now succeeds
    // without a separate verification step (no 403).
    login(&app, "lost@example.com", "new-password-456").await;
}
