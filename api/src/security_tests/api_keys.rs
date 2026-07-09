//! HTTP-level tests for the public-API keys (issue #284), driving the real router.
//!
//! Covers the security-relevant behaviour a unit test of `auth::api_key` can't see on
//! its own: the plaintext is returned exactly once and never re-exposed by the list,
//! a key authenticates the collection/wish-list surface via `Authorization: Bearer`,
//! scope is enforced (a read-only key is 403 on a write), management is session-only
//! (a key can't manage keys), keys are isolated per user (no IDOR), and revoke works.

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// Mint a key for a signed-in user (via their session JWT) and return its one-time
/// plaintext.
async fn create_key(app: &TestApp, access: &str, name: &str, scope: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            json!({ "name": name, "scope": scope }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {body:?}");
    body["key"].as_str().expect("plaintext key").to_string()
}

/// A `DELETE` carrying a bearer credential (no helper exists for it in the harness).
fn delete_with_bearer(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn create_returns_plaintext_once_and_list_hides_it() {
    let app = test_app().await;
    let (access, _) = register(&app, "keys@example.com", PW).await;

    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "ci", "scope": "read_write" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body:?}");
    let plaintext = body["key"].as_str().expect("plaintext key");
    assert!(plaintext.starts_with("tcgl_"), "key should carry the label");
    assert_eq!(body["scope"], "read_write");
    assert!(
        body["key_prefix"].as_str().unwrap().starts_with("tcgl_"),
        "prefix is shown for later identification"
    );

    // The list returns metadata only — never the plaintext or the stored hash.
    let (status, _, body) = send(&app, get_with_bearer("/api/auth/api-keys", &access)).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["data"].as_array().expect("data array");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.get("key").is_none(), "list must not carry the plaintext");
    assert!(
        item.get("token_hash").is_none(),
        "list must not carry the stored hash"
    );
    assert_eq!(item["name"], "ci");
    assert_eq!(item["scope"], "read_write");
    assert!(item["key_prefix"].as_str().unwrap().starts_with("tcgl_"));
}

#[tokio::test]
async fn read_write_key_authenticates_reads_and_passes_the_write_gate() {
    let app = test_app().await;
    let (access, _) = register(&app, "rw@example.com", PW).await;
    let key = create_key(&app, &access, "rw", "read_write").await;

    // A read is served for the key's owner.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &key)).await;
    assert_eq!(status, StatusCode::OK);

    // A write passes the scope gate (the `WritableUser` extractor accepts read_write);
    // the handler then 404s the unknown card — the point is it is NOT a 403.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/cards/does-not-exist",
            &key,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:?}");
}

#[tokio::test]
async fn read_only_key_can_read_but_not_write() {
    let app = test_app().await;
    let (access, _) = register(&app, "ro@example.com", PW).await;
    let key = create_key(&app, &access, "ro", "read").await;

    // GET reads are allowed.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &key)).await;
    assert_eq!(status, StatusCode::OK);

    // The batch-count POST is a *read*, so a read-only key may call it.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/owned",
            &key,
            json!({ "ids": ["x"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A mutation is forbidden — 403 (valid credential, insufficient scope), NOT 401.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/cards/does-not-exist",
            &key,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body:?}");

    // The wish-list write is likewise forbidden.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/cards/does-not-exist",
            &key,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_keys_cannot_manage_api_keys() {
    let app = test_app().await;
    let (access, _) = register(&app, "mgmt@example.com", PW).await;
    let key = create_key(&app, &access, "k", "read_write").await;

    // Presenting the KEY (not the session JWT) to any management verb is 403.
    let (status, _, _) = send(&app, get_with_bearer("/api/auth/api-keys", &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &key,
            json!({ "name": "n", "scope": "read" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = send(&app, delete_with_bearer("/api/auth/api-keys/1", &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn keys_are_isolated_and_revoke_is_scoped_and_idempotent() {
    let app = test_app().await;
    let (alice, _) = register(&app, "alice@example.com", PW).await;
    let (bob, _) = register(&app, "bob@example.com", PW).await;

    // Alice mints a key.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &alice,
            json!({ "name": "alice-key", "scope": "read" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let alice_key_id = body["id"].as_i64().expect("id");
    let alice_plaintext = body["key"].as_str().unwrap().to_string();

    // Bob's list doesn't include Alice's key.
    let (_, _, body) = send(&app, get_with_bearer("/api/auth/api-keys", &bob)).await;
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // Bob cannot revoke Alice's key — 404 (ids don't leak across accounts).
    let (status, _, _) = send(
        &app,
        delete_with_bearer(&format!("/api/auth/api-keys/{alice_key_id}"), &bob),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Alice's key still works.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &alice_plaintext)).await;
    assert_eq!(status, StatusCode::OK);

    // Alice revokes it -> 204; the key stops working (401); re-revoke is idempotent 204.
    let (status, _, _) = send(
        &app,
        delete_with_bearer(&format!("/api/auth/api-keys/{alice_key_id}"), &alice),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", &alice_plaintext)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = send(
        &app,
        delete_with_bearer(&format!("/api/auth/api-keys/{alice_key_id}"), &alice),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // A never-existed id is 404.
    let (status, _, _) = send(&app, delete_with_bearer("/api/auth/api-keys/999999", &alice)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_or_missing_credential_is_401() {
    let app = test_app().await;
    // A bogus tcgl_ key resolves to no user.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/mtg", "tcgl_deadbeef")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // No credential at all.
    let (status, _, _) = send(&app, get("/api/collection/mtg")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_validates_scope_name_and_expiry() {
    let app = test_app().await;
    let (access, _) = register(&app, "val@example.com", PW).await;

    // Unknown scope.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "n", "scope": "admin" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Blank name.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "   ", "scope": "read" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Zero-day expiry.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "n", "scope": "read", "expires_in_days": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn per_user_active_key_cap_is_enforced() {
    let app = test_app().await;
    let (access, _) = register(&app, "cap@example.com", PW).await;

    // The cap is 25 active keys.
    for i in 0..25 {
        let (status, _, body) = send(
            &app,
            json_with_bearer(
                "POST",
                "/api/auth/api-keys",
                &access,
                json!({ "name": format!("k{i}"), "scope": "read" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "key {i} failed: {body:?}");
    }

    // The 26th is refused with 409.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "over", "scope": "read" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}
