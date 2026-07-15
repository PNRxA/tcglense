//! Public sealed-product catalog: the `/api/games/{game}/products*` reads are
//! publicly readable, shared-cacheable, and filter correctly; unknown game/product
//! ids are `no-store` 404s. Drives the real router in-process (no network), seeding
//! product fixtures straight into the harness DB.

use super::harness::*;
use crate::entities::{card, product_price_history, sealed_component, sealed_content};
use crate::test_support::{insert_card, insert_product, set_product_msrp};
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
    // Product 100 has a curated MSRP; product 400 (seeded by seed_products above) has none.
    set_product_msrp(&app.state.db, "100", "179.99").await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/100")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["id"], "100");
    assert_eq!(body["set_code"], "mkm");
    assert_eq!(body["product_type"], "collector_display");
    assert_eq!(body["prices"]["usd"], "249.99");
    // MSRP (curated retail price) surfaces on the wire when set, and is null otherwise.
    assert_eq!(body["msrp"], "179.99");
    let (_, _, no_msrp) = send(&app, get("/api/games/mtg/products/400")).await;
    assert!(no_msrp["msrp"].is_null(), "a product with no curated MSRP reports null");

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

    // `q` matches each word as a case-insensitive name substring (not Scryfall
    // syntax); a single word behaves like a plain substring.
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

// Issue #273: a multi-word `q` finds products whose name contains *every* word, even
// when the words are not adjacent — "karlov box" matches "Karlov Collector Booster
// Box". Each whitespace-separated word is an independent, order-independent,
// case-insensitive name substring, AND-ed together.
#[tokio::test]
async fn q_matches_each_word_as_an_order_independent_substring() {
    let app = test_app().await;
    seed_products(&app).await;

    // Non-adjacent words match: only "Karlov Collector Booster Box" (100) has both
    // "karlov" and "box" — the plain-substring "karlov box" used to match nothing.
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=karlov%20box")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "100");

    // Order-independent: reversing the words gives the same single match.
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=box%20karlov")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "100");

    // Case is folded on both sides, and runs of whitespace collapse.
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=KARLOV%20%20Box")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], "100");

    // Words shared by two products return both ("booster box" -> 100 and 400).
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=booster%20box")).await;
    assert_eq!(body["total"], 2);
    let ids: Vec<&str> =
        body["data"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"100") && ids.contains(&"400"));

    // AND semantics: no single product carries both words -> nothing matches.
    let (_, _, body) = send(&app, get("/api/games/mtg/products?q=karlov%20bloomburrow")).await;
    assert_eq!(body["total"], 0);
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

