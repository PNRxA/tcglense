//! The new-signup switch (`SIGNUPS_ENABLED=false`).
//!
//! An operator can temporarily stop new registrations while existing users keep
//! signing in. When disabled, both `POST /register` and `POST
//! /complete-registration` are refused with a `403` carrying the configured (or a
//! generic) notice, and `GET /api/config` advertises the state so the SPA can show
//! the message and disable its signup form. Crucially, `login` is unaffected.

use super::harness::*;

/// Seed a ready-to-use account directly in the DB (real Argon2 hash + verified),
/// bypassing the register flow — which is exactly what a signups-disabled app
/// refuses. Lets the login-still-works test start from an existing user.
async fn seed_verified_account(app: &TestApp, email: &str, password: &str) {
    use crate::auth::password::hash_password;
    use crate::entities::user;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};

    let now = Utc::now();
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(Some(hash_password(password).expect("hash password"))),
        email_verified_at: Set(Some(now)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("seed account");
}

#[tokio::test]
async fn disabled_signups_reject_registration_with_the_configured_message() {
    let message = "Signups are paused while we scale up — check back soon!";
    let app = test_app_signups_disabled(Some(message)).await;

    // Starting a new registration is refused with the operator's message. 403 (a
    // policy refusal), not 401 — the SPA must not treat it as an auth-refresh cue.
    let (status, _, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "new@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], message);

    // The completion step is gated too, so a link minted before signups were
    // turned off can't finalise a brand-new account either. (A garbage token still
    // gets the signups-disabled 403, proving the guard runs before any token work.)
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": "whatever", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], message);

    // No account was created for the refused address.
    let emails = delivered_emails(&app).await;
    assert!(emails.is_empty(), "a refused signup must send no mail");
}

#[tokio::test]
async fn disabled_signups_without_a_message_use_a_generic_notice() {
    let app = test_app_signups_disabled(None).await;

    let (status, _, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "new@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let error = body["error"].as_str().expect("error string");
    assert!(
        error.contains("temporarily disabled"),
        "generic fallback notice expected, got: {error}"
    );
}

#[tokio::test]
async fn public_config_advertises_the_disabled_signup_state_and_message() {
    let message = "We're not accepting new members right now.";
    let app = test_app_signups_disabled(Some(message)).await;

    let (status, _, body) = send(&app, get("/api/config")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signups_enabled"], false);
    assert_eq!(body["signups_disabled_message"], message);
}

#[tokio::test]
async fn public_config_reports_enabled_signups_from_the_test_config() {
    // The shipped default posture: signups open, no disabled-message to render.
    let app = test_app().await;

    let (status, _, body) = send(&app, get("/api/config")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signups_enabled"], true);
    assert!(
        body["signups_disabled_message"].is_null(),
        "no notice while signups are open: {body}"
    );
}

/// Seed a pending (password-less, unverified) account — the row `register`
/// creates before completion — directly in the DB, and return its id. Lets a test
/// simulate a stale pending registration that predates the signups pause.
async fn seed_pending_account(app: &TestApp, email: &str) -> i32 {
    use crate::entities::user;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};

    let now = Utc::now();
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("seed pending account")
    .id
}

#[tokio::test]
async fn disabled_signups_refuse_activating_a_pending_account_via_password_reset() {
    use crate::auth::email_token::{EmailTokenPurpose, issue};
    use crate::entities::prelude::User;
    use sea_orm::EntityTrait;

    let app = test_app_signups_disabled(Some("closed")).await;
    // A stale pending row that predates the pause (register can no longer mint one).
    let user_id = seed_pending_account(&app, "stale@example.com").await;
    // Its owner requests a reset (forgot-password stays generic) and gets a token…
    let token = issue(&app.state.db, user_id, EmailTokenPurpose::ResetPassword)
        .await
        .expect("issue reset token");

    // …but the reset must NOT finalise the account into a usable login while
    // signups are disabled — that is the same new-account creation register refuses.
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "closed");

    // The account is still pending: no password was set, so it can't sign in.
    let row = User::find_by_id(user_id)
        .one(&app.state.db)
        .await
        .expect("query")
        .expect("row");
    assert!(
        row.password_hash.is_none(),
        "a disabled-signups reset must not activate the pending account"
    );
}

#[tokio::test]
async fn existing_users_can_still_reset_their_password_when_signups_are_disabled() {
    use crate::auth::email_token::{EmailTokenPurpose, issue};
    use crate::entities::{prelude::User, user};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let app = test_app_signups_disabled(Some("closed")).await;
    seed_verified_account(&app, "member@example.com", "old-password").await;
    let user = User::find()
        .filter(user::Column::Email.eq("member@example.com"))
        .one(&app.state.db)
        .await
        .expect("query")
        .expect("row");
    let token = issue(&app.state.db, user.id, EmailTokenPurpose::ResetPassword)
        .await
        .expect("issue reset token");

    // A genuine reset for an already-activated account is untouched by the pause —
    // existing users must keep recovering their access.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/reset-password",
            json!({ "token": token, "password": "brand-new-password" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The new password works; the old one no longer does.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "member@example.com", "password": "brand-new-password" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn existing_users_can_still_log_in_when_signups_are_disabled() {
    let app = test_app_signups_disabled(Some("closed")).await;
    seed_verified_account(&app, "member@example.com", "password123").await;

    // Registration is closed, but the existing account authenticates normally and
    // gets a session (login shares nothing with the signup switch).
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "member@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "login must work when signups are off: {body:?}"
    );
    assert!(body["access_token"].as_str().is_some());
    assert!(refresh_token_from(&headers).is_some());
}
