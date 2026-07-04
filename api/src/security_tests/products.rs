//! Public sealed-product catalog: the `/api/games/{game}/products*` reads are
//! publicly readable, shared-cacheable, and filter correctly; unknown game/product
//! ids are `no-store` 404s. Drives the real router in-process (no network), seeding
//! product fixtures straight into the harness DB.

use super::harness::*;
use crate::entities::{card, product_price_history, sealed_content};
use crate::test_support::{insert_card, insert_product};
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

/// Insert one `sealed_contents` membership row for `(product, card)`.
async fn insert_sealed(
    db: &sea_orm::DatabaseConnection,
    product_id: i32,
    card_id: i32,
    membership: &str,
    foil: bool,
) {
    let now = Utc::now();
    sealed_content::ActiveModel {
        id: NotSet,
        game: Set("mtg".to_string()),
        product_id: Set(product_id),
        card_id: Set(card_id),
        membership: Set(membership.to_string()),
        foil: Set(foil),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .expect("insert sealed content");
}

#[tokio::test]
async fn card_sealed_endpoint_groups_products_by_membership() {
    let app = test_app().await;
    let db = &app.state.db;

    let card = insert_card(db, "sf-card").await;
    let _other = insert_card(db, "sf-other").await;
    let box_id =
        insert_product(db, "100", "Collector Booster Box", "mkm", "collector_display", Some("249.99")).await;
    let deck_id = insert_product(db, "300", "Commander Deck", "blb", "commander_deck", Some("44.99")).await;
    let sld_id = insert_product(db, "500", "Secret Lair Drop", "sld", "secret_lair", Some("29.99")).await;

    // The card is definitely in the deck, can be pulled (as a foil) from the box, and may
    // be in the Secret Lair.
    insert_sealed(db, deck_id, card, "contains", false).await;
    insert_sealed(db, box_id, card, "booster", true).await;
    insert_sealed(db, sld_id, card, "variable", true).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/sf-card/sealed")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the card-sealed read must be browser + CDN cacheable"
    );
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    // Ordered contains -> booster -> variable (the "found in / can be in / may be in" split).
    assert_eq!(data[0]["membership"], "contains");
    assert_eq!(data[0]["product"]["id"], "300");
    assert_eq!(data[0]["foil"], false);
    assert_eq!(data[1]["membership"], "booster");
    assert_eq!(data[1]["product"]["id"], "100");
    assert_eq!(data[1]["foil"], true, "a foil-only booster pull is flagged");
    assert_eq!(data[2]["membership"], "variable");
    assert_eq!(data[2]["product"]["id"], "500");
    // The nested product reuses the shared Product wire shape.
    assert!(data[0]["product"]["prices"].is_object());

    // A card in no product -> a clean, cacheable empty list.
    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/sf-other/sealed")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // An unknown card is a no-store 404.
    let (status, headers, _) = send(&app, get("/api/games/mtg/cards/nope/sealed")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn product_cards_endpoint_lists_cards_by_membership() {
    let app = test_app().await;
    let db = &app.state.db;

    // Cards insert in order, so their ids are 1, 2, 3 — the within-membership tiebreak.
    let card_a = insert_card(db, "sf-a").await;
    let card_b = insert_card(db, "sf-b").await;
    let card_c = insert_card(db, "sf-c").await;
    let box_id =
        insert_product(db, "100", "Collector Booster Box", "mkm", "collector_display", Some("249.99")).await;
    // A product with no ingested contents (the empty-page path).
    let _empty = insert_product(db, "200", "Empty Bundle", "mkm", "bundle", Some("9.99")).await;

    // card_a is both pullable (foil) from the box AND guaranteed in it (non-foil): it must
    // collapse to a single "contains", non-foil entry. card_c is guaranteed; card_b may be.
    insert_sealed(db, box_id, card_a, "booster", true).await;
    insert_sealed(db, box_id, card_a, "contains", false).await;
    insert_sealed(db, box_id, card_c, "contains", false).await;
    insert_sealed(db, box_id, card_b, "variable", true).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/100/cards")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the product-cards read must be browser + CDN cacheable"
    );
    assert_eq!(body["total"], 3, "three distinct cards, card_a deduped");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);

    // Guaranteed ("contains") cards lead — by id within the bucket — then "variable".
    assert_eq!(data[0]["card"]["id"], "sf-a");
    assert_eq!(data[0]["membership"], "contains");
    assert_eq!(data[0]["foil"], false, "a non-foil 'contains' beats the foil 'booster' row");
    assert_eq!(data[1]["card"]["id"], "sf-c");
    assert_eq!(data[1]["membership"], "contains");
    assert_eq!(data[2]["card"]["id"], "sf-b");
    assert_eq!(data[2]["membership"], "variable");
    assert_eq!(data[2]["foil"], true);
    // The nested card reuses the full shared Card wire shape.
    assert!(data[0]["card"]["prices"].is_object());
    assert!(data[0]["card"]["collector_number"].is_string());

    // Paginates by card, deterministically.
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards?page_size=2")).await;
    assert_eq!(body["total"], 3);
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["has_more"], true);
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards?page=2&page_size=2")).await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["card"]["id"], "sf-b");
    assert_eq!(body["has_more"], false);

    // A product with no ingested contents -> a clean, cacheable empty page.
    let (status, headers, body) = send(&app, get("/api/games/mtg/products/200/cards")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["total"], 0);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // Unknown game / product are no-store 404s.
    for uri in [
        "/api/games/nope/products/100/cards",
        "/api/games/mtg/products/999999/cards",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 404 must be no-store");
    }
}

/// Insert a card with a specific set code + collector number (the shared `insert_card`
/// hardcodes `tst` / `1` / NULL int, so it can't exercise the ordering tiebreaks).
async fn insert_card_at(
    db: &sea_orm::DatabaseConnection,
    external_id: &str,
    set_code: &str,
    collector_number: &str,
    collector_number_int: Option<i32>,
) -> i32 {
    let now = Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(format!("Card {external_id}")),
        set_code: Set(set_code.to_string()),
        set_name: Set("Test Set".to_string()),
        collector_number: Set(collector_number.to_string()),
        collector_number_int: Set(collector_number_int),
        lang: Set("en".to_string()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert card")
    .id
}

#[tokio::test]
async fn product_cards_order_within_membership_by_set_then_number() {
    let app = test_app().await;
    let db = &app.state.db;

    // All in one "contains" bucket, so the whole order is decided by the set/number
    // tiebreaks: set code first, then numeric collector number (10 after 2, not lexical),
    // then a non-numeric number (NULL int) parked last within its set.
    let box_id = insert_product(db, "700", "Bundle", "mkm", "bundle", Some("9.99")).await;
    let bbb1 = insert_card_at(db, "bbb-1", "bbb", "1", Some(1)).await;
    let aaa10 = insert_card_at(db, "aaa-10", "aaa", "10", Some(10)).await;
    let aaa2 = insert_card_at(db, "aaa-2", "aaa", "2", Some(2)).await;
    let aaax = insert_card_at(db, "aaa-x", "aaa", "X", None).await;
    for cid in [bbb1, aaa10, aaa2, aaax] {
        insert_sealed(db, box_id, cid, "contains", false).await;
    }

    let (status, _, body) = send(&app, get("/api/games/mtg/products/700/cards")).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["card"]["id"].as_str().unwrap())
        .collect();
    // aaa#2, aaa#10 (numeric), aaa#X (non-numeric last), then set bbb.
    assert_eq!(ids, vec!["aaa-2", "aaa-10", "aaa-x", "bbb-1"]);
}