/// Insert one `sealed_components` composition row for a product.
#[allow(clippy::too_many_arguments)]
async fn insert_component(
    db: &sea_orm::DatabaseConnection,
    product_id: i32,
    position: i32,
    kind: &str,
    name: &str,
    quantity: i32,
    child_product_id: Option<i32>,
    child_card_id: Option<i32>,
) {
    let now = Utc::now();
    sealed_component::ActiveModel {
        id: NotSet,
        game: Set("mtg".to_string()),
        product_id: Set(product_id),
        position: Set(position),
        kind: Set(kind.to_string()),
        name: Set(name.to_string()),
        quantity: Set(quantity),
        child_product_id: Set(child_product_id),
        child_card_id: Set(child_card_id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .expect("insert sealed component");
}

#[tokio::test]
async fn product_contents_endpoint_lists_composition_with_links() {
    let app = test_app().await;
    let db = &app.state.db;

    let bundle = insert_product(db, "600", "Commander's Bundle", "tla", "bundle", Some("59.99")).await;
    let pack = insert_product(db, "640", "Play Booster Pack", "tla", "play_pack", Some("4.99")).await;
    let promo = insert_card(db, "sf-promo").await;
    // A product with no ingested composition (the empty path).
    insert_product(db, "700", "Empty Deck", "tla", "commander_deck", Some("44.99")).await;

    // 9x Play Booster (linked to the pack), 1x foil promo (linked to the card), and a
    // textual physical extra — ordered by `position`.
    insert_component(db, bundle, 0, "sealed", "Play Booster", 9, Some(pack), None).await;
    insert_component(db, bundle, 1, "card", "Momo, Friendly Flier", 1, None, Some(promo)).await;
    insert_component(db, bundle, 2, "other", "Spindown life counter", 1, None, None).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/600/contents")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the product-contents read must be browser + CDN cacheable"
    );
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);

    // Ordered by position; a sealed component links its sub-product (reusing the Product wire
    // shape) and shows the child's catalog name; a card component links its card; the extra
    // is textual (both links null).
    assert_eq!(data[0]["kind"], "sealed");
    assert_eq!(data[0]["quantity"], 9);
    assert_eq!(data[0]["name"], "Play Booster Pack", "prefers the linked product's catalog name");
    assert_eq!(data[0]["product"]["id"], "640");
    assert!(data[0]["product"]["prices"].is_object());
    assert!(data[0]["card"].is_null());

    assert_eq!(data[1]["kind"], "card");
    assert_eq!(data[1]["card"]["id"], "sf-promo");
    assert_eq!(data[1]["name"], "Card sf-promo", "prefers the linked card's catalog name");
    assert!(data[1]["card"]["collector_number"].is_string());
    assert!(data[1]["product"].is_null());

    assert_eq!(data[2]["kind"], "other");
    assert_eq!(data[2]["name"], "Spindown life counter");
    assert!(data[2]["product"].is_null());
    assert!(data[2]["card"].is_null());

    // A product with no composition -> a clean, cacheable empty list.
    let (status, headers, body) = send(&app, get("/api/games/mtg/products/700/contents")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // Unknown game / product are no-store 404s.
    for uri in [
        "/api/games/nope/products/600/contents",
        "/api/games/mtg/products/999999/contents",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 404 must be no-store");
    }
}

#[tokio::test]
async fn product_containers_endpoint_lists_direct_parents_with_quantities() {
    let app = test_app().await;
    let db = &app.state.db;

    let pack = insert_product(db, "800", "Play Booster Pack", "tla", "play_pack", Some("4.99")).await;
    let box_product =
        insert_product(db, "810", "Play Booster Box", "tla", "play_display", Some("139.99")).await;
    let bundle = insert_product(db, "820", "Gift Bundle", "tla", "gift_bundle", Some("79.99")).await;

    // The box models its packs in two direct line items; the reverse endpoint collapses
    // those to one parent and sums their quantities. The bundle is a second parent.
    insert_component(db, box_product, 0, "sealed", "Play Booster Pack", 30, Some(pack), None).await;
    insert_component(db, box_product, 1, "sealed", "Box Topper Pack", 6, Some(pack), None).await;
    insert_component(db, bundle, 0, "sealed", "Play Booster Pack", 9, Some(pack), None).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/products/800/containers")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the reverse product-composition read must be browser + CDN cacheable"
    );
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    // Parent name order is stable, and each entry reuses the full Product wire shape.
    assert_eq!(data[0]["product"]["id"], "820");
    assert_eq!(data[0]["quantity"], 9);
    assert!(data[0]["product"]["prices"].is_object());
    assert_eq!(data[1]["product"]["id"], "810");
    assert_eq!(data[1]["quantity"], 36);

    // A product with no parent composition has a clean empty list.
    let (status, headers, body) = send(&app, get("/api/games/mtg/products/810/containers")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // Unknown game / product are no-store 404s, matching the forward contents endpoint.
    for uri in [
        "/api/games/nope/products/800/containers",
        "/api/games/mtg/products/999999/containers",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 404 must be no-store");
    }
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

#[tokio::test]
async fn product_cards_flags_and_orders_collector_booster_exclusives() {
    let app = test_app().await;
    let db = &app.state.db;

    // One set with a collector booster + a play booster. A card is shared by both booster
    // pools; one is only on the collector sheets (an "exclusive" special printing); one is
    // only on the play sheets. Ids ascend by insert order, but exclusivity dominates the
    // order so that's not what's under test.
    let shared = insert_card(db, "sf-shared").await;
    let collector_only = insert_card(db, "sf-collector").await;
    let play_only = insert_card(db, "sf-play").await;
    let collector = insert_product(db, "100", "Collector Booster Pack", "mkm", "collector_pack", Some("24.99")).await;
    let play = insert_product(db, "200", "Play Booster Pack", "mkm", "play_pack", Some("4.99")).await;

    for cid in [shared, collector_only] {
        insert_sealed(db, collector, cid, "booster", false).await;
    }
    for cid in [shared, play_only] {
        insert_sealed(db, play, cid, "booster", false).await;
    }

    // The collector booster: the collector-only card is flagged exclusive and leads the
    // list; the shared card is not exclusive; the play-only card isn't in this product.
    let (status, _, body) = send(&app, get("/api/games/mtg/products/100/cards")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["card"]["id"], "sf-collector", "the exclusive card leads");
    assert_eq!(data[0]["membership"], "booster");
    assert_eq!(data[0]["exclusive"], true);
    assert_eq!(data[1]["card"]["id"], "sf-shared");
    assert_eq!(data[1]["exclusive"], false, "a card in another booster family isn't exclusive");

    // Symmetrically, the play booster flags its own play-only card exclusive.
    let (_, _, body) = send(&app, get("/api/games/mtg/products/200/cards")).await;
    assert_eq!(body["total"], 2);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data[0]["card"]["id"], "sf-play");
    assert_eq!(data[0]["exclusive"], true);
    assert_eq!(data[1]["card"]["id"], "sf-shared");
    assert_eq!(data[1]["exclusive"], false);
}

#[tokio::test]
async fn product_cards_no_exclusives_without_a_comparison_family() {
    let app = test_app().await;
    let db = &app.state.db;

    // A collector-booster-only release (no play/draft/set booster to compare against, as
    // for a Universes-Beyond Commander set): "exclusive" would be vacuously true of every
    // card, which is no signal, so nothing is flagged. A same-family sibling (the collector
    // display) is also NOT a comparison pool.
    let a = insert_card(db, "sf-a").await;
    let b = insert_card(db, "sf-b").await;
    let pack = insert_product(db, "100", "Collector Booster Pack", "who", "collector_pack", Some("24.99")).await;
    let display = insert_product(db, "101", "Collector Booster Box", "who", "collector_display", Some("249.99")).await;
    let _deck = insert_product(db, "300", "Commander Deck", "who", "commander_deck", Some("44.99")).await;
    for cid in [a, b] {
        insert_sealed(db, pack, cid, "booster", false).await;
        insert_sealed(db, display, cid, "booster", false).await;
    }

    let (status, _, body) = send(&app, get("/api/games/mtg/products/100/cards")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    for entry in body["data"].as_array().unwrap() {
        assert_eq!(entry["exclusive"], false, "no exclusivity without another booster family");
    }

    // A non-booster product (a deck) never flags exclusivity either.
    insert_sealed(db, _deck, a, "contains", false).await;
    let (_, _, body) = send(&app, get("/api/games/mtg/products/300/cards")).await;
    assert_eq!(body["data"][0]["exclusive"], false);
}

#[tokio::test]
async fn product_cards_flags_bundle_contained_collector_exclusives() {
    let app = test_app().await;
    let db = &app.state.db;

    // A gift bundle whose booster pool (from the play + collector boosters it wraps) spans a
    // shared play card and a collector-only card, plus one guaranteed card. Its `sealed`
    // components link the play + collector boosters — so the split is judged against the set's
    // play booster and titled after the *contained* collector booster (issue #290).
    let shared = insert_card(db, "sf-shared").await;
    let collector_only = insert_card(db, "sf-collector").await;
    let guaranteed = insert_card(db, "sf-guar").await;
    let collector =
        insert_product(db, "100", "Collector Booster Pack", "fin", "collector_pack", Some("24.99")).await;
    let play = insert_product(db, "200", "Play Booster Pack", "fin", "play_pack", Some("4.99")).await;
    let bundle = insert_product(db, "300", "Gift Bundle", "fin", "bundle", Some("49.99")).await;

    // The bundle's inherited booster pool + its guaranteed card.
    insert_sealed(db, bundle, shared, "booster", false).await;
    insert_sealed(db, bundle, collector_only, "booster", false).await;
    insert_sealed(db, bundle, guaranteed, "contains", false).await;
    // The set's standalone play booster carries the shared card (the comparison pool); the
    // collector-only card is on no other family's booster.
    insert_sealed(db, play, shared, "booster", false).await;
    // The bundle wraps both boosters (only the collector child drives the "premium" family).
    insert_component(db, bundle, 0, "sealed", "Play Booster", 9, Some(play), None).await;
    insert_component(db, bundle, 1, "sealed", "Collector Booster", 1, Some(collector), None).await;

    // /cards: the collector-only card is flagged exclusive and leads the booster pool.
    let (status, _, body) = send(&app, get("/api/games/mtg/products/300/cards")).await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().unwrap();
    // contains (guaranteed) leads, then the exclusive booster card, then the shared one.
    assert_eq!(data[0]["card"]["id"], "sf-guar");
    assert_eq!(data[0]["membership"], "contains");
    assert_eq!(data[1]["card"]["id"], "sf-collector");
    assert_eq!(data[1]["membership"], "booster");
    assert_eq!(data[1]["exclusive"], true, "a collector-only card the bundle wraps is exclusive");
    assert_eq!(data[2]["card"]["id"], "sf-shared");
    assert_eq!(data[2]["exclusive"], false, "the shared play card is not exclusive");

    // /cards/sections: the exclusive section exists and is titled after the collector family.
    let (status, _, body) = send(&app, get("/api/games/mtg/products/300/cards/sections")).await;
    assert_eq!(status, StatusCode::OK);
    let sections: Vec<(&str, u64, Option<&str>)> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["key"].as_str().unwrap(),
                s["total"].as_u64().unwrap(),
                s["booster_family"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        sections,
        vec![
            ("contains", 1, None),
            ("exclusive", 1, Some("collector_pack")),
            ("booster", 1, None),
        ],
        "the bundle splits into contains / collector-exclusive / shared-booster, the exclusive \
         section naming the contained collector family"
    );
}

#[tokio::test]
async fn product_card_sections_bundle_premium_family_variants() {
    let app = test_app().await;
    let db = &app.state.db;

    // A Chocobo-style bundle: it wraps a play booster + a *generic* special booster (Final
    // Fantasy's Chocobo booster is a `pack`). The special-only card reads exclusive, titled
    // after the generic booster family.
    let shared = insert_card(db, "sf-shared").await;
    let special_only = insert_card(db, "sf-special").await;
    let play = insert_product(db, "200", "Play Booster Pack", "fin", "play_pack", Some("4.99")).await;
    let chocobo = insert_product(db, "210", "Chocobo Booster Pack", "fin", "pack", Some("6.99")).await;
    let chocobo_bundle = insert_product(db, "310", "Chocobo Bundle", "fin", "bundle", Some("39.99")).await;

    insert_sealed(db, chocobo_bundle, shared, "booster", false).await;
    insert_sealed(db, chocobo_bundle, special_only, "booster", false).await;
    insert_sealed(db, play, shared, "booster", false).await;
    insert_component(db, chocobo_bundle, 0, "sealed", "Play Booster", 10, Some(play), None).await;
    insert_component(db, chocobo_bundle, 1, "sealed", "Chocobo Booster", 1, Some(chocobo), None).await;

    let (_, _, body) = send(&app, get("/api/games/mtg/products/310/cards/sections")).await;
    let exclusive = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "exclusive")
        .expect("a generic-booster bundle still splits out its exclusives");
    assert_eq!(exclusive["total"], 1);
    assert_eq!(exclusive["booster_family"], "pack", "titled after the generic booster family");

    // A plain bundle wrapping only a play booster has no premium tier — no exclusive section,
    // and every booster card is just "Can be pulled from boosters".
    let plain = insert_product(db, "320", "Bloomburrow Bundle", "blb", "bundle", Some("39.99")).await;
    let other_play = insert_product(db, "220", "Play Booster Pack", "blb", "play_pack", Some("4.99")).await;
    let c1 = insert_card(db, "sf-blb-1").await;
    insert_sealed(db, plain, c1, "booster", false).await;
    insert_sealed(db, other_play, c1, "booster", false).await;
    insert_component(db, plain, 0, "sealed", "Play Booster", 8, Some(other_play), None).await;

    let (_, _, body) = send(&app, get("/api/games/mtg/products/320/cards/sections")).await;
    let keys: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["booster"], "a play-only bundle has no exclusive section");
}

