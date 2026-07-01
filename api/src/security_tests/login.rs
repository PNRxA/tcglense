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
