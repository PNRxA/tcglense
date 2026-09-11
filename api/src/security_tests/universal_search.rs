//! The universal search (`GET /api/games/{game}/search`): one public, shared-cacheable read
//! answering across cards, sealed products, precons and keywords at once. Drives the real
//! router over the seeded dummy catalog, plus a few hand-inserted rows where the seed's
//! names can't tell a rule apart (every seeded name starts with "Dummy").
//!
//! What these pin: the cache posture (public catalog, `ETag`, errors `no-store`); that every
//! leg answers the **same** every-word, any-order, any-case name rule; that cards fold to one
//! row per name; that a word the card name lacks may name the printing's set instead — and
//! picks that set's printing — while a bare set name never answers cards (issue #709); that
//! sets answer by name or exact code, dressed as the set list dresses them; that prefix
//! matches lead each group; that `limit` clamps and `has_more` is honest; and that the
//! request can neither inject nor overflow the query builder.

use sea_orm::{ActiveModelTrait, Set};

use super::harness::*;
use crate::entities::{card, card_set};
use crate::test_support::{insert_product, url_encode};

/// Insert a set row, so a card's `set_code` resolves to a name the search can match.
async fn insert_named_set(app: &TestApp, code: &str, name: &str, released_at: Option<&str>) {
    let now = chrono::Utc::now();
    card_set::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        code: Set(code.to_string()),
        name: Set(name.to_string()),
        set_type: Set(Some("expansion".to_string())),
        released_at: Set(released_at.map(str::to_string)),
        card_count: Set(0),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("insert set");
}

/// Insert a card with a given name (and printing), the way `insert_card` does but named:
/// the seed's names all start with "Dummy", which can't tell "starts with" from "contains".
async fn insert_named_card(app: &TestApp, external_id: &str, name: &str, set_code: &str) {
    insert_printing(app, external_id, name, set_code, "1").await;
}

/// [`insert_named_card`] with a collector number, for the set-and-number rule.
async fn insert_printing(
    app: &TestApp,
    external_id: &str,
    name: &str,
    set_code: &str,
    collector_number: &str,
) {
    let now = chrono::Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(name.to_string()),
        set_code: Set(set_code.to_string()),
        set_name: Set(set_code.to_uppercase()),
        collector_number: Set(collector_number.to_string()),
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
    for group in ["cards", "sets", "products", "precons"] {
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
    // A set hit is a `CardSet`, dressed like a set-list tile: the derived `has_subtypes`
    // gate and the folded `card_count` ride along, not the bare row.
    let set = &body["sets"]["data"][0];
    assert_eq!(set["code"], "dmu");
    assert_eq!(set["name"], "Dummy Universe");
    for key in ["card_count", "has_drops", "has_subtypes", "released_at"] {
        assert!(!set[key].is_null(), "set hit lacks `{key}`: {set}");
    }
    let (_, _, listed) = send(&app, get(&format!("/api/games/{game}/sets"))).await;
    let tile = listed["data"]
        .as_array()
        .expect("sets")
        .iter()
        .find(|row| row["code"] == "dmu")
        .expect("dmu tile");
    assert_eq!(
        set, tile,
        "the search dresses a set exactly as the list does"
    );

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
async fn a_word_the_card_name_lacks_may_name_its_set_and_picks_that_printing() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    insert_named_set(&app, "lea", "Limited Edition Alpha", Some("1993-08-05")).await;
    insert_named_set(&app, "m10", "Magic 2010", Some("2009-07-17")).await;
    insert_named_set(&app, "cmr", "Commander Legends", Some("2020-11-20")).await;
    insert_named_card(&app, "bolt-lea", "Lightning Bolt", "lea").await;
    insert_named_card(&app, "bolt-m10", "Lightning Bolt", "m10").await;
    insert_named_card(&app, "ring-cmr", "Sol Ring", "cmr").await;
    insert_named_card(&app, "ring-lea", "Sol Ring", "lea").await;

    // A trailing set-name word narrows the fold to that set's printing.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/search?q=lightning%20bolt%20alpha"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body["cards"]), vec!["Lightning Bolt"]);
    assert_eq!(body["cards"]["data"][0]["set_code"], "lea");

    // So does a set code — whole and case-insensitively — wherever it sits in the term.
    let (_, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/search?q=M10%20lightning%20bolt"
        )),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Lightning Bolt"]);
    assert_eq!(body["cards"]["data"][0]["set_code"], "m10");

    // Part of a set name is enough ("legends" → Commander Legends).
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=sol%20ring%20legends")),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Sol Ring"]);
    assert_eq!(body["cards"]["data"][0]["set_code"], "cmr");

    // A set the card was never printed in is still a miss — the word must name *its* set.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=sol%20ring%20m10")),
    )
    .await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());

    // A bare set name answers no cards: it is the sets group's question. Without the
    // "at least one word in the name" rule this would list arbitrary Alpha cards.
    let (_, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/search?q=limited%20edition%20alpha"
        )),
    )
    .await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
    assert_eq!(names(&body["sets"]), vec!["Limited Edition Alpha"]);
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=cmr"))).await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
    assert_eq!(names(&body["sets"]), vec!["Commander Legends"]);
}

