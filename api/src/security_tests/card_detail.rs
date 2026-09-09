//! The single-card route's detail payload (`GET /api/games/{game}/cards/{id}`, issue
//! #673): it answers `CardDetail` — every shared `Card` field flattened at the top level
//! *plus* the print + collector details only this route carries (artist, flavour text,
//! finishes, frame/border/stamp/promo facts, Reserved List, produced mana, a Battle's
//! defense, the EDHREC/Penny ranks) — while the **listings** keep answering the unwidened
//! `Card`. That asymmetry is the point of the issue: `Card` rides every list page (up to
//! 200 rows, CDN/ETag-cached), so a test here pins that none of the detail keys leaked
//! onto it. Drives the real router in-process, seeding card fixtures straight into the
//! harness DB.

use super::harness::*;
use crate::entities::card;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};

/// The card the detail assertions read: every detail column populated, plus enough of the
/// shared `Card` shape (prices, faces, legalities) to prove the flatten carries it all.
async fn insert_detailed_card(db: &sea_orm::DatabaseConnection) {
    let now = Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set("detail-card".to_string()),
        oracle_id: Set(Some("oracle-detail".to_string())),
        name: Set("Detailed Siege".to_string()),
        set_code: Set("tst".to_string()),
        set_name: Set("Test Set".to_string()),
        collector_number: Set("1".to_string()),
        collector_number_int: Set(Some(1)),
        rarity: Set(Some("mythic".to_string())),
        lang: Set("en".to_string()),
        released_at: Set(Some("2024-02-09".to_string())),
        mana_cost: Set(Some("{2}{W}".to_string())),
        cmc: Set(Some(3.0)),
        type_line: Set(Some("Battle — Siege".to_string())),
        layout: Set(Some("battle".to_string())),
        color_identity: Set(Some("W".to_string())),
        colors: Set(Some("W".to_string())),
        price_usd: Set(Some("1.50".to_string())),
        price_usd_foil: Set(Some("9.99".to_string())),
        price_usd_etched: Set(Some("14.50".to_string())),
        price_eur: Set(Some("1.20".to_string())),
        price_tix: Set(Some("0.03".to_string())),
        legalities: Set(Some(
            r#"{"modern":"legal","commander":"banned"}"#.to_string(),
        )),
        card_faces: Set(Some(
            r#"[{"name":"Detailed Siege","mana_cost":"{2}{W}","type_line":"Battle — Siege",
                 "image_small":null,"image_normal":null,"image_large":null,"image_png":null,
                 "image_art_crop":null},
                {"name":"Detailed Aftermath","mana_cost":null,"type_line":"Creature — Angel",
                 "image_small":null,"image_normal":null,"image_large":null,"image_png":null,
                 "image_art_crop":null}]"#
                .to_string(),
        )),
        // --- The detail-only columns. ---
        artist: Set(Some("Rebecca Guay".to_string())),
        artist_ids: Set(Some("artist-a,artist-b".to_string())),
        illustration_id: Set(Some("illustration-1".to_string())),
        flavor_text: Set(Some("The siege breaks at dawn.".to_string())),
        watermark: Set(Some("orzhov".to_string())),
        finishes: Set(Some("nonfoil,foil".to_string())),
        frame: Set(Some("2015".to_string())),
        frame_effects: Set(Some("showcase,legendary".to_string())),
        border_color: Set(Some("borderless".to_string())),
        security_stamp: Set(Some("oval".to_string())),
        promo_types: Set(Some("prerelease,buyabox".to_string())),
        produced_mana: Set(Some("W,B".to_string())),
        defense: Set(Some("5".to_string())),
        reserved: Set(Some(true)),
        full_art: Set(Some(true)),
        textless: Set(Some(false)),
        promo: Set(Some(true)),
        variation: Set(Some(false)),
        story_spotlight: Set(Some(true)),
        content_warning: Set(None),
        edhrec_rank: Set(Some(1234)),
        penny_rank: Set(Some(567)),
        // --- The external ids (issue #686); `cardmarket_id` left NULL on purpose. ---
        tcgplayer_id: Set(Some(179421)),
        tcgplayer_etched_id: Set(Some(250123)),
        multiverse_ids: Set(Some("450221,450222".to_string())),
        mtgo_id: Set(Some(68968)),
        mtgo_foil_id: Set(Some(68969)),
        arena_id: Set(Some(67890)),
        cardmarket_id: Set(None),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert detailed card");
}

