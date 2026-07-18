//! HTTP-level coverage for collection sealed products (#435).

use super::harness::*;
use crate::entities::prelude::{Product, ProductPriceHistory};
use crate::entities::{product, product_price_history};
use crate::test_support::{insert_card_set, insert_product};
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
        "/api/collection/mtg/products/by-set",
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

    // Today's capture is unchanged. The 1D movers should therefore fall back to yesterday's
    // movement against the previous available capture (three days ago; two days ago is
    // deliberately absent, so that baseline is reached by carry-forward). The ten-day-old rows
    // keep the three-days-ago snapshot reachable only as the day-before-yesterday anchor (it
    // is neither the week anchor nor the earliest price), so the day assertions only pass when
    // that anchor resolves correctly — and they give the week window a newest-anchor movement
    // to pin the non-fallback sealed ranking. The ten-day and three-day prices deliberately
    // differ: `priced_at` carries forward, so equal ones would let a wrong baseline resolve to
    // the ten-day row and report the same numbers, leaving the fallback pinned by nothing.
    let (today, yesterday, three_days_ago, ten_days_ago) =
        (day_offset(0), day_offset(1), day_offset(3), day_offset(10));
    set_product_price_history(
        db,
        internal_product_id(db, "100").await,
        &[
            (ten_days_ago.clone(), Some("5.00")),
            (three_days_ago.clone(), Some("10.00")),
            (yesterday.clone(), Some("20.00")),
            (today.clone(), Some("20.00")),
        ],
    )
    .await;
    set_product_price_history(
        db,
        internal_product_id(db, "200").await,
        &[
            (ten_days_ago, Some("30.00")),
            (three_days_ago.clone(), Some("20.00")),
            (yesterday.clone(), Some("15.00")),
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
    // Three days back rather than yesterday: yesterday's capture is flat against today's (the
    // fallback needs it that way), so only a day whose prices actually differ can show the
    // holdings being revalued at the historic price instead of today's.
    let earlier_point = history["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|point| point["date"] == three_days_ago)
        .expect("the three-days-ago point");
    assert_eq!(
        earlier_point["sealed_value_usd"], "40.00",
        "current sealed holdings are revalued before their add date: 2×$10 + 1×$20"
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
    assert_eq!(movers["sealed"]["day_as_of"], yesterday);
    let gainer = &movers["sealed"]["day"]["gainers"][0];
    assert_eq!(gainer["product"]["id"], "100");
    assert_eq!(gainer["change_usd"], "20.00", "two boxes gained $10 each");
    let loser = &movers["sealed"]["day"]["losers"][0];
    assert_eq!(loser["product"]["id"], "200");
    assert_eq!(loser["change_usd"], "-5.00");
    // The week window ranks on the non-fallback path: today's capture against the
    // ten-day-old carry-forward, anchored at the sealed series' own newest snapshot.
    let week_gainer = &movers["sealed"]["week"]["gainers"][0];
    assert_eq!(week_gainer["product"]["id"], "100");
    assert_eq!(
        week_gainer["change_usd"], "30.00",
        "two boxes gained $15 each since d10"
    );
    let week_loser = &movers["sealed"]["week"]["losers"][0];
    assert_eq!(week_loser["product"]["id"], "200");
    assert_eq!(
        week_loser["change_usd"], "-15.00",
        "one box lost $15 since d10"
    );
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

#[tokio::test]
async fn by_set_groups_products_newest_set_first_with_aggregates() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "by-set-owner@example.com", "password123").await;

    // Two catalog sets with distinct release dates so the group order is deterministic:
    // "new" (2024-06) must lead "old" (2024-01).
    insert_card_set(db, "new", "Newest Set", Some("2024-06-01")).await;
    insert_card_set(db, "old", "Oldest Set", Some("2024-01-01")).await;

    // Two products in the newest set (names chosen to prove the case-insensitive intra-group
    // sort: "alpha bundle" < "Zebra Box"), one in the older set.
    insert_product(
        db,
        "10",
        "Zebra Box",
        "new",
        "collector_display",
        Some("10.00"),
    )
    .await;
    insert_product(db, "11", "alpha bundle", "new", "bundle", Some("5.00")).await;
    insert_product(
        db,
        "20",
        "Gamma Deck",
        "old",
        "commander_deck",
        Some("3.00"),
    )
    .await;

    // 2 regular + 1 foil of "10" (the foil finish is unpriced, so it lifts total_products but
    // not value), 1 of "11", 4 of "20".
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &product_path("10"),
            &token,
            json!({ "quantity": 2, "foil_quantity": 1 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own 10 failed: {body:?}");
    own_product(&app, &token, "11", 1).await;
    own_product(&app, &token, "20", 4).await;

    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/by-set", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "by-set failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    // The page unit is the set group, not the product.
    assert_eq!(body["total"], 2, "two sets => two groups");
    let groups = body["data"].as_array().unwrap();
    assert_eq!(groups.len(), 2);

    // Newest set leads, with its catalog name and per-group aggregates.
    let newest = &groups[0];
    assert_eq!(newest["code"], "new");
    assert_eq!(newest["name"], "Newest Set");
    assert_eq!(newest["unique_products"], 2);
    assert_eq!(newest["total_products"], 4, "2+1 foil of 10, plus 1 of 11");
    assert_eq!(
        newest["total_value_usd"], "25.00",
        "2×$10 + 1×$5 (foil unpriced)"
    );
    // Products name-sorted case-insensitively within the group: "alpha bundle" then "Zebra Box".
    let newest_products = newest["products"].as_array().unwrap();
    assert_eq!(newest_products[0]["product"]["id"], "11");
    assert_eq!(newest_products[1]["product"]["id"], "10");

    // The older set follows.
    let oldest = &groups[1];
    assert_eq!(oldest["code"], "old");
    assert_eq!(oldest["name"], "Oldest Set");
    assert_eq!(oldest["unique_products"], 1);
    assert_eq!(oldest["total_products"], 4);
    assert_eq!(oldest["total_value_usd"], "12.00", "4×$3");
}

#[tokio::test]
async fn by_set_unknown_set_group_has_null_name_and_orders_by_product_date() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "by-set-unknown@example.com", "password123").await;

    // A known set with an OLD release date, and a product in a set that has no `card_sets`
    // row at all. The unknown group falls back to the newest release date among its held
    // products (insert_product stamps "2024-02-09"), which beats the known set's 2024-01-01.
    insert_card_set(db, "kn", "Known Set", Some("2024-01-01")).await;
    insert_product(db, "10", "Known Box", "kn", "bundle", Some("1.00")).await;
    insert_product(db, "20", "Ghost Box", "ghost", "bundle", Some("2.00")).await;
    own_product(&app, &token, "10", 1).await;
    own_product(&app, &token, "20", 1).await;

    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/by-set", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "by-set failed: {body:?}");
    assert_eq!(body["total"], 2);
    let groups = body["data"].as_array().unwrap();

    // The unknown set sorts FIRST: its product-date fallback (2024-02-09) outranks the known
    // set's own release date (2024-01-01) — so the fallback is used, not treated as date-less.
    assert_eq!(groups[0]["code"], "ghost");
    assert!(
        groups[0]["name"].is_null(),
        "a set with no catalog row has a null name: {:?}",
        groups[0]
    );
    assert_eq!(groups[1]["code"], "kn");
    assert_eq!(groups[1]["name"], "Known Set");
}

#[tokio::test]
async fn by_set_paginates_over_groups() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "by-set-pager@example.com", "password123").await;

    insert_card_set(db, "new", "Newest Set", Some("2024-06-01")).await;
    insert_card_set(db, "old", "Oldest Set", Some("2024-01-01")).await;
    insert_product(db, "10", "New Box", "new", "bundle", Some("10.00")).await;
    insert_product(db, "20", "Old Box", "old", "bundle", Some("3.00")).await;
    own_product(&app, &token, "10", 1).await;
    own_product(&app, &token, "20", 1).await;

    // Page 1 of size 1: total counts sets (2), the newest set alone, and more to come.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/by-set?page_size=1", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["total"], 2,
        "total is the set count, not the product count"
    );
    assert_eq!(body["has_more"], true);
    let page1 = body["data"].as_array().unwrap();
    assert_eq!(page1.len(), 1);
    assert_eq!(page1[0]["code"], "new");

    // Page 2: the older set, no more.
    let (_, _, body) = send(
        &app,
        get_with_bearer(
            "/api/collection/mtg/products/by-set?page=2&page_size=1",
            &token,
        ),
    )
    .await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["has_more"], false);
    let page2 = body["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["code"], "old");
}

