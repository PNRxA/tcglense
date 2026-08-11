//! The preconstructed-deck browser: the `/api/games/{game}/precons*` reads are public,
//! shared-cacheable and filter correctly, and copying one into your own decks is an
//! authenticated write that produces a deck the rest of the app understands.
//!
//! Drives the real router over the seeded dummy catalog (which seeds three precons — a
//! Commander deck, a Secret Lair drop, and a starter deck with a sideboard), so the reads
//! answer in the real wire shapes and the copy lands real cards.

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// The seeded Commander precon — the one with a command zone and a sealed product.
const COMMANDER_SLUG: &str = "dummy-universe-commander-dmu";
/// The seeded starter deck — the one with a sideboard.
const STARTER_SLUG: &str = "dummy-base-set-starter-dmb";

#[tokio::test]
async fn precon_list_is_publicly_readable_and_shared_cacheable() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/precons")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a precon is public catalog data, so its list is browser + CDN cacheable"
    );
    assert_eq!(body["total"], 3);
    let data = body["data"].as_array().expect("data array");

    // Newest first: the 2024 sets lead the 2019 Secret Lair.
    let released: Vec<&str> = data
        .iter()
        .map(|d| d["released_at"].as_str().unwrap_or(""))
        .collect();
    let mut sorted = released.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(released, sorted, "the default order is newest first");

    // The tile carries everything it needs without a second request.
    let commander = data
        .iter()
        .find(|d| d["slug"] == COMMANDER_SLUG)
        .expect("the commander precon");
    assert_eq!(commander["deck_type"], "Commander Deck");
    assert_eq!(commander["set_code"], "dmu");
    assert_eq!(commander["set_name"], "Dummy Universe");
    assert!(commander["card_count"].as_i64().unwrap() > 0);
    assert!(
        commander["face_card"]["card_id"].as_str().is_some(),
        "the tile's face card rides the list: {commander:?}"
    );
    assert!(
        commander["color_identity"].is_array(),
        "colours are letters, folded at ingest"
    );
}

#[tokio::test]
async fn precon_list_filters_by_set_type_and_name() {
    let app = test_app_with_catalog().await;

    let (_, _, body) = send(&app, get("/api/games/mtg/precons?set=sld")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["deck_type"], "Secret Lair Drop");

    let (_, _, body) = send(&app, get("/api/games/mtg/precons?type=Commander%20Deck")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["slug"], COMMANDER_SLUG);

    // Every word must match, in any order and any case — the sealed list's rule.
    let (_, _, body) = send(&app, get("/api/games/mtg/precons?q=commander%20dummy")).await;
    assert_eq!(body["total"], 1);
    let (_, _, body) = send(&app, get("/api/games/mtg/precons?q=nothing%20here")).await;
    assert_eq!(body["total"], 0);

    // An unknown set/type filters to nothing rather than erroring.
    let (status, _, body) = send(&app, get("/api/games/mtg/precons?set=zzz")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn precon_facets_publish_the_filter_vocabulary() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/precons/facets")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    assert_eq!(body["data"]["total"], 3);
    let types: Vec<&str> = body["data"]["types"]
        .as_array()
        .expect("types")
        .iter()
        .map(|t| t["type"].as_str().expect("type"))
        .collect();
    assert!(types.contains(&"Commander Deck"));
    assert!(types.contains(&"Secret Lair Drop"));
    let sets = body["data"]["sets"].as_array().expect("sets");
    assert_eq!(sets.len(), 3);
    assert!(
        sets.iter()
            .any(|s| s["code"] == "dmu" && s["name"] == "Dummy Universe"),
        "the set filter resolves catalog names: {sets:?}"
    );
    // `facets` is a static segment, so it must never be read as a precon slug.
    assert!(body["data"]["types"].is_array());
}

#[tokio::test]
async fn precon_detail_returns_boards_summary_and_product() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    assert_eq!(body["slug"], COMMANDER_SLUG);
    assert_eq!(body["name"], "Dummy Universe Commander");

    // The command zone leads the card list, whatever the board names sort like.
    let cards = body["cards"].as_array().expect("cards");
    assert!(!cards.is_empty());
    assert_eq!(cards[0]["board"], "commander");
    assert!(cards[0]["foil"].as_bool().unwrap(), "a foil commander");
    // Cards carry the full catalog payload, so the page reuses the card grid.
    assert!(cards[0]["card"]["id"].as_str().is_some());
    assert!(cards[0]["card"]["set_code"].as_str().is_some());

    // The value summary is the deck proper and matches the header's own count.
    assert_eq!(body["summary"]["total_cards"], body["card_count"]);
    assert_eq!(body["sideboard_summary"]["total_cards"], 0);
    // The sealed product it ships in rides along, for the price + buy link.
    assert_eq!(body["product"]["name"], "Dummy Universe Commander Deck");
}

