//! Unauthenticated public **wish-list** reads (issue #493): the
//! `/api/u/{handle}/wishlist/{game}` surface plus the authed `/api/wishlist/{game}/visibility`
//! toggle. Asserts a private/unknown handle is a uniform 404 (`no-store`, no oracle), a public
//! wish list is CDN-cacheable + ETag'd, the wish-list flag is **independent** of the collection
//! flag (one public never leaks the other), API-key scope holds on the toggle, and the profile
//! lists public wish lists. Mirrors `public_collection` for the wish-list twin.

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

/// Want one card (absolute counts) on the token's user's wish list.
async fn want_card(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/wishlist/mtg/cards/{id}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "want card failed: {body:?}");
}

/// Want one sealed product (absolute counts) on the token's user's wish list.
async fn want_product(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/wishlist/mtg/products/{id}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "want product failed: {body:?}");
}

/// Own one card in the *collection* (to prove the two surfaces share independently).
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

/// Flip the wish-list sharing flag on the token's user's mtg wish list.
async fn set_wishlist_public(app: &Router, token: &str, public: bool) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/visibility",
            token,
            json!({ "public": public }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "set wishlist visibility failed: {body:?}"
    );
}

async fn set_collection_public(app: &Router, token: &str, public: bool) {
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
    assert_eq!(
        status,
        StatusCode::OK,
        "set collection visibility failed: {body:?}"
    );
}

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

/// Register a user who wants `id` in mtg and has a username; returns `(handle, access)`.
/// The wish list is left **private** — the caller opts it public when needed.
async fn wanter_with_card(app: &TestApp, email: &str, name: &str, id: &str) -> (String, String) {
    let (access, _) = register(app, email, "password one two").await;
    want_card(app, &access, id, 2).await;
    let handle = set_username(app, &access, name).await;
    (handle, access)
}

#[tokio::test]
async fn private_wishlist_is_404_no_store() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, _) = wanter_with_card(&app, "privwl@example.test", "hiddenwl", &id).await;

    // Wants cards + has a handle, but never made the wish list public — a uniform 404 (not
    // 403) so the surface confirms nothing, and never CDN-pinned.
    for path in [
        format!("/api/u/{handle}/wishlist/mtg"),
        format!("/api/u/{handle}/wishlist/mtg/summary"),
        format!("/api/u/{handle}/wishlist/mtg/sets"),
        format!("/api/u/{handle}/wishlist/mtg/products"),
        format!("/api/u/{handle}/wishlist/mtg/products/summary"),
        format!("/api/u/{handle}/wishlist/mtg/products/sets"),
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

#[tokio::test]
async fn public_wishlist_is_readable_and_cacheable() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);
    let (handle, access) = wanter_with_card(&app, "pubwl@example.test", "sharedwl", &id).await;
    set_wishlist_public(&app, &access, true).await;

    let (status, headers, body) = send(&app, get(&format!("/api/u/{handle}/wishlist/mtg"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public wishlist read failed: {body:?}"
    );
    // Handle-keyed, so shared-cacheable under the public-holdings policy + ETag'd.
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    let etag = headers
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .expect("etag on a public read")
        .to_string();

    // The list is exactly the owner's wanted cards.
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
        .uri(format!("/api/u/{handle}/wishlist/mtg"))
        .header("if-none-match", &etag)
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);

    // Summary reflects the two wanted copies of the one card.
    let (status, _, body) = send(&app, get(&format!("/api/u/{handle}/wishlist/mtg/summary"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public wishlist summary failed: {body:?}"
    );
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 2);
}

/// The wish-list flag and the collection flag are wholly independent: a public collection with
/// a private wish list exposes the collection but 404s the wish list, and vice versa.
#[tokio::test]
async fn wishlist_and_collection_sharing_are_independent() {
    let app = test_app_with_catalog().await;
    let ids = sample_card_ids(&app, 2).await;
    let (access, _) = register(&app, "indep@example.test", "password one two").await;
    // Own ids[0] in the collection, want ids[1] on the wish list.
    own_card(&app, &access, &ids[0], 1).await;
    want_card(&app, &access, &ids[1], 1).await;
    let handle = set_username(&app, &access, "indep").await;

    // Make ONLY the collection public.
    set_collection_public(&app, &access, true).await;
    let (status, _, _) = send(&app, get(&format!("/api/u/{handle}/mtg"))).await;
    assert_eq!(status, StatusCode::OK, "collection should be public");
    let (status, _, _) = send(&app, get(&format!("/api/u/{handle}/wishlist/mtg"))).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "wish list must stay private while only the collection is public"
    );

    // Now make ONLY the wish list public (collection back to private).
    set_collection_public(&app, &access, false).await;
    set_wishlist_public(&app, &access, true).await;
    let (status, _, _) = send(&app, get(&format!("/api/u/{handle}/mtg"))).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "collection must stay private while only the wish list is public"
    );
    let (status, _, body) = send(&app, get(&format!("/api/u/{handle}/wishlist/mtg"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "wish list should be public: {body:?}"
    );
    let ids_out: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["card"]["id"].as_str())
        .collect();
    assert_eq!(ids_out, vec![ids[1].as_str()], "only the wanted card shows");
}