/// A card with every detail column NULL — the shape of a row the sync hasn't rewritten,
/// and of a printing that genuinely carries none of these facts.
async fn insert_bare_card(db: &sea_orm::DatabaseConnection, external_id: &str, oracle_id: &str) {
    let now = Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        oracle_id: Set(Some(oracle_id.to_string())),
        name: Set("Detailed Siege".to_string()),
        set_code: Set("tsu".to_string()),
        set_name: Set("Test Set Two".to_string()),
        collector_number: Set("2".to_string()),
        collector_number_int: Set(Some(2)),
        lang: Set("en".to_string()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert bare card");
}

/// A printing whose foil is a separate `…★` object folded onto it: the base's stored
/// `finishes` is `nonfoil` exactly (the pairing rule) and its foil price is the star's.
async fn insert_folded_pair(db: &sea_orm::DatabaseConnection) {
    let now = Utc::now();
    let base = card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set("folded-base".to_string()),
        oracle_id: Set(Some("oracle-folded".to_string())),
        name: Set("Folded Relic".to_string()),
        set_code: Set("sld".to_string()),
        set_name: Set("Secret Lair Drop".to_string()),
        collector_number: Set("1587".to_string()),
        collector_number_int: Set(Some(1587)),
        lang: Set("en".to_string()),
        finishes: Set(Some("nonfoil".to_string())),
        promo_types: Set(Some("boosterfun".to_string())),
        price_usd: Set(Some("10.00".to_string())),
        // Copied on from the star by `enrich_foil_variant_prices`.
        price_usd_foil: Set(Some("25.00".to_string())),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert folded base");
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set("folded-star".to_string()),
        oracle_id: Set(Some("oracle-folded".to_string())),
        name: Set("Folded Relic".to_string()),
        set_code: Set("sld".to_string()),
        set_name: Set("Secret Lair Drop".to_string()),
        collector_number: Set("1587★".to_string()),
        collector_number_int: Set(Some(1587)),
        lang: Set("en".to_string()),
        finishes: Set(Some("foil".to_string())),
        promo_types: Set(Some("boosterfun,rainbowfoil".to_string())),
        price_usd_foil: Set(Some("25.00".to_string())),
        folded_onto_id: Set(Some(base.id)),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert folded star");
}

#[tokio::test]
async fn a_folded_foil_variant_lends_its_finish_and_treatment_to_its_base() {
    let app = test_app().await;
    insert_folded_pair(&app.state.db).await;

    // The base's page is the only page the folded star has, and it already shows the
    // star's foil price — so its finishes must say foil too, and carry the star's
    // foil-treatment tag, without the stored columns being rewritten.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards/folded-base")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["prices"]["usd_foil"], "25.00");
    assert_eq!(body["finishes"], serde_json::json!(["nonfoil", "foil"]));
    assert_eq!(
        body["promo_types"],
        serde_json::json!(["boosterfun", "rainbowfoil"])
    );

    // The star itself still answers its own columns by id (the fold is a listing
    // presentation, not a deletion), and it lends nothing to itself.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards/folded-star")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["finishes"], serde_json::json!(["foil"]));
    assert_eq!(
        body["promo_types"],
        serde_json::json!(["boosterfun", "rainbowfoil"])
    );
}

#[tokio::test]
async fn card_detail_flattens_the_shared_card_and_adds_the_print_details() {
    let app = test_app().await;
    insert_detailed_card(&app.state.db).await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/detail-card")).await;
    assert_eq!(status, StatusCode::OK);
    // Same public catalog cache policy as before the detail fields arrived: the response
    // is still a pure function of its URL.
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the card detail must stay browser + CDN cacheable"
    );

    // --- Every `Card` field is flattened at the top level, so a client typed against
    //     `Card` keeps working unchanged. ---
    assert_eq!(body["id"], "detail-card");
    assert_eq!(body["name"], "Detailed Siege");
    assert_eq!(body["set_code"], "tst");
    assert_eq!(body["set_name"], "Test Set");
    assert_eq!(body["collector_number"], "1");
    assert_eq!(body["rarity"], "mythic");
    assert_eq!(body["layout"], "battle");
    assert_eq!(body["color_identity"], json!(["W"]));
    assert_eq!(body["prices"]["usd"], "1.50");
    assert_eq!(body["prices"]["usd_foil"], "9.99");
    // The etched-foil price rides the shared `prices` object (issue #676) — the one wire
    // surface a card's third finish is priced on.
    assert_eq!(body["prices"]["usd_etched"], "14.50");
    assert_eq!(body["prices"]["eur"], "1.20");
    assert_eq!(body["prices"]["tix"], "0.03");
    assert_eq!(body["has_image"], false);
    let faces = body["faces"].as_array().expect("faces is an array");
    assert_eq!(faces.len(), 2);
    assert_eq!(faces[0]["name"], "Detailed Siege");
    assert_eq!(faces[1]["name"], "Detailed Aftermath");
    assert_eq!(body["legalities"]["modern"], "legal");
    assert_eq!(body["legalities"]["commander"], "banned");
    // The flatten is a flatten, not a nested object.
    assert!(
        body.get("card").is_none(),
        "the shared Card must be flattened, not nested under a `card` key: {body:?}"
    );

    // --- The detail-only fields. ---
    assert_eq!(body["artist"], "Rebecca Guay");
    assert_eq!(body["artist_ids"], json!(["artist-a", "artist-b"]));
    assert_eq!(body["illustration_id"], "illustration-1");
    assert_eq!(body["flavor_text"], "The siege breaks at dawn.");
    assert_eq!(body["watermark"], "orzhov");
    // The comma-joined columns come back split into arrays.
    assert_eq!(body["finishes"], json!(["nonfoil", "foil"]));
    assert_eq!(body["frame"], "2015");
    assert_eq!(body["frame_effects"], json!(["showcase", "legendary"]));
    assert_eq!(body["border_color"], "borderless");
    assert_eq!(body["security_stamp"], "oval");
    assert_eq!(body["promo_types"], json!(["prerelease", "buyabox"]));
    assert_eq!(body["produced_mana"], json!(["W", "B"]));
    // The printed defense box, exactly as stored (a string, like power/toughness).
    assert_eq!(body["defense"], "5");
    assert_eq!(body["reserved"], true);
    assert_eq!(body["full_art"], true);
    assert_eq!(body["textless"], false);
    assert_eq!(body["promo"], true);
    assert_eq!(body["variation"], false);
    assert_eq!(body["story_spotlight"], true);
    // A NULL provider boolean reads as `false`, never `null`.
    assert_eq!(body["content_warning"], false);
    // Ranks are numbers, not strings.
    assert_eq!(body["edhrec_rank"], 1234);
    assert_eq!(body["penny_rank"], 567);
    // The external ids (issue #686): numbers as stored, Gatherer's as a list, and a
    // provider with no mapping is `null` — a client shows a link only where there is one.
    assert_eq!(body["tcgplayer_id"], 179421);
    assert_eq!(body["tcgplayer_etched_id"], 250123);
    assert_eq!(body["multiverse_ids"], json!([450221, 450222]));
    assert_eq!(body["mtgo_id"], 68968);
    assert_eq!(body["mtgo_foil_id"], 68969);
    assert_eq!(body["arena_id"], 67890);
    assert!(body["cardmarket_id"].is_null());
}

