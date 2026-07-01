//! Refresh rotation + reuse detection (the full HTTP lifecycle).

use super::harness::*;

#[tokio::test]
async fn refresh_rotates_single_use_and_detects_token_theft() {
    let app = test_app().await;
    let (_, t1) = register(&app, "rotate@example.com", "password123").await;

    // t1 -> t2: success, new access token, rotated cookie.
    let (s, h2, b2) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b2["access_token"].as_str().is_some());
    let t2 = refresh_token_from(&h2).expect("rotated cookie t2");
    assert_ne!(t1, t2, "rotation must mint a new token");

    // Replaying t1 now (its successor t2 is still active) is a benign double-submit:
    // rejected and the cookie cleared, but the family is NOT burned.
    let (s, h, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(refresh_cookie_cleared(&h), "failed refresh must clear the cookie");

    // t2 still works -> t3 (proves the family survived the benign replay).
    let (s, h3, _) = send(&app, post_with_cookie("/api/auth/refresh", &t2)).await;
    assert_eq!(s, StatusCode::OK);
    let t3 = refresh_token_from(&h3).expect("rotated cookie t3");

    // Now replay t1 again: its successor t2 has itself been revoked, so this is
    // unambiguous theft — the whole family is burned.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // The live t3 is now dead too.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t3)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_without_a_cookie_is_unauthorized_and_mints_nothing() {
    let app = test_app().await;
    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/refresh")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // With no cookie there is nothing to clear, but a failed refresh must never
    // hand back a usable refresh token.
    assert!(refresh_token_from(&headers).is_none());
}

#[tokio::test]
async fn logout_revokes_the_refresh_token_and_is_idempotent() {
    let app = test_app().await;
    let (_, t1) = register(&app, "logout@example.com", "password123").await;

    let (status, headers, _) = send(&app, post_with_cookie("/api/auth/logout", &t1)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(refresh_cookie_cleared(&headers));

    // The revoked token can no longer be exchanged.
    let (status, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Logout with no cookie is still a clean 204.
    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
