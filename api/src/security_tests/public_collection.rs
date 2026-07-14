//! Unauthenticated public collection reads (issues #361/#362): the `/api/u/{handle}`
//! surface. Asserts a private/unknown/bad handle is a uniform 404 (`no-store`, no
//! existence oracle), a public game is CDN-cacheable + ETag'd, per-user isolation holds,
//! and no PII (email / password hash) ever leaks into a public response.

use super::harness::*;

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

async fn set_username(app: &Router, token: &str, name: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer("PUT", "/api/auth/username", token, json!({ "username": name })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set username failed: {body:?}");
    body["handle"].as_str().expect("handle").to_string()
}

async fn set_public(app: &Router, token: &str, public: bool) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/visibility",
            token,
            json!({ "public": public }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set visibility failed: {body:?}");
}

/// Register a user who owns `id` in mtg and has a username; returns `(handle, access)`.
/// The mtg collection is left **private** — the caller opts it public when needed.
async fn owner_with_card(app: &TestApp, email: &str, name: &str, id: &str) -> (String, String) {
    let (access, _) = register(app, email, "password one two").await;
    own_card(app, &access, id, 2).await;
    let handle = set_username(app, &access, name).await;
    (handle, access)
}

#[tokio::test]
async fn private_game_is_404_no_store() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, _) = owner_with_card(&app, "priv@example.test", "hidden", &id).await;

    // Owns cards + has a handle, but never made mtg public — a uniform 404 (not 403) so the
    // surface confirms nothing, and never CDN-pinned.
    for path in [
        format!("/api/u/{handle}/mtg"),
        format!("/api/u/{handle}/mtg/summary"),
        format!("/api/u/{handle}/mtg/sets"),
    ] {
        let (status, headers, _) = send(&app, get(&path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should 404 while private");
        assert_eq!(cache_control(&headers), Some("no-store"), "{path} cache");
    }
}

#[tokio::test]
async fn public_game_is_readable_and_cacheable() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, access) = owner_with_card(&app, "pub@example.test", "shared", &id).await;
    set_public(&app, &access, true).await;

    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/mtg"))).await;
    assert_eq!(status, StatusCode::OK, "public read failed: {body:?}");
    // Handle-keyed, so shared-cacheable under the shorter public-holdings policy...
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    // ...and ETag'd for cheap revalidation.
    let etag = headers
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .expect("etag on a public read")
        .to_string();

    // The list is exactly the owner's owned cards.
    let ids: Vec<&str> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .filter_map(|e| e["card"]["id"].as_str())
        .collect();
    assert_eq!(ids, vec![id.as_str()]);

    // A matching `If-None-Match` comes back `304 Not Modified`.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/u/{handle}/mtg"))
        .header("if-none-match", &etag)
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn public_reads_leak_no_pii() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let email = "secret.owner@example.test";
    let (handle, access) = owner_with_card(&app, email, "nopii", &id).await;
    set_public(&app, &access, true).await;

    for path in [format!("/api/u/{handle}"), format!("/api/u/{handle}/mtg")] {
        let (status, _, body) = send(&app, get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path} failed: {body:?}");
        let raw = body.to_string();
        assert!(!raw.contains(email), "{path} leaked the owner's email");
        assert!(!raw.contains("password_hash"), "{path} leaked a password hash");
    }
}

#[tokio::test]
async fn bad_handles_are_404_no_store() {
    let app = test_app_with_catalog().await;
    for path in [
        "/api/u/nodash/mtg",      // no discriminator separator
        "/api/u/alice-xx/mtg",    // non-numeric discriminator
        "/api/u/alice-0/mtg",     // discriminator out of range
        "/api/u/ghost-0001/mtg",  // well-formed but unknown user
        "/api/u/ghost-0001",      // unknown profile
    ] {
        let (status, headers, _) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{path} cache");
    }
}

#[tokio::test]
async fn profile_lists_only_public_games() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);

    // A user with a username but no public game: the profile 404s (no bare-profile leak).
    let (private_handle, _) = owner_with_card(&app, "noprofile@example.test", "nogames", &id).await;
    let (status, _, _) = send(&app, get(&format!("/api/u/{private_handle}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Once mtg is public, the profile lists exactly that game with its summary.
    let (handle, access) = owner_with_card(&app, "profile@example.test", "haspublic", &id).await;
    set_public(&app, &access, true).await;
    let (status, _, body) = send(&app, get(&format!("/api/u/{handle}"))).await;
    assert_eq!(status, StatusCode::OK);
    let games = body["games"].as_array().expect("games array");
    assert_eq!(games.len(), 1);
    assert_eq!(games[0]["game"], "mtg");
    assert_eq!(games[0]["summary"]["unique_cards"], 1);
}

#[tokio::test]
async fn public_collections_are_isolated_per_handle() {
    let app = test_app_with_catalog().await;
    let ids = sample_card_ids(&app, 2).await;

    // Alice public with her card; Bob private with a different card.
    let (alice, alice_access) = owner_with_card(&app, "alice@example.test", "alice", &ids[0]).await;
    set_public(&app, &alice_access, true).await;
    let (bob, bob_access) = owner_with_card(&app, "bob@example.test", "bob", &ids[1]).await;
    let _ = &bob_access; // Bob's collection stays private.

    // Alice's public page shows only Alice's card.
    let (status, _, body) = send(&app, get(&format!("/api/u/{alice}/mtg"))).await;
    assert_eq!(status, StatusCode::OK);
    let alice_ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["card"]["id"].as_str())
        .collect();
    assert_eq!(alice_ids, vec![ids[0].as_str()]);

    // Bob is private, so his handle 404s even though he owns cards.
    let (status, _, _) = send(&app, get(&format!("/api/u/{bob}/mtg"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The remaining public read surfaces — summary, the per-set landing, and a set's sub-type
/// view — return the owner's data with a 200 and the shared-cacheable policy. Previously only
/// `private_game_is_404_no_store` touched summary/sets (asserting the 404 side); this pins
/// their success paths. (The `drops` view shares the exact `owned_drop_page` core the authed
/// `collection_set_drops` handler exercises, and the seeded test catalog has no drop-grouped
/// set — even the authed test 404s `dmb` drops — so its 200 isn't reachable from here.)
#[tokio::test]
async fn public_summary_sets_and_subtypes_are_readable() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, access) = owner_with_card(&app, "views@example.test", "views", &id).await;
    set_public(&app, &access, true).await;

    // Summary: the owner owns 2 copies of exactly one card (see `owner_with_card`).
    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/mtg/summary"))).await;
    assert_eq!(status, StatusCode::OK, "public summary failed: {body:?}");
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE));
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 2);

    // Per-set landing: the set the owned card belongs to is present, shared-cacheable.
    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/mtg/sets"))).await;
    assert_eq!(status, StatusCode::OK, "public sets failed: {body:?}");
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE));
    let sets = body["data"].as_array().expect("sets data array");
    assert!(!sets.is_empty(), "an owner of a card owns cards in >= 1 set");

    // Sub-type view: reachable publicly for the set the owned card is in (the shared
    // owned_subtype_page core, handle-resolved). A 200 confirms the read path works.
    let code = sets[0]["code"].as_str().expect("set code");
    let (status, headers, _) =
        send(&app, get(&format!("/api/u/{handle}/mtg/sets/{code}/subtypes"))).await;
    assert_eq!(status, StatusCode::OK, "public set subtypes should be readable while public");
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE));
}