#[tokio::test]
async fn a_card_with_no_print_details_answers_nulls_empty_arrays_and_false_flags() {
    let app = test_app().await;
    insert_bare_card(&app.state.db, "bare-card", "oracle-bare").await;

    let (status, _, body) = send(&app, get("/api/games/mtg/cards/bare-card")).await;
    assert_eq!(status, StatusCode::OK);

    for key in ["artist", "illustration_id", "flavor_text", "watermark"] {
        assert!(body[key].is_null(), "{key} should be null: {body:?}");
    }
    for key in ["frame", "border_color", "security_stamp", "defense"] {
        assert!(body[key].is_null(), "{key} should be null: {body:?}");
    }
    // A NULL comma-joined column is an empty array, never null — a client can always
    // iterate it.
    for key in [
        "artist_ids",
        "finishes",
        "frame_effects",
        "promo_types",
        "produced_mana",
    ] {
        assert_eq!(body[key], json!([]), "{key} should be []: {body:?}");
    }
    for key in [
        "reserved",
        "full_art",
        "textless",
        "promo",
        "variation",
        "story_spotlight",
        "content_warning",
    ] {
        assert_eq!(body[key], false, "{key} should be false: {body:?}");
    }
    assert!(body["edhrec_rank"].is_null());
    assert!(body["penny_rank"].is_null());
    for key in [
        "tcgplayer_id",
        "tcgplayer_etched_id",
        "cardmarket_id",
        "mtgo_id",
        "mtgo_foil_id",
        "arena_id",
    ] {
        assert!(body[key].is_null(), "{key} should be null: {body:?}");
    }
    assert_eq!(body["multiverse_ids"], json!([]));
}

