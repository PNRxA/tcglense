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
    assert_eq!(b2["user"]["email"], "rotate@example.com");
    assert!(b2["user"]["id"].as_i64().is_some());
    let t2 = refresh_token_from(&h2).expect("rotated cookie t2");
    assert_ne!(t1, t2, "rotation must mint a new token");

    // Replaying t1 now (its successor t2 is still active) is a BENIGN concurrent
    // double-submit — exactly what happens when two tabs (or a browser session-
    // restore, or a refetch-on-reconnect firing in every tab) present the same
    // not-yet-rotated cookie at once. It is rejected (401) and the family is NOT
    // burned, but crucially it must NOT emit a Set-Cookie: the browser already
    // holds the live t2 that the winning request set, and clearing it here would
    // race that Set-Cookie and log every tab out. So no refresh cookie is touched.
    let (s, h, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(
        !refresh_cookie_cleared(&h),
        "a benign concurrent double-submit must not clear the browser's live cookie"
    );
    assert!(
        refresh_token_from(&h).is_none(),
        "a benign double-submit mints no new cookie either — it leaves the jar alone"
    );

    // t2 still works -> t3 (proves the family survived the benign replay).
    let (s, h3, _) = send(&app, post_with_cookie("/api/auth/refresh", &t2)).await;
    assert_eq!(s, StatusCode::OK);
    let t3 = refresh_token_from(&h3).expect("rotated cookie t3");

    // Now replay t1 again: its successor t2 has itself been revoked, so this is
    // unambiguous theft — the whole family is burned AND the cookie is cleared
    // (a genuine dead session, unlike the benign replay above).
    let (s, h, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(
        refresh_cookie_cleared(&h),
        "detected token reuse must clear the cookie"
    );

    // The live t3 is now dead too.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t3)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

/// Reuse detection burns only the replayed token's FAMILY (one browser's login
/// lineage), not every session the user has: a second, independent login on the
/// same account must survive one browser's stale-jar replay (issue #417 — users
/// reporting logouts across all their devices).
#[tokio::test]
async fn reuse_detection_burns_only_the_replayed_family() {
    let app = test_app().await;
    // Device A registers (family A); device B logs into the SAME account (family B).
    let (_, a1) = register(&app, "twodevices@example.com", "password123").await;
    let (_, b1) = login(&app, "twodevices@example.com", "password123").await;

    // Device A rotates a1 -> a2 -> a3, so a1's successor a2 is itself revoked.
    let (s, ha2, _) = send(&app, post_with_cookie("/api/auth/refresh", &a1)).await;
    assert_eq!(s, StatusCode::OK);
    let a2 = refresh_token_from(&ha2).expect("a2");
    let (s, ha3, _) = send(&app, post_with_cookie("/api/auth/refresh", &a2)).await;
    assert_eq!(s, StatusCode::OK);
    let a3 = refresh_token_from(&ha3).expect("a3");

    // Replaying a1 is reuse: family A is burned (a3 no longer rotates)...
    let (s, h, _) = send(&app, post_with_cookie("/api/auth/refresh", &a1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(
        refresh_cookie_cleared(&h),
        "detected reuse clears the cookie"
    );
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &a3)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "family A is dead");

    // ...but device B's independent family is untouched: it still rotates.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &b1)).await;
    assert_eq!(s, StatusCode::OK, "the other device's session must survive");
}

/// Once reuse burns one login family, keeping the stolen ancestor must not let
/// an attacker repeatedly kill sessions the user creates afterward.
#[tokio::test]
async fn replaying_an_old_token_cannot_kill_a_fresh_login_family() {
    let app = test_app().await;
    let (_, t1) = register(&app, "family@example.com", "password123").await;

    // Advance this family twice so replaying t1 is unambiguous reuse.
    let (s, h2, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::OK);
    let t2 = refresh_token_from(&h2).expect("t2");
    let (s, h3, _) = send(&app, post_with_cookie("/api/auth/refresh", &t2)).await;
    assert_eq!(s, StatusCode::OK);
    let t3 = refresh_token_from(&h3).expect("t3");

    let (reuse, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(reuse, StatusCode::UNAUTHORIZED);
    let (burned, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t3)).await;
    assert_eq!(burned, StatusCode::UNAUTHORIZED);

    // A new login is a distinct family. Replaying exactly the same stolen t1
    // again remains a 401 but cannot revoke this new cookie.
    let (_, fresh) = login(&app, "family@example.com", "password123").await;
    let (reuse_again, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(reuse_again, StatusCode::UNAUTHORIZED);
    let (fresh_status, _, fresh_body) =
        send(&app, post_with_cookie("/api/auth/refresh", &fresh)).await;
    assert_eq!(fresh_status, StatusCode::OK);
    assert_eq!(fresh_body["user"]["email"], "family@example.com");
}

/// A genuinely unknown/invalid refresh cookie is a dead session: 401 AND the
/// cookie is cleared (distinct from the benign concurrent double-submit above,
/// which leaves the cookie intact).
#[tokio::test]
async fn refresh_with_an_unknown_cookie_clears_it() {
    let app = test_app().await;
    let (s, h, _) = send(
        &app,
        post_with_cookie("/api/auth/refresh", "deadbeefdeadbeefdeadbeefdeadbeef"),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(refresh_cookie_cleared(&h), "an unknown cookie is cleared");
    assert!(refresh_token_from(&h).is_none());
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
async fn logout_revokes_the_refresh_family_and_is_idempotent() {
    let app = test_app().await;
    let (_, t1) = register(&app, "logout@example.com", "password123").await;

    let (status, headers, _) = send(&app, post_with_cookie("/api/auth/logout", &t1)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(refresh_cookie_cleared(&headers));

    // The revoked token can no longer be exchanged.
    let (status, headers, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        refresh_cookie_cleared(&headers),
        "a server-revoked logout cookie is a definitive dead session"
    );

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
