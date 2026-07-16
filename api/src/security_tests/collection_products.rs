//! HTTP-level coverage for collection sealed products (#435).

use super::harness::*;
use crate::entities::prelude::{Product, ProductPriceHistory};
use crate::entities::{product, product_price_history};
use crate::test_support::insert_product;
use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};

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

async fn internal_product_id(db: &sea_orm::DatabaseConnection, external_id: &str) -> i32 {
    Product::find()
        .filter(product::Column::Game.eq("mtg"))
        .filter(product::Column::ExternalId.eq(external_id))
        .one(db)
        .await
        .expect("query product")
        .expect("seeded product exists")
        .id
}

async fn set_product_price_history(
    db: &sea_orm::DatabaseConnection,
    product_id: i32,
    rows: &[(String, Option<&str>)],
) {
    ProductPriceHistory::delete_many()
        .filter(product_price_history::Column::Game.eq("mtg"))
        .filter(product_price_history::Column::ProductId.eq(product_id))
        .exec(db)
        .await
        .expect("wipe product history");
    let now = Utc::now();
    let models = rows
        .iter()
        .map(|(date, usd)| product_price_history::ActiveModel {
            game: Set("mtg".to_string()),
            product_id: Set(product_id),
            as_of_date: Set(date.clone()),
            price_usd: Set(usd.map(str::to_string)),
            price_usd_foil: Set(None),
            created_at: Set(now),
            ..Default::default()
        });
    ProductPriceHistory::insert_many(models)
        .exec(db)
        .await
        .expect("insert controlled product history");
}

fn day_offset(offset: i64) -> String {
    (Utc::now().date_naive() - Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string()
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
async fn sealed_holdings_feed_value_history_and_movers() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-analytics@example.com", "password123").await;
    insert_product(
        db,
        "100",
        "Gainer Box",
        "mkm",
        "collector_display",
        Some("20.00"),
    )
    .await;
    insert_product(
        db,
        "200",
        "Loser Box",
        "mkm",
        "draft_display",
        Some("15.00"),
    )
    .await;
    own_product(&app, &token, "100", 2).await;
    own_product(&app, &token, "200", 1).await;

    let (today, yesterday) = (day_offset(0), day_offset(1));
    set_product_price_history(
        db,
        internal_product_id(db, "100").await,
        &[
            (yesterday.clone(), Some("10.00")),
            (today.clone(), Some("20.00")),
        ],
    )
    .await;
    set_product_price_history(
        db,
        internal_product_id(db, "200").await,
        &[
            (yesterday.clone(), Some("20.00")),
            (today.clone(), Some("15.00")),
        ],
    )
    .await;

    let (status, _, history) = send(
        &app,
        get_with_bearer("/api/collection/mtg/value-history", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "value history failed: {history:?}");
    let today_point = history["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|point| point["date"] == today)
        .expect("today's point");
    assert!(
        today_point["value_usd"].is_null(),
        "no cards means no card line"
    );
    assert_eq!(today_point["sealed_value_usd"], "55.00", "2×$20 + 1×$15");
    let yesterday_point = history["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|point| point["date"] == yesterday)
        .expect("yesterday's point");
    assert_eq!(
        yesterday_point["sealed_value_usd"], "40.00",
        "current sealed holdings are revalued before their add date"
    );

    let (status, _, movers) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {movers:?}");
    assert!(
        movers["as_of"].is_null(),
        "sealed history must not change card as_of"
    );
    assert_eq!(movers["day"]["gainers"], json!([]));
    assert_eq!(movers["sealed"]["as_of"], today);
    let gainer = &movers["sealed"]["day"]["gainers"][0];
    assert_eq!(gainer["product"]["id"], "100");
    assert_eq!(gainer["change_usd"], "20.00", "two boxes gained $10 each");
    let loser = &movers["sealed"]["day"]["losers"][0];
    assert_eq!(loser["product"]["id"], "200");
    assert_eq!(loser["change_usd"], "-5.00");
}

#[tokio::test]
async fn ranged_value_history_carries_a_pre_cutoff_sealed_price_for_a_new_holding() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-range-anchor@example.com", "password123").await;
    insert_product(db, "300", "Sparse Box", "mkm", "bundle", Some("25.00")).await;
    own_product(&app, &token, "300", 1).await;

    let product_id = internal_product_id(db, "300").await;
    set_product_price_history(db, product_id, &[(day_offset(8), Some("25.00"))]).await;

    let (status, _, history) = send(
        &app,
        get_with_bearer("/api/collection/mtg/value-history?range=7d", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "value history failed: {history:?}");
    assert_eq!(history["data"][0]["date"], day_offset(7));
    assert_eq!(history["data"][0]["sealed_value_usd"], "25.00");
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
