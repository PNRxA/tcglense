//! Username + per-game public-visibility toggle (issues #361/#362): the authed
//! set-username path (validation, discriminator allocation, rename), the visibility
//! toggle, API-key scope enforcement on both, and that secrets never leak.
//!
//! These drive the real router over the seeded dummy catalog, so a card can be owned by
//! its real external id and a collection made genuinely public.

use super::harness::*;

/// Grab `n` real card external ids from the seeded catalog.
async fn sample_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(status, StatusCode::OK, "listing seeded cards failed: {body:?}");
    body["data"]
        .as_array()
        .expect("cards data array")
        .iter()
        .take(n)
        .map(|c| c["id"].as_str().expect("card id").to_string())
        .collect()
}

/// Own one card (absolute counts) for the token's user.
async fn own_card(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/collection/mtg/cards/{id}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own card failed: {body:?}");
}

/// Mint an API key of the given scope for a session (`read` or `read_write`).
async fn create_key(app: &TestApp, access: &str, scope: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            json!({ "name": "k", "scope": scope }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {body:?}");
    body["key"].as_str().expect("plaintext key").to_string()
}

/// PUT a username for the token's user; returns the status + response body.
async fn set_username(app: &TestApp, token: &str, name: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(
        app,
        json_with_bearer("PUT", "/api/auth/username", token, json!({ "username": name })),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn set_username_reflects_in_me_and_hides_secrets() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "ash@pallet.town", "correct horse staple").await;

    let (status, body) = set_username(&app, &access, "ash_ketchum").await;
    assert_eq!(status, StatusCode::OK, "set username failed: {body:?}");
    assert_eq!(body["username"], "ash_ketchum");
    let disc = body["discriminator"].as_i64().expect("discriminator");
    assert!((1..=9999).contains(&disc), "discriminator out of range: {disc}");
    assert_eq!(body["handle"], format!("ash_ketchum-{disc:04}"));

    // `/me` now carries the handle, and no response ever leaks the password hash.
    let (status, _, me) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["username"], "ash_ketchum");
    assert_eq!(me["user"]["discriminator"], disc);
    assert!(!me.to_string().contains("password_hash"));
    assert!(!body.to_string().contains("password_hash"));
}

#[tokio::test]
async fn shared_username_gets_distinct_discriminators() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice@example.test", "password one two").await;
    let (bob, _) = register(&app, "bob@example.test", "password one two").await;

    let (_, a) = set_username(&app, &alice, "collector").await;
    let (_, b) = set_username(&app, &bob, "Collector").await; // case-insensitive same name

    assert_eq!(a["username"], "collector");
    assert_eq!(b["username"], "Collector");
    assert_ne!(
        a["discriminator"], b["discriminator"],
        "two users sharing a name must get different discriminators"
    );
}

#[tokio::test]
async fn rename_keeps_discriminator_when_free() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "rename@example.test", "password one two").await;

    let (_, first) = set_username(&app, &access, "ada").await;
    let disc = first["discriminator"].as_i64().expect("discriminator");

    // Rename to a fresh name whose #disc is free — the tag is kept.
    let (status, second) = set_username(&app, &access, "grace").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["username"], "grace");
    assert_eq!(second["discriminator"].as_i64().expect("discriminator"), disc);
}

#[tokio::test]
async fn invalid_usernames_are_422() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "val@example.test", "password one two").await;

    for name in ["ab", "bad name", "bad-name", "admin", "TCGLense", "fuck"] {
        let (status, body) = set_username(&app, &access, name).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "username {name:?} should be rejected: {body:?}"
        );
    }
}

#[tokio::test]
async fn read_only_key_cannot_set_username() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "ro@example.test", "password one two").await;
    let key = create_key(&app, &access, "read").await;

    // A read-only key must not be able to claim a handle — 403 (valid credential,
    // insufficient scope), not 401.
    let (status, _) = set_username(&app, &key, "sneaky").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn visibility_requires_auth() {
    let app = test_app_with_catalog().await;

    let (status, headers, _) = send(&app, get("/api/collection/mtg/visibility")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("PUT")
            .uri("/api/collection/mtg/visibility")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "public": true }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn enable_public_without_username_is_409_then_works() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "toggle@example.test", "password one two").await;

    // No username yet: enabling public is a 409 the SPA branches on.
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", "/api/collection/mtg/visibility", &access, json!({ "public": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // After choosing a username, the same request succeeds and reports the handle.
    set_username(&app, &access, "toggler").await;
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", "/api/collection/mtg/visibility", &access, json!({ "public": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "enable failed: {body:?}");
    assert_eq!(body["public"], true);
    assert!(body["handle"].as_str().is_some_and(|h| h.starts_with("toggler-")));

    // Reading the state back reflects it.
    let (status, _, get_body) =
        send(&app, get_with_bearer("/api/collection/mtg/visibility", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get_body["public"], true);

    // Disabling flips it back.
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", "/api/collection/mtg/visibility", &access, json!({ "public": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, get_body) =
        send(&app, get_with_bearer("/api/collection/mtg/visibility", &access)).await;
    assert_eq!(get_body["public"], false);
}

#[tokio::test]
async fn read_only_key_cannot_toggle_visibility() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "rokey@example.test", "password one two").await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    own_card(&app, &access, &id, 1).await;
    set_username(&app, &access, "keyholder").await;
    let key = create_key(&app, &access, "read").await;

    // The read-only key can READ the visibility state (a read)...
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg/visibility", &key)).await;
    assert_eq!(status, StatusCode::OK);

    // ...but not flip it (a write) — 403.
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", "/api/collection/mtg/visibility", &key, json!({ "public": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
