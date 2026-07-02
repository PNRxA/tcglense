//! Registration / response hygiene.

use super::harness::*;

#[tokio::test]
async fn register_mints_no_session_and_never_leaks_secret_material() {
    let app = test_app().await;
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "User@Example.COM", "password": "password123", "display_name": "Tester" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    // Verify-first: registration mints NO session — no access token in the
    // body, no refresh cookie on the wire — until the emailed link is used.
    assert!(body["access_token"].is_null());
    assert!(refresh_token_from(&headers).is_none());
    // Email is canonicalised (trimmed + lowercased).
    assert_eq!(body["user"]["email"], "user@example.com");
    assert_eq!(body["user"]["display_name"], "Tester");

    // The public user shape must never carry secret material — including the
    // verification token, which must ride ONLY in the email (the response going
    // back to the unauthenticated caller must not shortcut the mailbox proof).
    let raw = body.to_string();
    assert!(!raw.contains("password_hash"), "leaked field name: {raw}");
    assert!(!raw.contains("$argon2"), "leaked a hash: {raw}");
    let token = latest_email_token(&app, "user@example.com").await;
    assert!(
        !raw.contains(&token),
        "the verification token must not appear in the response body"
    );
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
async fn register_rejects_invalid_email_and_weak_password() {
    let app = test_app().await;

    let (s1, _, b1) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "no-at-sign", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(s1, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b1["error"].as_str().is_some());

    let (s2, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "ok@example.com", "password": "short" }),
        ),
    )
    .await;
    assert_eq!(s2, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn duplicate_email_is_conflict_case_insensitively() {
    let app = test_app().await;
    register(&app, "Dup@Example.com", "password123").await;

    // Same address, different casing — the case-insensitive account must collide.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "dup@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
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
        password_hash: Set("irrelevant".to_string()),
        display_name: Set(None),
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

    // Register: a password past the 1024-char cap is a cheap-to-send /
    // expensive-to-hash Argon2 DoS -> 422.
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "big@example.com", "password": "a".repeat(1025) }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Register: an email past the 254-char cap -> 422.
    let long_email = format!("{}@example.com", "a".repeat(250));
    assert!(long_email.len() > 254);
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": long_email, "password": "password123" }),
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
