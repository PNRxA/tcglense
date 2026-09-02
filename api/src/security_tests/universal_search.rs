//! The universal search (`GET /api/games/{game}/search`): one public, shared-cacheable read
//! answering across cards, sealed products, precons and keywords at once. Drives the real
//! router over the seeded dummy catalog, plus a few hand-inserted rows where the seed's
//! names can't tell a rule apart (every seeded name starts with "Dummy").
//!
//! What these pin: the cache posture (public catalog, `ETag`, errors `no-store`); that every
//! leg answers the **same** every-word, any-order, any-case name rule; that cards fold to one
//! row per name; that prefix matches lead each group; that `limit` clamps and `has_more` is
//! honest; and that the request can neither inject nor overflow the query builder.

use sea_orm::{ActiveModelTrait, Set};

use super::harness::*;
use crate::entities::card;
use crate::test_support::{insert_product, url_encode};

/// Insert a card with a given name (and printing), the way `insert_card` does but named:
/// the seed's names all start with "Dummy", which can't tell "starts with" from "contains".
async fn insert_named_card(app: &TestApp, external_id: &str, name: &str, set_code: &str) {
    let now = chrono::Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(name.to_string()),
        set_code: Set(set_code.to_string()),
        set_name: Set(set_code.to_uppercase()),
        collector_number: Set("1".to_string()),
        lang: Set("en".to_string()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("insert card");
}

fn names(group: &Value) -> Vec<String> {
    group["data"]
        .as_array()
        .expect("group data array")
        .iter()
        .map(|row| row["name"].as_str().expect("name").to_string())
        .collect()
}

#[tokio::test]
async fn search_is_public_and_shared_cacheable() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    let (status, headers, body) =
        send(&app, get(&format!("/api/games/{game}/search?q=dummy"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the same for every visitor, so a CDN may store it"
    );
    assert!(headers.contains_key("etag"), "carries a validator");

    // Every catalog group answers the seed; the keyword glossary has nothing called dummy.
    for group in ["cards", "products", "precons"] {
        assert!(
            !body[group]["data"].as_array().expect("array").is_empty(),
            "{group} should match the seeded catalog"
        );
    }
    assert!(
        body["keywords"]["data"]
            .as_array()
            .expect("array")
            .is_empty()
    );
    assert_eq!(body["keywords"]["has_more"], false);

    // Each group is cut at the default limit and says so.
    assert_eq!(body["cards"]["data"].as_array().unwrap().len(), 5);
    assert_eq!(body["cards"]["has_more"], true);
}

#[tokio::test]
async fn every_group_carries_its_own_listings_wire_shape() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // A card hit is the full `Card` payload (a client renders it with the tile it has).
    // Every seeded card is "Dummy <Colour> <Noun>"; only the set is called "Universe".
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=dummy"))).await;
    let card = &body["cards"]["data"][0];
    for key in [
        "id",
        "name",
        "set_code",
        "set_name",
        "has_image",
        "prices",
        "faces",
    ] {
        assert!(!card[key].is_null(), "card hit lacks `{key}`: {card}");
    }
    // A product hit is a `Product`, a precon hit a `PreconDeck` complete with its face card.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy%20universe")),
    )
    .await;
    let product = &body["products"]["data"][0];
    assert!(product["product_type"].is_string(), "{product}");
    assert!(product["set_name"].is_string(), "{product}");
    let precon = &body["precons"]["data"][0];
    assert_eq!(precon["slug"], "dummy-universe-commander-dmu");
    assert_eq!(precon["set_name"], "Dummy Universe");
    assert!(precon["face_card"]["card_id"].is_string(), "{precon}");

    // And a keyword hit is a `KeywordEntry`.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=vigilance"))).await;
    let keyword = &body["keywords"]["data"][0];
    assert_eq!(keyword["name"], "Vigilance");
    assert_eq!(keyword["slug"], "vigilance");
    assert_eq!(keyword["kind"], "ability");
}

#[tokio::test]
async fn cards_fold_to_one_row_per_name() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The dummy catalog reprints "Dummy Reprinted Relic" across two sets: one hit, not two.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=reprinted"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body["cards"]), vec!["Dummy Reprinted Relic"]);
    assert_eq!(body["cards"]["has_more"], false);
}

#[tokio::test]
async fn every_leg_matches_every_word_in_any_order_and_case() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Cards: the reprint again, words reversed and shouted.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=RELIC%20reprinted")),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Dummy Reprinted Relic"]);

    // Products + precons: the seeded "Dummy Universe Commander Deck" product and the
    // "Dummy Universe Commander" precon, found by "commander universe".
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=commander%20universe")),
    )
    .await;
    assert!(names(&body["products"]).contains(&"Dummy Universe Commander Deck".to_string()));
    assert_eq!(names(&body["precons"]), vec!["Dummy Universe Commander"]);

    // Keywords: "strike first" finds First strike.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=strike%20first")),
    )
    .await;
    assert!(names(&body["keywords"]).contains(&"First strike".to_string()));

    // A word no name carries empties every group — with no error.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy%20zzzz")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for group in ["cards", "products", "precons", "keywords"] {
        assert!(
            body[group]["data"].as_array().unwrap().is_empty(),
            "{group}"
        );
        assert_eq!(body[group]["has_more"], false, "{group}");
    }
}

