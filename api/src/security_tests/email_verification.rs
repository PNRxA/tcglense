//! Email verification — the verify-first login gate, the emailed single-use
//! token, and the anti-enumeration posture of resend-verification.

use chrono::{Duration, Utc};
use sea_orm::{EntityTrait, sea_query::Expr};

use super::harness::*;
use crate::entities::{email_token, prelude::EmailToken};

#[tokio::test]
async fn login_is_refused_until_the_email_is_verified() {
    let app = test_app().await;
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "gate@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

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

    // Consume the emailed token; login then succeeds.
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
    send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "once@example.com", "password": "password123" }),
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
    send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "resend@example.com", "password": "password123" }),
        ),
    )
    .await;

    // A known and an unknown address answer identically — no existence oracle.
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

    // Registration just issued a token, so that resend was inside the cooldown:
    // exactly one email (the registration one) went out, and none to the ghost.
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails.iter().filter(|e| e.to == "resend@example.com").count(),
        1
    );
    assert!(emails.iter().all(|e| e.to != "ghost@example.com"));

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
}
