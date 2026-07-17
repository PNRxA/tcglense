//! Registration / response hygiene.
//!
//! Registration is email-first (issue #176): `POST /register` takes the address
//! plus optional same-site navigation context and always answers the same generic
//! 200 — whether the address is new, mid-registration, or already registered —
//! so it can't be used to enumerate accounts (the old duplicate `409` was exactly
//! that oracle). The password (+ display name) is set by
//! `POST /complete-registration`, which consumes the emailed single-use token and
//! signs the account in.

use super::harness::*;

async fn latest_registration_link(app: &TestApp, to: &str) -> url::Url {
    let emails = delivered_emails(app).await;
    let email = emails
        .iter()
        .rev()
        .find(|email| email.to == to)
        .unwrap_or_else(|| panic!("no registration email delivered to {to}"));
    let link = email
        .text
        .lines()
        .find(|line| line.starts_with(&app.state.config.public_site_url))
        .unwrap_or_else(|| {
            panic!(
                "registration email contains no completion link: {}",
                email.text
            )
        });
    url::Url::parse(link).expect("registration email carries a valid absolute URL")
}

#[tokio::test]
async fn safe_registration_redirect_is_encoded_and_preserved_in_the_completion_link() {
    let app = test_app().await;
    let redirect = "/collection/magic?sort=name&dir=asc#owned";
    let (status, _, body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "redirect@example.com", "redirect": redirect }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "completion_token": null }));
    let link = latest_registration_link(&app, "redirect@example.com").await;
    assert_eq!(link.path(), "/complete-registration");
    assert_eq!(
        link.query_pairs()
            .find(|(key, _)| key == "redirect")
            .map(|(_, value)| value.into_owned()),
        Some(redirect.to_string())
    );
    assert!(
        link.as_str()
            .contains("redirect=%2Fcollection%2Fmagic%3Fsort%3Dname%26dir%3Dasc%23owned"),
        "redirect must be encoded as one query value: {link}"
    );
}

#[tokio::test]
async fn unsafe_registration_redirects_are_silently_omitted() {
    let app = test_app().await;
    let cases = [
        ("absolute", "https://evil.example/phish".to_string()),
        ("protocol-relative", "//evil.example/phish".to_string()),
        ("backslash", "/safe\\evil.example".to_string()),
        ("control", "/safe\nheader".to_string()),
        ("oversized", format!("/{}", "a".repeat(4096))),
    ];

    for (label, redirect) in cases {
        let email = format!("unsafe-{label}@example.com");
        let (status, _, body) = send(
            &app,
            json_post(
                "/api/auth/register",
                json!({ "email": email, "redirect": redirect }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{label}: {body:?}");
        assert_eq!(
            body,
            json!({ "completion_token": null }),
            "{label}: invalid navigation context changed the generic response"
        );

        let link = latest_registration_link(&app, &email).await;
        assert!(
            link.query_pairs().all(|(key, _)| key != "redirect"),
            "{label}: unsafe redirect leaked into completion link: {link}"
        );
    }
}

#[tokio::test]
async fn register_answers_generically_and_mints_no_session() {
    let app = test_app().await;
    let (status, headers, body) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "User@Example.COM" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    // No session, and no account echo: the body reveals nothing about whether
    // the address was new or already registered.
    assert!(body["completion_token"].is_null());
    assert!(body.get("user").is_none(), "no user echo: {body}");
    assert!(refresh_token_from(&headers).is_none());

    // The completion token must ride ONLY in the email (the response going
    // back to the unauthenticated caller must not shortcut the mailbox proof).
    // The address is canonicalised (trimmed + lowercased) before mailing.
    let token = latest_email_token(&app, "user@example.com").await;
    let raw = body.to_string();
    assert!(
        !raw.contains(&token),
        "the completion token must not appear in the response body"
    );
    assert!(!raw.contains("password_hash"), "leaked field name: {raw}");
    assert!(!raw.contains("$argon2"), "leaked a hash: {raw}");
}

#[tokio::test]
async fn registering_an_existing_email_is_indistinguishable_and_sends_nothing() {
    use crate::entities::email_token;
    use sea_orm::{EntityTrait, sea_query::Expr};

    let app = test_app().await;
    register(&app, "taken@example.com", "password123").await;

    // Age the account's tokens out of the 60s issue cooldown, so "no mail" below
    // can only mean the activated account was skipped — not that the cooldown
    // happened to swallow the issue.
    email_token::Entity::update_many()
        .col_expr(
            email_token::Column::CreatedAt,
            Expr::value(chrono::Utc::now() - chrono::Duration::hours(2)),
        )
        .exec(&app.state.db)
        .await
        .expect("age tokens");

    let taken = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "Taken@Example.com" }),
        ),
    )
    .await;
    let fresh = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "fresh@example.com" }),
        ),
    )
    .await;

    // Identical status + body for a taken and an unknown address — no 409, no
    // distinguishable field. (Timing is covered by the send running off the
    // request path; see `spawn_send`.)
    assert_eq!(taken.0, StatusCode::OK);
    assert_eq!(fresh.0, StatusCode::OK);
    assert_eq!(taken.2, fresh.2);

    // The activated account got no new mail; the fresh address got its link.
    let emails = delivered_emails(&app).await;
    assert_eq!(
        emails
            .iter()
            .filter(|e| e.to == "taken@example.com")
            .count(),
        1,
        "an already-registered address must not be mailed again by register"
    );
    assert_eq!(
        emails
            .iter()
            .filter(|e| e.to == "fresh@example.com")
            .count(),
        1,
        "a new address gets exactly one completion link"
    );
}

