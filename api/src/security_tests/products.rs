//! Public sealed-product catalog: the `/api/games/{game}/products*` reads are
//! publicly readable, shared-cacheable, and filter correctly; unknown game/product
//! ids are `no-store` 404s. Drives the real router in-process (no network), seeding
//! product fixtures straight into the harness DB.

use super::harness::*;
use crate::entities::product_price_history;
use crate::test_support::insert_product;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, NotSet};

/// Seed a small, deterministic set of sealed products across two sets + a few types.
async fn seed_products(app: &TestApp) {
    let db = &app.state.db;
    insert_product(db, "100", "Karlov Collector Booster Box", "mkm", "collector_display", Some("249.99")).await;
    insert_product(db, "200", "Karlov Bundle", "mkm", "bundle", Some("39.99")).await;
    insert_product(db, "300", "Bloomburrow Commander Deck", "blb", "commander_deck", Some("44.99")).await;
    // A product with no market price (exercises the null-price path in the list/sort).
    insert_product(db, "400", "Bloomburrow Draft Booster Box", "blb", "draft_display", None).await;
}

#[tokio::test]
async fn products_list_is_publicly_readable_and_shared_cacheable() {
    let app = test_app().await;
    seed_products(&app).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "product reads must be browser + CDN cacheable"
    );
    assert_eq!(body["total"], 4);
    assert_eq!(body["data"].as_array().unwrap().len(), 4);
    // The wire shape mirrors the card DTO idioms.
    let first = &body["data"][0];
    assert!(first["id"].is_string());
    assert!(first["prices"].is_object());
    assert!(first["product_type"].is_string());
}

#[tokio::test]
async fn product_detail_and_prices_are_readable() {
    let app = test_app().await;
    seed_products(&app).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/100")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["id"], "100");
    assert_eq!(body["set_code"], "mkm");
    assert_eq!(body["product_type"], "collector_display");
    assert_eq!(body["prices"]["usd"], "249.99");

    // Prices endpoint: empty (no history) but a clean, cacheable 200.
    let (status, headers, body) = send(&app, get("/api/games/mtg/products/100/prices")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // With a couple of history rows the series comes back oldest-first.
    let now = Utc::now();
    for (date, usd) in [("2024-05-01", "260.00"), ("2024-06-01", "249.99")] {
        product_price_history::ActiveModel {
            id: NotSet,
            game: Set("mtg".to_string()),
            product_id: Set(1), // first inserted product
            as_of_date: Set(date.to_string()),
            price_usd: Set(Some(usd.to_string())),
            price_usd_foil: Set(None),
            created_at: Set(now),
        }
        .insert(&app.state.db)
        .await
        .expect("insert history");
    }
    let (status, _, body) = send(&app, get("/api/games/mtg/products/100/prices")).await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["date"], "2024-05-01");
    assert_eq!(data[1]["date"], "2024-06-01");

    // An unknown range is a 422.
    let (status, _, _) = send(&app, get("/api/games/mtg/products/100/prices?range=week")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn unknown_game_and_product_are_no_store_404s() {
    let app = test_app().await;
    seed_products(&app).await;

    for uri in [
        "/api/games/nope/products",
        "/api/games/nope/products/100",
        "/api/games/mtg/products/999999",
        "/api/games/mtg/products/999999/prices",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 404 must be no-store");
    }
}

#[tokio::test]
async fn q_set_and_type_filters_narrow_the_list() {
    let app = test_app().await;
    seed_products(&app).await;

    // `q` is a case-insensitive name substring (not Scryfall syntax).
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=bundle")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "200");

    // `set` scopes to one set code (case-insensitively).
    let (_, _, body) = send(&app, get("/api/games/mtg/products?set=BLB")).await;
    assert_eq!(body["total"], 2);
    for p in body["data"].as_array().unwrap() {
        assert_eq!(p["set_code"], "blb");
    }

    // `type` filters on the classified product type.
    let (_, _, body) = send(&app, get("/api/games/mtg/products?type=collector_display")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "100");

    // Filters compose (set + type).
    let (_, _, body) =
        send(&app, get("/api/games/mtg/products?set=mkm&type=bundle")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "200");

    // A bad sort is a 422 (consistent with the card lists).
    let (status, _, _) = send(&app, get("/api/games/mtg/products?sort=nonsense")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn facets_expose_the_types_and_sets_in_use() {
    let app = test_app().await;
    seed_products(&app).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/facets")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));

    let types: Vec<&str> = body["data"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    // Alphabetical, distinct.
    assert_eq!(types, vec!["bundle", "collector_display", "commander_deck", "draft_display"]);

    let set_codes: Vec<&str> = body["data"]["sets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["code"].as_str().unwrap())
        .collect();
    assert!(set_codes.contains(&"mkm"));
    assert!(set_codes.contains(&"blb"));
}