/// The by-set grouping is scoped to the caller's own collection: it never folds in a
/// wish-list want or another user's holding.
#[tokio::test]
async fn by_set_is_isolated_from_wishlist_and_other_users() {
    let app = test_app().await;
    let db = &app.state.db;
    let (alice, _) = register(&app, "by-set-alice@example.com", "password123").await;
    let (bob, _) = register(&app, "by-set-bob@example.com", "password123").await;
    insert_card_set(db, "new", "Newest Set", Some("2024-06-01")).await;
    insert_product(db, "10", "Owned Box", "new", "bundle", Some("10.00")).await;
    insert_product(db, "20", "Wanted Box", "new", "bundle", Some("5.00")).await;

    // Alice owns 10 and wants 20 (wish list). Her collection by-set shows only the owned one.
    own_product(&app, &alice, "10", 2).await;
    let (_, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/products/20",
            &alice,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;

    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/by-set", &alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["total"], 1,
        "the wanted product must not appear in the collection"
    );
    let products = body["data"][0]["products"].as_array().unwrap();
    assert_eq!(products.len(), 1);
    assert_eq!(products[0]["product"]["id"], "10");

    // Bob owns nothing: an empty page, not Alice's group.
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/products/by-set", &bob),
    )
    .await;
    assert_eq!(body["total"], 0);
    assert!(body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn sealed_movers_day_falls_back_across_a_missing_capture_day() {
    let app = test_app().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "sealed-movers-gap@example.com", "password123").await;
    insert_product(
        db,
        "100",
        "Gap Box",
        "mkm",
        "collector_display",
        Some("20.00"),
    )
    .await;
    own_product(&app, &token, "100", 2).await;

    // The feed skipped yesterday and three days ago. The newest capture repeats the one before
    // it, so 1D falls back — but onto a baseline no anchor column holds: `prev_day` stops at the
    // two-days-ago row, and the next-oldest anchor is the ten-day week one. Only the gap path's
    // own baseline fetch reaches the four-day row, and the four- and ten-day prices differ so
    // that carrying forward from the wrong one cannot report these numbers.
    let (d0, d2, d4, d10) = (day_offset(0), day_offset(2), day_offset(4), day_offset(10));
    set_product_price_history(
        db,
        internal_product_id(db, "100").await,
        &[
            (d10, Some("3.00")),
            (d4, Some("5.00")),
            (d2.clone(), Some("8.00")),
            (d0.clone(), Some("8.00")),
        ],
    )
    .await;

    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(
        body["sealed"]["as_of"], d0,
        "longer windows keep the newest anchor"
    );
    assert_eq!(
        body["sealed"]["day_as_of"], d2,
        "1D reports the previous available capture"
    );
    let gainer = &body["sealed"]["day"]["gainers"][0];
    assert_eq!(gainer["product"]["id"], "100");
    assert_eq!(
        gainer["value_prev"], "10.00",
        "two boxes at the four-day row"
    );
    assert_eq!(gainer["value_now"], "16.00");
    assert_eq!(gainer["change_usd"], "6.00");
    assert!(
        body["sealed"]["day"]["losers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