#[tokio::test]
async fn the_listings_still_answer_the_unwidened_card_shape() {
    let app = test_app().await;
    insert_detailed_card(&app.state.db).await;
    // A sibling printing sharing the detailed card's oracle id, so `/prints` has a row.
    insert_bare_card(&app.state.db, "sibling-print", "oracle-detail").await;

    // Every detail-only key must be absent from a listing row: `Card` rides every list
    // page (up to 200 rows, CDN-cached), which is why these columns live on the detail
    // route alone (issue #673).
    let detail_only = [
        "artist",
        "artist_ids",
        "illustration_id",
        "flavor_text",
        "watermark",
        "finishes",
        "frame",
        "frame_effects",
        "border_color",
        "security_stamp",
        "promo_types",
        "produced_mana",
        "defense",
        "reserved",
        "full_art",
        "textless",
        "promo",
        "variation",
        "story_spotlight",
        "content_warning",
        "edhrec_rank",
        "penny_rank",
        "tcgplayer_id",
        "tcgplayer_etched_id",
        "cardmarket_id",
        "multiverse_ids",
        "mtgo_id",
        "mtgo_foil_id",
        "arena_id",
    ];

    // The card listing.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards")).await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["data"].as_array().expect("a page of cards");
    assert_eq!(rows.len(), 2, "both seeded cards list");
    for row in rows {
        // It is the shared `Card`: the common keys are there...
        assert!(row["id"].is_string(), "a listing row carries `id`");
        assert!(row["prices"].is_object(), "a listing row carries `prices`");
        // ...and none of the detail-only ones are.
        for key in detail_only {
            assert!(
                row.get(key).is_none(),
                "the shared Card DTO must not be widened with `{key}`: {row:?}"
            );
        }
    }

    // The other-printings list is the same shared shape.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards/detail-card/prints")).await;
    assert_eq!(status, StatusCode::OK);
    let prints = body["data"].as_array().expect("the other printings");
    assert_eq!(prints.len(), 1, "the sibling printing, not the card itself");
    assert_eq!(prints[0]["id"], "sibling-print");
    for key in detail_only {
        assert!(
            prints[0].get(key).is_none(),
            "a /prints row must not carry `{key}`: {:?}",
            prints[0]
        );
    }
}

#[tokio::test]
async fn unknown_game_and_card_details_are_no_store_404s() {
    let app = test_app().await;
    insert_detailed_card(&app.state.db).await;

    for uri in [
        "/api/games/nope/cards/detail-card",
        "/api/games/mtg/cards/missing",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "{uri} 404 must be no-store"
        );
    }
}
