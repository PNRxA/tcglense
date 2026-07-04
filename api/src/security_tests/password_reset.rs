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

/// A password reset ends **every** existing session, not just one. `reset_password`
/// calls `revoke_all_for_user` (a whole-family revocation keyed by user id) — this
/// is the property that evicts an attacker's stolen session when a victim resets
/// their password. A regression narrowing it to a single-token revoke would leave
/// other live sessions authenticated, so pin it with several concurrent sessions.
#[tokio::test]
async fn reset_revokes_every_active_session() {
    let app = test_app().await;
    // Three concurrent sessions for one account: one from registration, two logins,
    // each with its own distinct refresh cookie.
    let (_, session1) = register(&app, "multi@example.com", "password123").await;
    let (_, session2) = login(&app, "multi@example.com", "password123").await;
    let (_, session3) = login(&app, "multi@example.com", "password123").await;
    assert_ne!(session1, session2);
    assert_ne!(session2, session3);

    // Reset the password.
    send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "multi@example.com" }),
        ),
    )
    .await;
    let token = latest_email_token(&app, "multi@example.com").await;
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "new-password-456" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "reset failed: {body:?}");

    // EVERY pre-reset session is dead — not just the most recent one. Each refresh
    // cookie is now rejected at rotation.
    for (label, refresh) in [
        ("registration session", &session1),
        ("second session", &session2),
        ("third session", &session3),
    ] {
        let (rotate_status, _, _) =
            send(&app, post_with_cookie("/api/auth/refresh", refresh)).await;
        assert_eq!(
            rotate_status,
            StatusCode::UNAUTHORIZED,
            "{label} must be revoked by the reset"
        );
    }
}

/// A registration-completion link left outstanding when the account is later
/// activated by a *different* path (forgot-password + reset) can no longer set a
/// password: `complete_registration` refuses a token once the account has a password
/// (generic 401), so an intercepted/replayed completion link can't take over an
/// already-secured account.
#[tokio::test]
async fn a_completion_token_is_refused_after_the_account_is_activated_via_reset() {
    let app = test_app().await;
    // Start a pending email-first registration and capture the completion link, but
    // do NOT complete it.
    send(
        &app,
        json_post("/api/auth/register", json!({ "email": "revive@example.com" })),
    )
    .await;
    let completion = token_for_subject(&app, "revive@example.com", "Finish creating").await;

    // Activate + secure the account by a different path: forgot-password + reset sets
    // the first password and verifies the (previously pending) account.
    send(
        &app,
        json_post(
            "/api/auth/forgot-password",
            json!({ "email": "revive@example.com" }),
        ),
    )
    .await;
    let reset = token_for_subject(&app, "revive@example.com", "Reset your").await;
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": reset, "password": "reset-password-1" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The still-outstanding completion link can no longer set a password: the account
    // now HAS one, so the password-exists gate refuses it with the same generic 401 as
    // any dead token — a completion link never doubles as a password reset.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": completion, "password": "attacker-password-9" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The account still holds the RESET password, untouched by the refused completion:
    // the reset password logs in, the attacker's would-be password does not.
    login(&app, "revive@example.com", "reset-password-1").await;
    let (attacker, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "revive@example.com", "password": "attacker-password-9" }),
        ),
    )
    .await;
    assert_eq!(
        attacker,
        StatusCode::UNAUTHORIZED,
        "the refused completion must not have set a password"
    );
}
