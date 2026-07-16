//! HTTP-level coverage for collection sealed products (#435).

use super::harness::*;
use crate::test_support::insert_product;

fn product_path(id: &str) -> String {
    format!("/api/collection/mtg/products/{id}")
}

async fn own_product(app: &Router, token: &str, id: &str, quantity: i64) {
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
    assert_eq!(status, StatusCode::OK, "own product failed: {body:?}");
}

#[tokio::test]
async fn collection_products_are_private_and_no_store() {
    let app = test_app().await;
    for uri in [
        "/api/collection/mtg/products",
        "/api/collection/mtg/products/summary",
        "/api/collection/mtg/products/100",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }
    let (status, headers, _) = send(
        &app,
        json_post(
            "/api/collection/mtg/products/owned",
            json!({ "ids": ["100"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn set_get_list_summary_counts_and_remove_round_trip() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-owner@example.com", "password123").await;
    insert_product(
        db,
        "100",
        "Collector Booster Box",
        "mkm",
        "collector_display",
        Some("249.99"),
    )
    .await;
    insert_product(db, "200", "Bundle", "mkm", "bundle", Some("39.99")).await;

    let (status, _, body) = send(&app, get_with_bearer(&product_path("200"), &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["quantity"], 0);

    own_product(&app, &token, "100", 2).await;

    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &token)).await;
    assert_eq!(body["quantity"], 2);

    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["product"]["id"], "100");
    assert_eq!(body["data"][0]["quantity"], 2);

    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/summary", &token),
    )
    .await;
    assert_eq!(body["unique_products"], 1);
    assert_eq!(body["total_products"], 2);
    assert_eq!(body["total_value_usd"], "499.98");

    let (_, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/products/owned",
            &token,
            json!({ "ids": ["100", "200", "unknown"] }),
        ),
    )
    .await;
    assert_eq!(body["data"]["100"]["quantity"], 2);
    assert!(body["data"].get("200").is_none());

    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &product_path("100"),
            &token,
            json!({ "quantity": 0, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products", &token),
    )
    .await;
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn collection_and_wishlist_product_holdings_are_independent_and_user_scoped() {
    let app = test_app().await;
    let db = &app.state.db;
    let (alice, _) = register(&app, "sealed-alice@example.com", "password123").await;
    let (bob, _) = register(&app, "sealed-bob@example.com", "password123").await;
    insert_product(db, "100", "Bundle", "mkm", "bundle", Some("39.99")).await;

    own_product(&app, &alice, "100", 3).await;

    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &bob)).await;
    assert_eq!(body["quantity"], 0, "another user's holding leaked");
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/products/100", &alice),
    )
    .await;
    assert_eq!(body["quantity"], 0, "collection write changed wish list");

    let (_, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/products/100",
            &alice,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;
    let (_, _, body) = send(&app, get_with_bearer(&product_path("100"), &alice)).await;
    assert_eq!(body["quantity"], 3, "wish-list write changed collection");
}

#[tokio::test]
async fn validation_and_unknown_resources_match_card_holdings() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-errors@example.com", "password123").await;
    insert_product(db, "100", "Bundle", "mkm", "bundle", Some("39.99")).await;

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &product_path("missing"),
            &token,
            json!({ "quantity": -1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation precedes lookup"
    );

    let (status, _, _) = send(&app, get_with_bearer(&product_path("missing"), &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/nope/products", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