#[tokio::test]
async fn a_set_word_and_a_collector_number_identify_a_printing_like_a_name_does() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    insert_named_set(&app, "cmr", "Commander Legends", Some("2020-11-20")).await;
    insert_named_set(&app, "lea", "Limited Edition Alpha", Some("1993-08-05")).await;
    insert_printing(&app, "ring-cmr", "Sol Ring", "cmr", "129").await;
    insert_printing(&app, "ring-lea", "Sol Ring", "lea", "270").await;
    insert_printing(&app, "bolt-lea", "Lightning Bolt", "lea", "161").await;
    insert_printing(&app, "other-cmr", "Arcane Signet", "cmr", "161").await;
    insert_printing(&app, "lettered-cmr", "Jeska's Will", "cmr", "12A").await;

    // Set code + number: the printing, with no name word at all — as a checklist spells it.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=cmr%20129"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body["cards"]), vec!["Sol Ring"]);
    assert_eq!(body["cards"]["data"][0]["set_code"], "cmr");
    assert_eq!(body["cards"]["data"][0]["collector_number"], "129");

    // The number is scoped to the named set: #161 in Alpha is the Bolt, not the Signet.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=161%20alpha")),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Lightning Bolt"]);
    // Case-insensitive, like everything else the box matches.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=CMR%2012a"))).await;
    assert_eq!(names(&body["cards"]), vec!["Jeska's Will"]);

    // A name may ride along, and must still agree with the printing.
    let (_, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/search?q=sol%20ring%20cmr%20129"
        )),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Sol Ring"]);
    let (_, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/search?q=sol%20ring%20cmr%20161"
        )),
    )
    .await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());

    // A number alone names nothing: every set has a #129.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=129"))).await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
    // Nor does a number in a set the card isn't in.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=lea%20129"))).await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn name_matches_lead_the_rows_a_set_word_let_in() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    // "ring" is a substring of a set name, so `sol ring` also admits a Lord of the Rings
    // card with "sol" in its name. It may join the list — after every name match.
    insert_named_set(
        &app,
        "ltr",
        "The Lord of the Rings: Tales of Middle-earth",
        Some("2023-06-23"),
    )
    .await;
    insert_named_set(&app, "cmr", "Commander Legends", Some("2020-11-20")).await;
    insert_named_card(&app, "soldier-ltr", "Aragorn's Soldier", "ltr").await;
    insert_named_card(&app, "parasol-cmr", "Parasol Ring", "cmr").await;
    insert_named_card(&app, "ring-cmr", "Sol Ring", "cmr").await;

    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=sol%20ring"))).await;
    assert_eq!(
        names(&body["cards"]),
        vec!["Sol Ring", "Parasol Ring", "Aragorn's Soldier"],
        "every name match first, then what the set word admitted"
    );
    // A two-letter word names no set by substring, so it can only be matched by name.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=sol%20of"))).await;
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_set_word_does_not_cost_the_typed_card_its_lead() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    insert_named_set(&app, "cmr", "Commander Legends", Some("2020-11-20")).await;
    insert_named_card(&app, "ring-cmr", "Sol Ring", "cmr").await;
    insert_named_card(&app, "parasol-cmr", "Parasol Ring", "cmr").await;
    insert_named_card(&app, "solemn-cmr", "Solemn Simulacrum", "cmr").await;

    // "Sol Ring" starts with the whole text: the plain prefix tier, as ever.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=sol%20ring"))).await;
    assert_eq!(names(&body["cards"]), vec!["Sol Ring", "Parasol Ring"]);

    // With a set word appended nothing starts with the whole text, so the rank falls back
    // to how many *leading* words the name starts with: "Sol Ring" (two) still leads
    // "Parasol Ring" (none), which the alphabet alone would have put first.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=sol%20ring%20legends")),
    )
    .await;
    assert_eq!(names(&body["cards"]), vec!["Sol Ring", "Parasol Ring"]);

    // One leading word beats none: "Solemn Simulacrum" (starts with "sol") ranks above
    // "Parasol Ring" once "sol r cmr" no longer prefixes anything whole.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=sol%20r%20cmr")),
    )
    .await;
    assert_eq!(
        names(&body["cards"]),
        vec!["Sol Ring", "Solemn Simulacrum", "Parasol Ring"]
    );
}