#[tokio::test]
async fn a_pending_registration_gets_the_link_resent_with_cooldown() {
    use crate::entities::email_token;
    use sea_orm::{EntityTrait, sea_query::Expr};

    let app = test_app().await;

    // Start a registration but never complete it.
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "pending@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Registering again inside the cooldown answers the same 200 and sends
    // nothing (the cooldown is unobservable from outside).
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "pending@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let sent = delivered_emails(&app).await;
    assert_eq!(sent.len(), 1, "cooldown suppresses the re-send");

    // Once the cooldown ages out, registering again re-sends a fresh link.
    email_token::Entity::update_many()
        .col_expr(
            email_token::Column::CreatedAt,
            Expr::value(chrono::Utc::now() - chrono::Duration::hours(2)),
        )
        .exec(&app.state.db)
        .await
        .expect("age tokens");
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "pending@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let sent = delivered_emails(&app).await;
    assert_eq!(sent.len(), 2, "an aged pending registration is re-sent");
}

#[tokio::test]
async fn a_pending_registration_cannot_sign_in() {
    let app = test_app().await;
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "limbo@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // No password exists yet, so ANY password fails — with exactly the same
    // generic 401 an unknown address gets (no pending-account oracle either).
    let pending = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "limbo@example.com", "password": "password123" }),
        ),
    )
    .await;
    let unknown = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "ghost@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(pending.0, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.0, StatusCode::UNAUTHORIZED);
    assert_eq!(pending.2, unknown.2);
}

#[tokio::test]
async fn completion_enforces_password_rules_before_spending_the_token() {
    let app = test_app().await;
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "rules@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let token = latest_email_token(&app, "rules@example.com").await;

    // A weak password is refused BEFORE the single-use token is consumed…
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "short" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // …so the same link still completes with a valid one, signing the user in
    // (session + refresh cookie), and the optional username (trimmed) is claimed with an
    // auto-assigned discriminator.
    let (s, headers, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "password123", "username": "  Tester  " }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["access_token"].as_str().is_some());
    assert_eq!(body["user"]["email"], "rules@example.com");
    assert_eq!(body["user"]["username"], "Tester");
    assert!(
        body["user"]["handle"]
            .as_str()
            .is_some_and(|h| h.starts_with("Tester-"))
    );
    assert!(refresh_token_from(&headers).is_some());

    // Single-use: the spent token cannot complete again.
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "password456" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn username_at_signup_is_optional_and_validated() {
    let app = test_app().await;
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "signup-name@example.com" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let token = latest_email_token(&app, "signup-name@example.com").await;

    // An invalid/reserved username is rejected 422 BEFORE the single-use token is consumed,
    // so the user can retry (mirrors the password pre-check).
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "password123", "username": "admin" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // The token survived: completing with a whitespace-only username succeeds and simply
    // leaves the account without a handle (username is opt-in).
    let (s, _, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "password123", "username": "   " }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body["user"]["username"].is_null(),
        "a whitespace-only username must leave the account without a handle: {body}"
    );
    assert!(body["user"]["handle"].is_null());
}

