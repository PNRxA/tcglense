//! Email verification — the no-provider dev bypass, the verify-first login gate,
//! the emailed single-use token, and the anti-enumeration posture of
//! resend-verification.
//!
//! Since registration went email-first (issue #176), the unverified-**with-a-
//! password** state — the only one that can still reach login's 403 gate — is no
//! longer reachable through the HTTP surface: a fresh registration is
//! password-less until completion, and completing (or resetting) the password
//! verifies the address. That state now belongs only to accounts predating the
//! feature, so these tests plant one by a direct entity insert (the same
//! bypass-the-handler idiom `registration.rs` uses for the case-insensitivity
//! fixture).

use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, EntityTrait, Set, sea_query::Expr};

use super::harness::*;
use crate::auth::password::hash_password;
use crate::entities::{email_token, prelude::EmailToken, user};

/// Plant a grandfathered (pre-#176) account directly: it HAS a password but was
/// never verified — the only state that can still reach login's 403 verification
/// gate, since an email-first registration is password-less until completion
/// (which verifies it). Bypasses the handler exactly like the case-insensitivity
/// fixture in `registration.rs`.
async fn insert_grandfathered_unverified(app: &TestApp, email: &str) {
    let now = Utc::now();
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(Some(hash_password("password123").expect("hash"))),
        created_at: Set(now),
        updated_at: Set(now),
        email_verified_at: Set(None),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("insert grandfathered unverified account");
}

#[tokio::test]
async fn with_no_email_provider_registration_bypass_completes_and_signs_in() {
    // Dev posture: no email provider, so the completion link can't be delivered.
    // Register hands the completion token straight back in the response instead
    // (the ONLY mode in which it does), so the SPA can drive to the set-password
    // step the undeliverable email would have linked to. Completing then signs
    // the account in, and login works with no separate verification step.
    let app = test_app_email_disabled().await;

    let (status, headers, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "dev@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Register itself mints no session — only the dev-bypass completion token.
    let completion = body["completion_token"]
        .as_str()
        .expect("dev-bypass completion token")
        .to_string();
    assert!(!completion.is_empty());
    assert!(refresh_token_from(&headers).is_none(), "register mints no session");

    // Nothing beyond that deliberate dev-only token leaks: no hash, no field name.
    let raw = body.to_string();
    assert!(!raw.contains("password_hash"), "leaked field name: {raw}");
    assert!(!raw.contains("$argon2"), "leaked a hash: {raw}");

    // Spend the token to choose a password; that signs the account in.
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": completion, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let access = body["access_token"].as_str().expect("session on completion");
    assert!(refresh_token_from(&headers).is_some(), "refresh cookie set");

    // The returned access token works on a protected route straight away.
    let (me, _, me_body) = send(&app, get_with_bearer("/api/auth/me", access)).await;
    assert_eq!(me, StatusCode::OK);
    assert_eq!(me_body["user"]["email"], "dev@example.com");

    // And a fresh login works with no verification step (email disabled).
    let (login, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "dev@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(login, StatusCode::OK);

    // Registering the now-completed address again hands back no completion token:
    // it already has a password, so there's nothing to complete (still 200).
    let (status, _, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "dev@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["completion_token"].is_null(),
        "a completed account has no completion token to hand back"
    );
}

#[tokio::test]
async fn login_is_refused_until_the_email_is_verified() {
    let app = test_app().await;
    insert_grandfathered_unverified(&app, "gate@example.com").await;

    // Correct password, unverified account -> a distinct 403, and still no session.
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "gate@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "email not verified");
    assert!(refresh_token_from(&headers).is_none());

    // WRONG password on an unverified account stays the generic 401: the 403 is
    // only ever revealed to a caller holding the correct password, so it can't
    // become an account-existence oracle.
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "gate@example.com", "password": "wrong-password" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "invalid email or password");

    // Ask for a verification link (a grandfathered account has a password, so
    // resend-verification will issue one), consume it, and login then succeeds.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "gate@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let token = latest_email_token(&app, "gate@example.com").await;
    let (status, _, _) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": token })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    login(&app, "gate@example.com", "password123").await;
}

#[tokio::test]
async fn verification_tokens_are_single_use_and_garbage_is_rejected() {
    let app = test_app().await;
    insert_grandfathered_unverified(&app, "once@example.com").await;

    // Obtain a live verification token via resend-verification.
    send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "once@example.com" }),
        ),
    )
    .await;
    let token = latest_email_token(&app, "once@example.com").await;

    let (first, _, _) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": token })),
    )
    .await;
    assert_eq!(first, StatusCode::NO_CONTENT);

    // Replay of the spent token is rejected, as is arbitrary garbage.
    let (second, _, body) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": token })),
    )
    .await;
    assert_eq!(second, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "invalid or expired token");

    let (garbage, _, _) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": "deadbeef" })),
    )
    .await;
    assert_eq!(garbage, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resend_verification_is_generic_and_respects_the_cooldown() {
    let app = test_app().await;
    insert_grandfathered_unverified(&app, "resend@example.com").await;

    // A known-unverified and an unknown address answer identically — no oracle.
    let (known, _, known_body) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "resend@example.com" }),
        ),
    )
    .await;
    let (unknown, _, unknown_body) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "ghost@example.com" }),
        ),
    )
    .await;
    assert_eq!(known, StatusCode::NO_CONTENT);
    assert_eq!(unknown, StatusCode::NO_CONTENT);
    assert_eq!(known_body, unknown_body);

    // The known address got exactly one link; the unknown got nothing.
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails.iter().filter(|e| e.to == "resend@example.com").count(),
        1
    );
    assert!(emails.iter().all(|e| e.to != "ghost@example.com"));

    // An immediate second resend is inside the 60s issue cooldown -> nothing new.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "resend@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails.iter().filter(|e| e.to == "resend@example.com").count(),
        1,
        "the cooldown suppresses a back-to-back resend"
    );

    // Age the outstanding token past the cooldown window; resend then delivers a
    // fresh link.
    EmailToken::update_many()
        .col_expr(
            email_token::Column::CreatedAt,
            Expr::value(Utc::now() - Duration::minutes(5)),
        )
        .exec(&app.state.db)
        .await
        .expect("age the outstanding token");
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "resend@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails.iter().filter(|e| e.to == "resend@example.com").count(),
        2
    );

    // The re-sent link works.
    let token = latest_email_token(&app, "resend@example.com").await;
    let (status, _, _) = send(
        &app,
        json_post("/api/auth/verify-email", json!({ "token": token })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // An already-verified account is never mailed again (same generic answer).
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "resend@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails.iter().filter(|e| e.to == "resend@example.com").count(),
        2
    );

    // A pending (password-less) registration gets NO verification mail from
    // resend-verification: that endpoint only mails accounts that already have a
    // password, so a pending sign-up's link is re-sent by POSTing /register.
    let (status, _, _) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "pending@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/resend-verification",
            json!({ "email": "pending@example.com" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let emails = delivered_emails(&app).await;
    let to_pending: Vec<_> = emails
        .iter()
        .filter(|e| e.to == "pending@example.com")
        .collect();
    assert_eq!(
        to_pending.len(),
        1,
        "resend-verification sends a pending account nothing extra"
    );
    assert!(
        to_pending[0].subject.contains("Finish creating"),
        "the one mail was the registration completion link, not a verification: {}",
        to_pending[0].subject
    );
}