#[tokio::test]
async fn a_sideboard_is_counted_and_valued_apart_from_the_deck() {
    let app = test_app_with_catalog().await;

    let (status, _, body) =
        send(&app, get(&format!("/api/games/mtg/precons/{STARTER_SLUG}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["sideboard_count"].as_i64().unwrap() > 0);
    assert_eq!(
        body["sideboard_summary"]["total_cards"], body["sideboard_count"],
        "the sideboard is summarised on its own"
    );
    assert_eq!(
        body["summary"]["total_cards"], body["card_count"],
        "and never folded into the deck's own totals"
    );
    // No sealed product links to this one: absent, not an error.
    assert!(body["product"].is_null());
}

#[tokio::test]
async fn unknown_precon_and_game_are_no_store_404s() {
    let app = test_app_with_catalog().await;

    for uri in [
        "/api/games/mtg/precons/does-not-exist",
        "/api/games/nope/precons",
        "/api/games/nope/precons/facets",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "a 404 must never be pinned in a shared cache: {uri}"
        );
    }
}

#[tokio::test]
async fn copying_a_precon_requires_authentication() {
    let app = test_app_with_catalog().await;

    let (status, headers, _) = send(
        &app,
        json_post(
            &format!("/api/decks/mtg/precons/{COMMANDER_SLUG}/copy"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn copying_a_precon_creates_a_deck_the_rules_understand() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "precon-copier@example.com", PW).await;

    let (status, headers, deck) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/decks/mtg/precons/{COMMANDER_SLUG}/copy"),
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "copy failed: {deck:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(deck["name"], "Dummy Universe Commander");
    assert_eq!(deck["format"], "commander", "the type states the format");
    assert_eq!(deck["is_public"], false, "a copy starts private");
    assert!(deck["folder_id"].is_null());

    // The command zone landed in the section the legality rules read, and the mainboard
    // was filed into the preset type buckets rather than one pile.
    let sections = deck["sections"].as_array().expect("sections");
    let names: Vec<&str> = sections
        .iter()
        .map(|s| s["name"].as_str().expect("name"))
        .collect();
    assert!(names.contains(&"Commander"), "sections: {names:?}");
    assert!(names.len() > 1, "the mainboard was categorised: {names:?}");
    assert!(
        sections.iter().all(|s| s["is_maybeboard"] == false),
        "nothing a precon ships is a maybeboard"
    );
    let commander_section = sections
        .iter()
        .find(|s| s["name"] == "Commander")
        .expect("a Commander section")["id"]
        .as_i64()
        .expect("section id");
    let cards = deck["cards"].as_array().expect("cards");
    let in_zone: Vec<&Value> = cards
        .iter()
        .filter(|c| c["section_id"] == commander_section)
        .collect();
    assert_eq!(in_zone.len(), 1, "exactly the precon's commander");
    assert_eq!(
        in_zone[0]["foil_quantity"], 1,
        "a foil precon row copies as a foil deck card"
    );
    assert_eq!(in_zone[0]["quantity"], 0);

    // …and the copy is a normal deck: it lists, and the deck analysis reads it.
    let deck_id = deck["id"].as_i64().expect("deck id");
    let (status, _, list) = send(&app, get_with_bearer("/api/decks/mtg", &access)).await;
    assert_eq!(status, StatusCode::OK);
    let listed = list["data"].as_array().expect("decks");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], deck_id);
    assert!(
        !listed[0]["commanders"]
            .as_array()
            .expect("commanders")
            .is_empty(),
        "the deck list reads the copied command zone: {listed:?}"
    );

    let (status, _, legality) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "legality failed: {legality:?}");
    assert_eq!(
        legality["data"]["format_key"], "commander",
        "the copy is judged against the format its type stated"
    );
    // The command-zone rule reads a deck's *section names*, so a copy that filed its
    // commander anywhere but `Commander` would come back breaching this one.
    assert!(
        legality["data"]["violations"]
            .as_array()
            .expect("violations")
            .iter()
            .all(|v| v["rule"] != "command-zone"),
        "the copied command zone satisfies the rule that reads section names: {legality:?}"
    );
}

#[tokio::test]
async fn copying_an_unknown_precon_is_a_404() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "precon-missing@example.com", PW).await;

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/precons/does-not-exist/copy",
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_read_only_api_key_cannot_copy_a_precon() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "precon-readonly@example.com", PW).await;
    let (status, _, key) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            &access,
            json!({ "name": "ro", "scope": "read" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {key:?}");
    let key = key["key"].as_str().expect("plaintext key");

    // The reads are public, so the key can browse…
    let (status, _, _) = send(&app, get("/api/games/mtg/precons")).await;
    assert_eq!(status, StatusCode::OK);

    // …but copying is a write: a read-only key is 403, not 401.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/decks/mtg/precons/{COMMANDER_SLUG}/copy"),
            key,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