#[tokio::test]
async fn a_completed_account_refuses_further_completion_tokens() {
    use crate::auth::email_token::{EmailTokenPurpose, issue};
    use crate::entities::{prelude::User, user};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let app = test_app().await;
    register(&app, "done@example.com", "password123").await;

    // Plant a second, still-live completion token for the (now completed)
    // account — the shape an attacker would need a completion link to act as a
    // password reset. It must be refused: the account has a password.
    let user = User::find()
        .filter(user::Column::Email.eq("done@example.com"))
        .one(&app.state.db)
        .await
        .expect("query")
        .expect("user exists");
    let planted = issue(
        &app.state.db,
        user.id,
        EmailTokenPurpose::CompleteRegistration,
    )
    .await
    .expect("issue");

    let (s, _, body) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": planted, "password": "hijacked-pass" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "invalid or expired token");

    // The original password still works — nothing was overwritten.
    login(&app, "done@example.com", "password123").await;
}

#[tokio::test]
async fn login_hardens_cookie_and_never_leaks_password_hash() {
    let app = test_app().await;
    register(&app, "hygiene@example.com", "password123").await;

    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "hygiene@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["access_token"].as_str().is_some());

    // The public user shape must never carry secret material.
    let raw = body.to_string();
    assert!(!raw.contains("password_hash"), "leaked field name: {raw}");
    assert!(!raw.contains("$argon2"), "leaked a hash: {raw}");

    // The long-lived refresh token must ride ONLY in Set-Cookie (httpOnly), never
    // echoed into the JSON body where JS could read it.
    let refresh = refresh_token_from(&headers).expect("refresh cookie set");
    assert!(
        !raw.contains(&refresh),
        "the refresh token must not appear in the response body"
    );

    // The refresh cookie rides only in Set-Cookie, hardened.
    let set_cookie = headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("tcglense_refresh="))
        .expect("refresh Set-Cookie");
    let lower = set_cookie.to_ascii_lowercase();
    assert!(lower.contains("httponly"), "{set_cookie}");
    assert!(lower.contains("samesite=lax"), "{set_cookie}");
    assert!(set_cookie.contains("Path=/api/auth"), "{set_cookie}");
}

#[tokio::test]
async fn register_rejects_an_invalid_email() {
    let app = test_app().await;

    let (s1, _, b1) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": "no-at-sign" })),
    )
    .await;
    assert_eq!(s1, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b1["error"].as_str().is_some());
}

#[tokio::test]
async fn case_insensitive_uniqueness_is_enforced_at_the_database() {
    use crate::entities::user;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};

    // Defense-in-depth beyond the handler's lowercasing: insert two rows that
    // differ only in email case *directly* via the entity (bypassing the handler),
    // and require the COLLATE NOCASE unique index to reject the second.
    let state = test_state().await;
    let now = Utc::now();
    let row = |email: &str| user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(Some("irrelevant".to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    row("collate@example.com")
        .insert(&state.db)
        .await
        .expect("first insert succeeds");
    let second = row("Collate@Example.com").insert(&state.db).await;
    assert!(
        second.is_err(),
        "a case-variant email must violate the unique index"
    );
}

#[tokio::test]
async fn oversized_credentials_are_rejected_before_hashing() {
    let app = test_app().await;

    // Register: an email past the 254-char cap -> 422.
    let long_email = format!("{}@example.com", "a".repeat(250));
    assert!(long_email.len() > 254);
    let (s, _, _) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": long_email })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Completion: a password past the 1024-char cap is a cheap-to-send /
    // expensive-to-hash Argon2 DoS -> 422, checked before the token is even
    // looked at (a garbage token still gets the validation error, not a 401).
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": "garbage", "password": "a".repeat(1025) }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Login: an oversized password must short-circuit to 422 *before* Argon2 runs,
    // rather than being hashed against the (dummy or real) verifier.
    register(&app, "victim@example.com", "password123").await;
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "victim@example.com", "password": "a".repeat(5000) }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}