/// Seed one collector booster (100) with a card in every display section, plus a play booster
/// (200) that shares the non-exclusive booster cards — so the collector-only ones read as
/// family-exclusive. Product 100 ends up with 1 guaranteed, 3 exclusive-booster, 2 shared-
/// booster, and 1 variable card (7 distinct), spanning all four display sections.
async fn seed_sectioned_collector(db: &sea_orm::DatabaseConnection) {
    let collector =
        insert_product(db, "100", "Collector Booster Pack", "mkm", "collector_pack", Some("24.99")).await;
    let play = insert_product(db, "200", "Play Booster Pack", "mkm", "play_pack", Some("4.99")).await;

    let guaranteed = insert_card(db, "sf-guar").await;
    insert_sealed(db, collector, guaranteed, "contains", false).await;

    // Collector-only booster cards (in no other family's booster) -> exclusive.
    for ext in ["sf-excl-1", "sf-excl-2", "sf-excl-3"] {
        let cid = insert_card(db, ext).await;
        insert_sealed(db, collector, cid, "booster", false).await;
    }
    // Booster cards the play booster also pulls -> shared, not exclusive.
    for ext in ["sf-shared-1", "sf-shared-2"] {
        let cid = insert_card(db, ext).await;
        insert_sealed(db, collector, cid, "booster", false).await;
        insert_sealed(db, play, cid, "booster", false).await;
    }
    let variable = insert_card(db, "sf-var").await;
    insert_sealed(db, collector, variable, "variable", false).await;
}

