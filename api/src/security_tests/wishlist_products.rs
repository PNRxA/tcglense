//! Per-user wish-list **sealed products** (issue #364): authentication gating with the
//! private-route `no-store` policy, the set/get/list/remove round trip, unknown
//! game/product 404s, quantity bounds, per-user isolation, independence from the card
//! wish list and the collection, and recency-ordered pagination. The sealed-product
//! mirror of `super::wishlist`, but seeded directly (no dummy catalog): products go
//! straight into the harness DB via `insert_product`, exactly like `super::products`.
//!
//! These drive the real router in-process, so a product can be wanted by its external
//! (TCGplayer) id and read back in the full public `Product` wire shape.

use super::harness::*;
use crate::test_support::{insert_card, insert_product};

/// The wish-list entry path for one sealed product (external id) under `mtg`.
fn product_path(id: &str) -> String {
    format!("/api/wishlist/mtg/products/{id}")
}

/// Want `quantity` regular copies of one sealed product, absolute counts, for the token's
/// user. The sealed-product mirror of `super::wishlist::want_card`.
async fn want_product(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &product_path(id),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "want product failed: {body:?}");
}

#[tokio::test]
async fn wishlist_products_require_authentication() {
    let app = test_app().await;

    // The list and the single-entry GET are per-user reads: unauthenticated -> 401, and
    // never shared-cached.
    for uri in ["/api/wishlist/mtg/products", "/api/wishlist/mtg/products/100"] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }

    // The upsert PUT is just as private (401 + no-store) with no bearer.
    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("PUT")
            .uri(product_path("100"))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "quantity": 1, "foil_quantity": 0 }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn set_get_list_and_remove_round_trip() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-wisher@example.com", "password123").await;

    // Two products: one we'll want, one only seeded (to prove a known-but-unwanted read
    // is zeros, not a 404).
    insert_product(db, "100", "Karlov Collector Booster Box", "mkm", "collector_display", Some("249.99")).await;
    insert_product(db, "200", "Karlov Bundle", "mkm", "bundle", Some("39.99")).await;

    // A fresh wish list is empty (and no-store).
    let (status, headers, body) =
        send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["total"], 0);
    assert!(body["data"].as_array().unwrap().is_empty());

    // A seeded-but-never-wanted product reads back zeros, not a 404.
    let (status, _, body) = send(&app, get_with_bearer(&product_path("200"), &token)).await;
    assert_eq!(status, StatusCode::OK, "known product, no row -> zeros: {body:?}");
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);

    // Want 3.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &product_path("100"), &token, json!({ "quantity": 3, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set failed: {body:?}");
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 0);

    // The single-entry read reflects the row.
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &token)).await;
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 0);

    // The list carries one entry with the full product payload plus counts.
    let (status, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let entry = &body["data"][0];
    assert_eq!(entry["quantity"], 3);
    assert_eq!(entry["foil_quantity"], 0);
    assert_eq!(entry["product"]["id"], "100");
    assert!(entry["product"]["name"].as_str().is_some(), "entry embeds the product name");
    assert_eq!(entry["product"]["set_code"], "mkm");
    assert!(entry["product"]["prices"].is_object(), "entry embeds the product prices");

    // Updating the same product upserts (no duplicate row).
    let (_, _, body) = send(
        &app,
        json_with_bearer("PUT", &product_path("100"), &token, json!({ "quantity": 5, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(body["quantity"], 5);
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 1, "update must not create a second row");
    assert_eq!(body["data"][0]["quantity"], 5);

    // Zeroing both counts removes the product from the wish list.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &product_path("100"), &token, json!({ "quantity": 0, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "remove failed: {body:?}");
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &token)).await;
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 0);
}

