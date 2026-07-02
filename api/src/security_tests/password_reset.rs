//! Password reset — the emailed single-use token, generic (anti-enumeration)
//! responses, and the revoke-everything semantics of a completed reset.

use super::harness::*;

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
async fn a_verification_token_cannot_reset_a_password() {
    let app = test_app().await;
    send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "cross@example.com", "password": "password123" }),
        ),
    )
    .await;

    // The registration email carries a VERIFICATION token; presenting it to the
    // reset endpoint must fail (purpose is enforced in the consuming UPDATE).
    let verify_token = latest_email_token(&app, "cross@example.com").await;
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": verify_token, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn completing_a_reset_verifies_an_unverified_account() {
    let app = test_app().await;
    // Register but never verify.
    send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "lost@example.com", "password": "password123" }),
        ),
    )
    .await;

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
    // without a separate verification step.
    login(&app, "lost@example.com", "new-password-456").await;
}