#[tokio::test]
async fn product_card_sections_lists_nonempty_sections_in_display_order() {
    let app = test_app().await;
    seed_sectioned_collector(&app.state.db).await;

    let (status, headers, body) =
        send(&app, get("/api/games/mtg/products/100/cards/sections")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the sections manifest must be browser + CDN cacheable"
    );
    // Non-empty sections only, in display order: contains, exclusive, booster (shared),
    // variable — each with its own card count.
    let got: Vec<(&str, u64)> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["key"].as_str().unwrap(), s["total"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        got,
        vec![("contains", 1), ("exclusive", 3), ("booster", 2), ("variable", 1)]
    );

    // A product with no ingested contents -> a clean, cacheable empty manifest (no sections).
    insert_product(&app.state.db, "900", "Empty Bundle", "mkm", "bundle", Some("9.99")).await;
    let (status, headers, body) =
        send(&app, get("/api/games/mtg/products/900/cards/sections")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // Unknown game / product are no-store 404s.
    for uri in [
        "/api/games/nope/products/100/cards/sections",
        "/api/games/mtg/products/999999/cards/sections",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 404 must be no-store");
    }
}

#[tokio::test]
async fn product_cards_section_filter_pages_within_one_section() {
    let app = test_app().await;
    seed_sectioned_collector(&app.state.db).await;

    // `?section=exclusive` pages only the 3 family-exclusive booster cards, reporting the
    // section's own total (not the product's), and every entry is a flagged booster card.
    let (status, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards?section=exclusive&page_size=2")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3, "the section total, not the whole product's");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(body["has_more"], true);
    for entry in data {
        assert_eq!(entry["membership"], "booster");
        assert_eq!(entry["exclusive"], true);
    }
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/products/100/cards?section=exclusive&page=2&page_size=2"),
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1, "the exclusive section's last page");
    assert_eq!(body["has_more"], false);

    // `?section=booster` pages only the shared (non-exclusive) booster cards.
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards?section=booster")).await;
    assert_eq!(body["total"], 2);
    for entry in body["data"].as_array().unwrap() {
        assert_eq!(entry["membership"], "booster");
        assert_eq!(entry["exclusive"], false, "the shared pool is not exclusive");
    }

    // `contains` / `variable` each surface their single card.
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards?section=contains")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["membership"], "contains");
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards?section=variable")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["membership"], "variable");

    // No `section` still returns the whole ordered list (the back-compatible default).
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards")).await;
    assert_eq!(body["total"], 7, "1 contains + 3 exclusive + 2 shared + 1 variable");

    // An unknown section is a no-store 422 (rejected before it matters).
    let (status, headers, _) =
        send(&app, get("/api/games/mtg/products/100/cards?section=nope")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn product_cards_q_filters_cards_and_sections_by_search() {
    let app = test_app().await;
    seed_sectioned_collector(&app.state.db).await;
    // Seeded card names are "Card sf-guar" / "Card sf-excl-{1..3}" / "Card sf-shared-{1,2}" /
    // "Card sf-var", so a bare `q` word matches by name substring (the Scryfall compiler,
    // reused because these rows are cards — issue #222).

    // `?q=excl` matches only the three exclusive cards: the paged read reports the *filtered*
    // total, every entry is a flagged booster card, and it stays browser + CDN cacheable.
    let (status, headers, body) = send(&app, get("/api/games/mtg/products/100/cards?q=excl")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a filtered card page is still shared-cacheable"
    );
    assert_eq!(body["total"], 3, "only the 3 cards whose name matches 'excl'");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    for entry in data {
        assert_eq!(entry["membership"], "booster");
        assert_eq!(entry["exclusive"], true);
    }

    // The manifest agrees under the same `q`: it collapses to the sections that still have
    // matches, with filtered counts and no empty sections — so the SPA renders exactly the
    // blocks the paged reads will fill.
    let (status, headers, body) =
        send(&app, get("/api/games/mtg/products/100/cards/sections?q=excl")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE));
    let got: Vec<(&str, u64)> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["key"].as_str().unwrap(), s["total"].as_u64().unwrap()))
        .collect();
    assert_eq!(got, vec![("exclusive", 3)], "only the exclusive section survives 'excl'");

    // A `q` that spans every section keeps them all, in display order.
    let (_, _, body) = send(&app, get("/api/games/mtg/products/100/cards/sections?q=sf")).await;
    let keys: Vec<&str> =
        body["data"].as_array().unwrap().iter().map(|s| s["key"].as_str().unwrap()).collect();
    assert_eq!(
        keys,
        vec!["contains", "exclusive", "booster", "variable"],
        "'sf' is in every seeded card name"
    );

    // `q` composes with `section`: the search applies on top of the section filter.
    let (_, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards?section=booster&q=shared")).await;
    assert_eq!(body["total"], 2, "the 2 shared booster cards named '…shared…'");
    for entry in body["data"].as_array().unwrap() {
        assert_eq!(entry["exclusive"], false);
    }
    // A section + q with no overlap is an empty page (the exclusive cards aren't named 'shared').
    let (_, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards?section=exclusive&q=shared")).await;
    assert_eq!(body["total"], 0);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // A `q` matching nothing is a clean, cacheable empty page + empty manifest (not an error),
    // so the SPA can keep the search box up and show a "no matches" note.
    let (status, _, body) = send(&app, get("/api/games/mtg/products/100/cards?q=zzzznope")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
    let (status, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards/sections?q=zzzznope")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);

    // A malformed query is a no-store 422 on both endpoints (rejected before the page loads),
    // exactly like the card catalog's search.
    for uri in [
        "/api/games/mtg/products/100/cards?q=boguskey:1",
        "/api/games/mtg/products/100/cards/sections?q=boguskey:1",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri} should 422");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 422 must be no-store");
    }
}

#[tokio::test]
async fn product_cards_sort_reorders_and_rejects_a_bad_sort() {
    let app = test_app().await;
    let db = &app.state.db;

    // Two guaranteed cards whose collector-number order (the default) is the reverse of their
    // name order, so a name sort visibly reorders them. `insert_card_at` names them
    // "Card zed" / "Card ace".
    let box_id = insert_product(db, "800", "Bundle", "mkm", "bundle", Some("9.99")).await;
    let zed = insert_card_at(db, "zed", "mkm", "1", Some(1)).await;
    let ace = insert_card_at(db, "ace", "mkm", "2", Some(2)).await;
    for cid in [zed, ace] {
        insert_sealed(db, box_id, cid, "contains", false).await;
    }
    let ids = |body: &serde_json::Value| -> Vec<String> {
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["card"]["id"].as_str().unwrap().to_string())
            .collect()
    };

    // Default: the product's natural set+collector order — cn 1 (zed) then cn 2 (ace).
    let (_, _, body) = send(&app, get("/api/games/mtg/products/800/cards")).await;
    assert_eq!(ids(&body), vec!["zed", "ace"]);

    // `?sort=name&dir=asc`: "Card ace" before "Card zed", overriding the collector order — and
    // a sorted page is still browser + CDN cacheable.
    let (status, headers, body) =
        send(&app, get("/api/games/mtg/products/800/cards?sort=name&dir=asc")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a sorted card page is still shared-cacheable"
    );
    assert_eq!(ids(&body), vec!["ace", "zed"]);

    // Descending name reverses it.
    let (_, _, body) =
        send(&app, get("/api/games/mtg/products/800/cards?sort=name&dir=desc")).await;
    assert_eq!(ids(&body), vec!["zed", "ace"]);

    // An unknown sort (or dir) is a no-store 422, consistent with the section filter + the
    // card lists — rejected before any card is loaded.
    for uri in [
        "/api/games/mtg/products/800/cards?sort=nonsense",
        "/api/games/mtg/products/800/cards?sort=name&dir=sideways",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri} should 422");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri} 422 must be no-store");
    }
}

