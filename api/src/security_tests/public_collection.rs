//! Unauthenticated public collection reads (issues #361/#362): the `/api/u/{handle}`
//! surface. Asserts a private/unknown/bad handle is a uniform 404 (`no-store`, no
//! existence oracle), a public game is CDN-cacheable + ETag'd, per-user isolation holds,
//! and no PII (email / password hash) ever leaks into a public response.

use super::harness::*;
use crate::test_support::{insert_card_set, insert_product};

async fn sample_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "listing seeded cards failed: {body:?}"
    );
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

async fn own_product(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/collection/mtg/products/{id}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own product failed: {body:?}");
}

async fn set_username(app: &Router, token: &str, name: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            token,
            json!({ "username": name }),
        ),
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
        // The sealed-product reads share the same visibility gate as the card reads.
        format!("/api/u/{handle}/mtg/products"),
        format!("/api/u/{handle}/mtg/products/summary"),
        format!("/api/u/{handle}/mtg/products/sets"),
    ] {
        let (status, headers, _) = send(&app, get(&path)).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} should 404 while private"
        );
        assert_eq!(cache_control(&headers), Some("no-store"), "{path} cache");
    }
}

/// The public sealed-product reads (list / summary / per-set tiles) mirror the card reads:
/// 404 `no-store` while private, then the owner's exact sealed holdings with the shared-cache
/// policy once public. Isolated per owner via the same visibility gate.
#[tokio::test]
async fn public_sealed_products_are_readable_when_public() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    insert_card_set(db, "sealedset", "Sealed Set", Some("2024-05-01")).await;
    insert_product(
        db,
        "700",
        "Booster Box",
        "sealedset",
        "collector_display",
        Some("100.00"),
    )
    .await;
    insert_product(db, "701", "Bundle", "sealedset", "bundle", Some("40.00")).await;

    let (access, _) = register(&app, "sealedpub@example.test", "password one two").await;
    // Own 2 boxes and 1 bundle (700 owned last → recency-leading row).
    own_product(&app, &access, "700", 2).await;
    own_product(&app, &access, "701", 1).await;
    let handle = set_username(&app, &access, "sealedpub").await;
    set_public(&app, &access, true).await;

    // Summary: 2 unique products, 3 total copies, value 2×$100 + 1×$40.
    let (status, headers, body) =
        send(&app, get(&format!("/api/u/{handle}/mtg/products/summary"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public product summary failed: {body:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    assert_eq!(body["unique_products"], 2);
    assert_eq!(body["total_products"], 3);
    assert_eq!(body["total_value_usd"], "240.00");

    // Per-set tiles: the one set, shared-cacheable, scoped to the owner's holdings.
    let (status, headers, body) =
        send(&app, get(&format!("/api/u/{handle}/mtg/products/sets"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public product sets failed: {body:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    let sets = body["data"].as_array().expect("sets data array");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0]["code"], "sealedset");
    assert_eq!(sets[0]["unique_products"], 2);

    // Flat list: exactly the owner's owned products (recency order), shared-cacheable, and the
    // `?set=` filter narrows to that set.
    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/mtg/products"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public product list failed: {body:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    assert_eq!(body["total"], 2);
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["product"]["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["701", "700"]);

    let (status, _, body) = send(
        &app,
        get(&format!("/api/u/{handle}/mtg/products?set=sealedset")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set-scoped list failed: {body:?}");
    assert_eq!(body["total"], 2);
    let (_, _, body) = send(
        &app,
        get(&format!("/api/u/{handle}/mtg/products?set=ghost")),
    )
    .await;
    assert_eq!(body["total"], 0, "an unheld set is an empty page");
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
        assert!(
            !raw.contains("password_hash"),
            "{path} leaked a password hash"
        );
    }
}

#[tokio::test]
async fn bad_handles_are_404_no_store() {
    let app = test_app_with_catalog().await;
    for path in [
        "/api/u/nodash/mtg",     // no discriminator separator
        "/api/u/alice-xx/mtg",   // non-numeric discriminator
        "/api/u/alice-0/mtg",    // discriminator out of range
        "/api/u/ghost-0001/mtg", // well-formed but unknown user
        "/api/u/ghost-0001",     // unknown profile
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
async fn public_owned_counts_returns_owner_holdings_no_store() {
    let app = test_app_with_catalog().await;
    let ids = sample_card_ids(&app, 2).await; // [owned, unowned]
    let (handle, access) = owner_with_card(&app, "counts@example.test", "counter", &ids[0]).await;
    set_public(&app, &access, true).await;

    // The show-ghosts overlay POSTs the visible catalog ids; the owner holds only ids[0].
    let (status, headers, body) = send(
        &app,
        json_post_from(
            &format!("/api/u/{handle}/mtg/owned"),
            "9.9.9.9",
            json!({ "ids": ids }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "owned lookup failed: {body:?}");
    // Body-varying (keyed by the posted id list), so it must never be shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));
    // The owned card carries the owner's count (owner_with_card owns 2 regular); the unowned
    // card is simply absent from the map (mirrors the authed `/owned` semantics).
    assert_eq!(body["data"][ids[0].as_str()]["quantity"], 2);
    assert!(
        body["data"].get(ids[1].as_str()).is_none(),
        "an unowned card must be absent from the map"
    );
}

#[tokio::test]
async fn public_owned_counts_private_handle_is_404_no_store() {
    let app = test_app_with_catalog().await;
    let ids = sample_card_ids(&app, 1).await;
    // Owns a card + has a handle, but the game is never made public: a uniform 404, no oracle.
    let (handle, _) = owner_with_card(&app, "privcounts@example.test", "privc", &ids[0]).await;

    let (status, headers, _) = send(
        &app,
        json_post_from(
            &format!("/api/u/{handle}/mtg/owned"),
            "9.9.9.9",
            json!({ "ids": ids }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn public_owned_counts_over_cap_is_422() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, access) = owner_with_card(&app, "capcounts@example.test", "capc", &id).await;
    set_public(&app, &access, true).await;

    // One over the per-request id ceiling → 422 (the same cap the authed `/owned` enforces),
    // even though these ids don't resolve — the cap is checked before any lookup.
    let too_many: Vec<String> = (0..=crate::handlers::shared::MAX_OWNED_IDS)
        .map(|i| format!("fake-{i}"))
        .collect();
    let (status, _, _) = send(
        &app,
        json_post_from(
            &format!("/api/u/{handle}/mtg/owned"),
            "9.9.9.9",
            json!({ "ids": too_many }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
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
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 2);

    // Per-set landing: the set the owned card belongs to is present, shared-cacheable.
    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/mtg/sets"))).await;
    assert_eq!(status, StatusCode::OK, "public sets failed: {body:?}");
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    let sets = body["data"].as_array().expect("sets data array");
    assert!(
        !sets.is_empty(),
        "an owner of a card owns cards in >= 1 set"
    );

    // Sub-type view: reachable publicly for the set the owned card is in (the shared
    // owned_subtype_page core, handle-resolved). A 200 confirms the read path works.
    let code = sets[0]["code"].as_str().expect("set code");
    let (status, headers, _) = send(
        &app,
        get(&format!("/api/u/{handle}/mtg/sets/{code}/subtypes")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public set subtypes should be readable while public"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
}