#[tokio::test]
async fn sets_answer_by_name_or_exact_code_newest_first_after_prefix() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    insert_named_set(&app, "cmr", "Commander Legends", Some("2020-11-20")).await;
    insert_named_set(
        &app,
        "clb",
        "Commander Legends: Battle for Baldur's Gate",
        Some("2022-06-10"),
    )
    .await;
    insert_named_set(&app, "leg", "Legends", Some("1994-06-01")).await;
    insert_named_set(&app, "tcmr", "Commander Legends Tokens", None).await;

    // Every word, any order, any case, against the name — prefix matches first, and within
    // a tier newest first (the set list's own order), NULL dates last.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=LEGENDS%20commander")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        names(&body["sets"]),
        vec![
            "Commander Legends: Battle for Baldur's Gate",
            "Commander Legends",
            "Commander Legends Tokens",
        ]
    );
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=legends"))).await;
    assert_eq!(names(&body["sets"])[0], "Legends", "prefix first");
    assert_eq!(body["sets"]["has_more"], false);

    // The whole term as a code — case-insensitively — and never a substring of one: `cm`
    // names no set, and `cmr` names exactly the one whose code it is.
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=CMR"))).await;
    assert_eq!(names(&body["sets"]), vec!["Commander Legends"]);
    let (_, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=cm"))).await;
    assert!(body["sets"]["data"].as_array().unwrap().is_empty());

    // Cut at the limit with an honest `has_more`.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=legends&limit=2")),
    )
    .await;
    assert_eq!(body["sets"]["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["sets"]["has_more"], true);
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

    // Sets: "universe dummy" finds Dummy Universe.
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=universe%20DUMMY")),
    )
    .await;
    assert_eq!(names(&body["sets"]), vec!["Dummy Universe"]);

    // A word no name carries empties every group — with no error.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy%20zzzz")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for group in ["cards", "sets", "products", "precons", "keywords"] {
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

    // Cut below the five seeded precons — at the SPA's own group limit — so the precon leg's
    // over-fetch is pinned too: the group says there is more, which is what lets the box
    // offer "All preconstructed decks matching …".
    let (_, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy&limit=4")),
    )
    .await;
    assert_eq!(body["precons"]["data"].as_array().unwrap().len(), 4);
    assert_eq!(body["precons"]["has_more"], true);

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
        for group in ["cards", "sets", "products", "precons", "keywords"] {
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
    assert!(body["sets"]["data"].as_array().unwrap().is_empty());
    // LIKE metacharacters match literally too: `%` is not a wildcard — in any leg, including
    // the set half of the card leg (resolved in Rust, where `%` is just a character).
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/search?q=%25"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cards"]["data"].as_array().unwrap().is_empty());
    assert!(body["sets"]["data"].as_array().unwrap().is_empty());
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/search?q=dummy%20%25")),
    )
    .await;
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