#[tokio::test]
async fn public_wishlist_sealed_products_readable_when_public() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    insert_card_set(db, "wlset", "WL Set", Some("2024-05-01")).await;
    insert_product(
        db,
        "800",
        "Booster Box",
        "wlset",
        "collector_display",
        Some("100.00"),
    )
    .await;
    insert_product(db, "801", "Bundle", "wlset", "bundle", Some("40.00")).await;

    let (access, _) = register(&app, "wlsealed@example.test", "password one two").await;
    want_product(&app, &access, "800", 2).await;
    want_product(&app, &access, "801", 1).await;
    let handle = set_username(&app, &access, "wlsealed").await;
    set_wishlist_public(&app, &access, true).await;

    // Summary: 2 unique products, 3 total, value 2×$100 + 1×$40.
    let (status, headers, body) = send(
        &app,
        get(&format!("/api/u/{handle}/wishlist/mtg/products/summary")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public wl product summary failed: {body:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_HOLDINGS_CACHE)
    );
    assert_eq!(body["unique_products"], 2);
    assert_eq!(body["total_products"], 3);
    assert_eq!(body["total_value_usd"], "240.00");

    // Flat list: exactly the wanted products (recency order).
    let (status, _, body) =
        send(&app, get(&format!("/api/u/{handle}/wishlist/mtg/products"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public wl product list failed: {body:?}"
    );
    assert_eq!(body["total"], 2);
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["product"]["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["801", "800"]);
}

#[tokio::test]
async fn profile_lists_public_wishlists() {
    let app = test_app_with_catalog().await;
    let id = sample_card_ids(&app, 1).await.remove(0);

    // A user with only a public wish list (no public collection) still resolves, and the
    // profile lists that game under `wishlists` with its wish-list summary.
    let (handle, access) = wanter_with_card(&app, "wlprofile@example.test", "wlprof", &id).await;
    set_wishlist_public(&app, &access, true).await;

    let (status, _, body) = send(&app, get(&format!("/api/u/{handle}"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "profile should resolve for a public wish list"
    );
    assert!(
        body["games"].as_array().is_none_or(|g| g.is_empty()),
        "no public collection, so `games` is empty"
    );
    let wishlists = body["wishlists"].as_array().expect("wishlists array");
    assert_eq!(wishlists.len(), 1);
    assert_eq!(wishlists[0]["game"], "mtg");
    assert_eq!(wishlists[0]["summary"]["unique_cards"], 1);
}

#[tokio::test]
async fn wishlist_visibility_toggle_scope_and_gate() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "wltoggle@example.test", "password one two").await;

    // Fresh account, no username: enabling public is a 409 the SPA branches on.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/visibility",
            &access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Default read (still no row) reports private.
    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/visibility", &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["public"], false);

    // After a username the same request succeeds and reports the handle.
    set_username(&app, &access, "wltoggler").await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/visibility",
            &access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "enable failed: {body:?}");
    assert_eq!(body["public"], true);
    assert!(
        body["handle"]
            .as_str()
            .is_some_and(|h| h.starts_with("wltoggler-"))
    );

    // A read-only API key can READ the state but not flip it (403 — valid but unscoped).
    let key = create_key(&app, &access, "read").await;
    let (status, _, _) = send(&app, get_with_bearer("/api/wishlist/mtg/visibility", &key)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/visibility",
            &key,
            json!({ "public": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn public_wishlist_owned_counts_returns_wanted_holdings_no_store() {
    let app = test_app_with_catalog().await;
    let ids = sample_card_ids(&app, 2).await; // [wanted, unwanted]
    let (handle, access) =
        wanter_with_card(&app, "wlcounts@example.test", "wlcounter", &ids[0]).await;
    set_wishlist_public(&app, &access, true).await;

    let (status, headers, body) = send(
        &app,
        json_post_from(
            &format!("/api/u/{handle}/wishlist/mtg/owned"),
            "9.9.9.9",
            json!({ "ids": ids }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "wanted lookup failed: {body:?}");
    // Body-varying, so never shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["data"][ids[0].as_str()]["quantity"], 2);
    assert!(
        body["data"].get(ids[1].as_str()).is_none(),
        "an unwanted card must be absent from the map"
    );

    // A private handle 404s the same POST (no oracle).
    let (priv_handle, _) = wanter_with_card(&app, "wlpriv@example.test", "wlpriv", &ids[0]).await;
    let (status, headers, _) = send(
        &app,
        json_post_from(
            &format!("/api/u/{priv_handle}/wishlist/mtg/owned"),
            "9.9.9.9",
            json!({ "ids": ids }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}