#[tokio::test]
async fn prefix_matches_lead_each_group() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    // "Bolt …" starts with the text, "… Bolt" only contains it; and a name that sorts
    // first alphabetically ("Alpha Bolt") must not beat the prefix match.
    insert_named_card(&app, "c-1", "Lightning Bolt", "lea").await;
    insert_named_card(&app, "c-2", "Bolt of Lightning", "tst").await;
    insert_named_card(&app, "c-3", "Alpha Bolt", "tst").await;
    // A second printing of the prefix card: still one row.
    insert_named_card(&app, "c-4", "Bolt of Lightning", "two").await;
    insert_product(
        &app.state.db,
        "p-1",
        "Thunder Bolt Bundle",
        "tst",
        "bundle",
        None,
    )
    .await;
    insert_product(&app.state.db, "p-2", "Bolt Box", "tst", "box", None).await;

    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=bolt"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        names(&body["cards"]),
        vec!["Bolt of Lightning", "Alpha Bolt", "Lightning Bolt"],
        "prefix first, then by name"
    );
    assert_eq!(
        names(&body["products"]),
        vec!["Bolt Box", "Thunder Bolt Bundle"]
    );

    // Keywords rank the same way: "cycling" leads the landcycling family.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=cycling&limit=3")),
    )
    .await;
    assert_eq!(names(&body["keywords"])[0], "Cycling");
    assert_eq!(body["keywords"]["has_more"], true);
}

#[tokio::test]
async fn limit_is_clamped_and_has_more_is_honest() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Below the floor: one per group.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy&limit=0")),
    )
    .await;
    assert_eq!(body["cards"]["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["cards"]["has_more"], true);

    // Above the ceiling: ten.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy&limit=999")),
    )
    .await;
    assert_eq!(body["cards"]["data"].as_array().unwrap().len(), 10);
    assert_eq!(body["cards"]["has_more"], true);

    // A group with fewer matches than the limit is whole and says so.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy&limit=10")),
    )
    .await;
    let precons = body["precons"]["data"].as_array().unwrap();
    assert_eq!(precons.len(), 5, "the seed has five precons");
    assert_eq!(body["precons"]["has_more"], false);

    // A non-numeric limit is a client error, not a fallback.
    let (status, _, _) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy&limit=lots")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn blank_query_answers_empty_groups() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    for path in [
        format!("/api/games/{game}/search"),
        format!("/api/games/{game}/search?q="),
        format!("/api/games/{game}/search?q=%20%20"),
    ] {
        let (status, _, body) = send(&app, get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        for group in ["cards", "products", "precons", "keywords"] {
            assert!(
                body[group]["data"].as_array().unwrap().is_empty(),
                "{path} {group}"
            );
            assert_eq!(body[group]["has_more"], false);
        }
    }
}

#[tokio::test]
async fn unknown_game_is_a_no_store_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, _) = send(&app, get("/api/games/nope/search?q=dummy")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn search_is_injection_safe_and_a_very_long_query_is_refused() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    let (_, _, before) = send(&app, get(&format!("/api/games/{game}/cards?page_size=1"))).await;
    let seeded_total = before["total"].as_u64().expect("total");

    // A SQL payload is a harmless literal name search across every leg.
    let injection = url_encode("'; DROP TABLE cards;--");
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q={injection}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
    // LIKE metacharacters match literally too: `%` is not a wildcard.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=%25"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());

    let (_, _, after) = send(&app, get(&format!("/api/games/{game}/cards?page_size=1"))).await;
    assert_eq!(
        after["total"].as_u64(),
        Some(seeded_total),
        "the cards table is intact"
    );

    // The every-word rule's cap: 33 words is a 422, never a stack overflow (see
    // `handlers::shared::search::every_word_matches`).
    let long = url_encode(&vec!["dummy"; 33].join(" "));
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q={long}"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().is_some());
    // Exactly the cap still answers.
    let at_cap = url_encode(&vec!["dummy"; 32].join(" "));
    let (status, _, _) = send(&app, get(&format!("/api/games/{game}/search?q={at_cap}"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn documented_in_the_openapi_spec() {
    let app = test_app().await;

    let (status, _, body) = send(&app, get("/api/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    let op = &body["paths"]["/api/games/{game}/search"]["get"];
    assert!(
        op.is_object(),
        "the universal search is a public JSON read, so it is documented"
    );
    assert_eq!(op["tags"][0], "Search");
    assert!(body["components"]["schemas"]["SearchResults"].is_object());
}