/// The wire deliberately keeps two independent counts (regular + foil): a foil-only want
/// round-trips through the set/get/list/remove path exactly like a regular one, and the
/// list entry carries the foil count too. The sealed-product mirror of the foil coverage
/// in `super::wishlist::set_get_and_remove_round_trip`.
#[tokio::test]
async fn foil_quantity_round_trip() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "foil-sealed@example.com", "password123").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;

    // Want 0 regular + 3 foil.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &product_path("100"), &token, json!({ "quantity": 0, "foil_quantity": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set failed: {body:?}");
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 3);

    // The single-entry read reflects the foil-only row.
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &token)).await;
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 3);

    // The list carries the entry with its foil_quantity, not just the regular count.
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["quantity"], 0);
    assert_eq!(body["data"][0]["foil_quantity"], 3);

    // Zeroing both counts removes the row — a foil-only want deletes just like a regular one.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &product_path("100"), &token, json!({ "quantity": 0, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "remove failed: {body:?}");
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn unknown_game_and_product_are_404() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "nf-sealed@example.com", "password123").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;

    // An unknown game 404s on the list and both entry verbs.
    let (status, _, _) = send(&app, get_with_bearer("/api/wishlist/nope/products", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game list");
    let (status, _, _) =
        send(&app, get_with_bearer("/api/wishlist/nope/products/100", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game entry GET");
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", "/api/wishlist/nope/products/100", &token, json!({ "quantity": 1, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game entry PUT");

    // An unknown product id in a valid game 404s on both entry verbs.
    let (status, _, _) = send(&app, get_with_bearer(&product_path("999999"), &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown product entry GET");
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", &product_path("999999"), &token, json!({ "quantity": 1, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown product entry PUT");
}

#[tokio::test]
async fn quantity_bounds_are_422() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "neg-sealed@example.com", "password123").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;

    // Negative and oversized counts are both a 422 (the shared per-holding bounds), rejected
    // before the product is resolved — on either count independently.
    for body in [
        json!({ "quantity": -1, "foil_quantity": 0 }),
        json!({ "quantity": 1_000_001, "foil_quantity": 0 }),
        json!({ "quantity": 0, "foil_quantity": -1 }),
        json!({ "quantity": 0, "foil_quantity": 1_000_001 }),
    ] {
        let (status, _, _) = send(
            &app,
            json_with_bearer("PUT", &product_path("100"), &token, body),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // The bounds check runs before the product is resolved: an out-of-bounds body at an
    // *unknown* product id is still 422, not 404 — pinning the 422-before-404 ordering the
    // "rejected before the product is resolved" comment above claims.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &product_path("999999"),
            &token,
            json!({ "quantity": -1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "422 before 404: {body:?}");
}

#[tokio::test]
async fn per_user_isolation() {
    let app = test_app().await;
    let db = &app.state.db;
    let (alice, _) = register(&app, "alice-sealed@example.com", "password123").await;
    let (bob, _) = register(&app, "bob-sealed@example.com", "password123").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;

    // Alice wants 2.
    want_product(&app, &alice, "100", 2).await;

    // Bob sees nothing Alice added — a wish list is scoped to the token's user.
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &bob)).await;
    assert_eq!(body["total"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &bob)).await;
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);

    // Bob wanting the same product creates his own distinct row — no clobber of Alice's.
    want_product(&app, &bob, "100", 7).await;
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &alice)).await;
    assert_eq!(body["quantity"], 2, "alice's count is unchanged by bob's write");
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &bob)).await;
    assert_eq!(body["quantity"], 7);
}

/// The sealed-product wish list is its own table: wanting a product touches neither the
/// card wish list nor the collection, and wanting a card leaves the products list empty.
/// The sealed-product extension of the pinned collection/wish-list independence invariant
/// — the collection deliberately has no sealed surface at all (issue #364).
#[tokio::test]
async fn independent_of_card_wishlist_and_collection() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "both-sealed@example.com", "password123").await;
    // Seed one card + one product directly (no dummy catalog needed for either want).
    insert_card(db, "sf-364").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;

    // Wanting a card leaves the sealed-products list empty — a card want is not a product.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", "/api/wishlist/mtg/cards/sf-364", &token, json!({ "quantity": 2, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "want card failed: {body:?}");
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 0, "a card want must not populate the sealed-products wish list");

    // Wanting a product leaves the card wish list and the collection untouched.
    want_product(&app, &token, "100", 3).await;
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &token)).await;
    assert_eq!(body["total"], 1, "the card wish list still holds only the one card");
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(body["total"], 0, "the collection has no sealed surface");

    // …and the product is where it belongs.
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/products", &token)).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["product"]["id"], "100");
}

#[tokio::test]
async fn list_paginates() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "pager-sealed@example.com", "password123").await;
    insert_product(db, "100", "Booster Box", "mkm", "collector_display", Some("99.99")).await;
    insert_product(db, "200", "Bundle", "mkm", "bundle", Some("39.99")).await;
    insert_product(db, "300", "Commander Deck", "blb", "commander_deck", Some("44.99")).await;

    // Want all three in order, so the most-recently-wanted (300) leads the recency sort.
    want_product(&app, &token, "100", 1).await;
    want_product(&app, &token, "200", 1).await;
    want_product(&app, &token, "300", 1).await;

    // Page 1 of size 2: the two newest wants, with more to come.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/wishlist/mtg/products?page_size=2", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3);
    assert_eq!(body["has_more"], true);
    let page1 = body["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    // Recency-desc: 300 (most recent) then 200.
    assert_eq!(page1[0]["product"]["id"], "300", "the most recently wanted product leads");
    assert_eq!(page1[1]["product"]["id"], "200");

    // Page 2: the remaining one, no more.
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/products?page=2&page_size=2", &token),
    )
    .await;
    assert_eq!(body["total"], 3);
    assert_eq!(body["has_more"], false);
    let page2 = body["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["product"]["id"], "100");

    // Re-wanting 100 (id-oldest, currently trailing) must bump its recency to the front,
    // ahead of 300 — pinning both the recency sort itself and that an upsert (not just an
    // insert) bumps `updated_at`.
    want_product(&app, &token, "100", 2).await;
    let (status, _, body) =
        send(&app, get_with_bearer("/api/wishlist/mtg/products?page_size=2", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let page1 = body["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0]["product"]["id"], "100", "re-wanting bumps recency to the front");
    assert_eq!(page1[1]["product"]["id"], "300");
}
