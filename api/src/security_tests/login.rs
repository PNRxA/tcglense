//! Login — generic failures (no user enumeration) — and the Bearer-protected route.

use super::harness::*;

#[tokio::test]
async fn login_succeeds_and_failures_are_generic() {
    let app = test_app().await;
    register(&app, "login@example.com", "password123").await;

    let (ok_status, ok_headers, ok_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "login@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(ok_status, StatusCode::OK);
    assert!(ok_body["access_token"].as_str().is_some());
    assert!(refresh_token_from(&ok_headers).is_some());

    let (wrong_pw_status, _, wrong_pw_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "login@example.com", "password": "wrong-password" }),
        ),
    )
    .await;
    let (no_user_status, _, no_user_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "ghost@example.com", "password": "password123" }),
        ),
    )
    .await;

    // Both 401, and the message is identical — no oracle for "does this user exist".
    assert_eq!(wrong_pw_status, StatusCode::UNAUTHORIZED);
    assert_eq!(no_user_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_pw_body["error"], "invalid email or password");
    assert_eq!(wrong_pw_body["error"], no_user_body["error"]);
}

#[tokio::test]
async fn me_requires_a_valid_bearer_token() {
    let app = test_app().await;
    let (access, _) = register(&app, "me@example.com", "password123").await;

    let (ok_status, _, ok_body) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(ok_status, StatusCode::OK);
    assert_eq!(ok_body["user"]["email"], "me@example.com");
    assert_eq!(ok_body["user"]["currency"], "USD");

    // Missing header.
    let (missing, _, _) = send(&app, get("/api/auth/me")).await;
    assert_eq!(missing, StatusCode::UNAUTHORIZED);

    // Malformed scheme.
    let (malformed, _, _) = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .header(AUTHORIZATION, "Token abc")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(malformed, StatusCode::UNAUTHORIZED);

    // Garbage / forged token.
    let (garbage, _, _) = send(&app, get_with_bearer("/api/auth/me", "not.a.jwt")).await;
    assert_eq!(garbage, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn display_currency_is_validated_and_persisted_on_the_account() {
    let app = test_app().await;
    let (access, _) = register(&app, "currency@example.com", "password123").await;

    let (status, headers, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/currency",
            &access,
            json!({ "currency": "AUD" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "currency update failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["currency"], "AUD");

    // A fresh read proves the preference was stored, not only echoed by the write.
    let (status, _, body) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["currency"], "AUD");

    // Codes are exact and restricted to the supported set; a failed update leaves the
    // previous preference untouched.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/currency",
            &access,
            json!({ "currency": "aud" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("USD, AUD"));

    let (_, _, body) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(body["user"]["currency"], "AUD");
}

/// A cryptographically-valid, unexpired access token stops working the instant its
/// account is deleted: the `AuthUser` extractor re-loads the user by the token's
/// subject on every request and rejects a token whose user no longer exists. This is
/// the only server-side check that a deleted/deactivated account can't keep acting on
/// authenticated per-user data for the token's remaining lifetime.
#[tokio::test]
async fn a_deleted_users_access_token_is_rejected() {
    use crate::entities::{prelude::User, user};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let app = test_app().await;
    let (access, _) = register(&app, "gone@example.com", "password123").await;

    // The token authenticates while the account exists.
    let (ok, _, _) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(ok, StatusCode::OK);

    // Delete the account (its collection/wishlist/token rows cascade).
    let uid = User::find()
        .filter(user::Column::Email.eq("gone@example.com"))
        .one(&app.state.db)
        .await
        .expect("query user")
        .expect("user exists")
        .id;
    User::delete_by_id(uid)
        .exec(&app.state.db)
        .await
        .expect("delete user");

    // The still-unexpired token no longer authenticates — not to `/me`, nor to the
    // authenticated per-user data surface (the extractor gates both).
    let (me, _, _) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(me, StatusCode::UNAUTHORIZED, "a deleted user's token must not authenticate");
    let (collection, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &access)).await;
    assert_eq!(collection, StatusCode::UNAUTHORIZED);
}

/// An expired-but-correctly-signed access token is rejected by the extractor on a
/// real protected route — pinning that the request path actually enforces `exp`
/// (the jwt unit test proves only the primitive rejects it). The token is forged for
/// the real user with the harness signing secret so its ONLY defect is the past
/// expiry: rejection can't be a bad signature or a missing user.
#[tokio::test]
async fn an_expired_access_token_is_rejected_by_a_protected_route() {
    use crate::auth::jwt::Claims;
    use crate::entities::{prelude::User, user};
    use chrono::{Duration, Utc};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let app = test_app().await;
    register(&app, "stale@example.com", "password123").await;

    let uid = User::find()
        .filter(user::Column::Email.eq("stale@example.com"))
        .one(&app.state.db)
        .await
        .expect("query user")
        .expect("user exists")
        .id;

    let past = (Utc::now() - Duration::hours(1)).timestamp() as usize;
    let claims = Claims {
        sub: uid.to_string(),
        email: "stale@example.com".to_string(),
        iat: past - 60,
        exp: past,
    };
    let expired = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(app.state.config.jwt_secret.as_bytes()),
    )
    .expect("encode expired token");

    let (me, _, _) = send(&app, get_with_bearer("/api/auth/me", &expired)).await;
    assert_eq!(me, StatusCode::UNAUTHORIZED, "an expired token must not authenticate");
    let (collection, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &expired)).await;
    assert_eq!(collection, StatusCode::UNAUTHORIZED);
}