#[tokio::test]
async fn product_cards_sort_reorders_within_sections_not_across_them() {
    let app = test_app().await;
    seed_sectioned_collector(&app.state.db).await;
    // Seeded names: "Card sf-guar" (contains), "Card sf-excl-{1..3}" (exclusive), "Card
    // sf-shared-{1,2}" (shared booster), "Card sf-var" (variable).
    let ids = |body: &serde_json::Value| -> Vec<String> {
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["card"]["id"].as_str().unwrap().to_string())
            .collect()
    };

    // Whole product, `?sort=name&dir=desc`: the sections still lead in display order
    // (contains → exclusive → booster → variable), and each section's own cards come out
    // name-descending — the sort re-orders *within* a section, never merges sections.
    let (status, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards?sort=name&dir=desc")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        ids(&body),
        vec![
            "sf-guar",
            "sf-excl-3",
            "sf-excl-2",
            "sf-excl-1",
            "sf-shared-2",
            "sf-shared-1",
            "sf-var",
        ]
    );

    // `sort` composes with `section`: just the exclusive section, name-descending.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/products/100/cards?section=exclusive&sort=name&dir=desc"),
    )
    .await;
    assert_eq!(body["total"], 3);
    assert_eq!(ids(&body), vec!["sf-excl-3", "sf-excl-2", "sf-excl-1"]);

    // `sort` also composes with a `q` search (the sort → group → search-filter path): the
    // search narrows to the three exclusive cards and they stay in the sorted (name-desc)
    // order, proving the search filter preserves the sorted order rather than resetting it.
    let (_, _, body) =
        send(&app, get("/api/games/mtg/products/100/cards?sort=name&dir=desc&q=excl")).await;
    assert_eq!(body["total"], 3, "only the 3 cards named '…excl…'");
    assert_eq!(ids(&body), vec!["sf-excl-3", "sf-excl-2", "sf-excl-1"]);
}
