//! Public search route — injection-safe, malformed -> 422 (not 500).

use super::harness::*;
use crate::test_support::url_encode;

#[tokio::test]
async fn search_is_injection_safe_and_maps_bad_queries_to_422() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // A baseline listing has data (the seed populated the catalog).
    let (base_status, _, base_body) =
        send(&app, get(&format!("/api/games/{game}/cards?page=1&page_size=5"))).await;
    assert_eq!(base_status, StatusCode::OK);
    let seeded_total = base_body["total"].as_u64().expect("total");
    assert!(seeded_total > 0, "dummy catalog should have seeded cards");

    // An unknown filter is a client error (422), never a 500.
    let (bad_status, _, bad_body) =
        send(&app, get(&format!("/api/games/{game}/cards?q=boguskey:1"))).await;
    assert_eq!(bad_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(bad_body["error"].as_str().is_some());

    // A SQL-injection payload is treated as a harmless literal name search: it
    // returns 200 and, crucially, the cards table is still intact afterwards.
    let injection = "'; DROP TABLE cards;--";
    let encoded: String = url_encode(injection);
    let (inj_status, _, _) =
        send(&app, get(&format!("/api/games/{game}/cards?q={encoded}"))).await;
    assert_eq!(inj_status, StatusCode::OK);

    let (after_status, _, after_body) =
        send(&app, get(&format!("/api/games/{game}/cards?page=1&page_size=5"))).await;
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(
        after_body["total"].as_u64(),
        Some(seeded_total),
        "the cards table must be untouched by the injection attempt"
    );
}

#[tokio::test]
async fn card_name_autocomplete_returns_distinct_names() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The dummy catalog reprints "Dummy Reprinted Relic" across two sets; the
    // autocomplete lists each unique name once (no per-printing duplicates).
    let (status, _, body) =
        send(&app, get(&format!("/api/games/{game}/card-names?q=Reprinted"))).await;
    assert_eq!(status, StatusCode::OK);
    let names = body["data"].as_array().expect("data array");
    assert_eq!(names.len(), 1, "distinct names only: {names:?}");
    assert_eq!(names[0].as_str(), Some("Dummy Reprinted Relic"));

    // A blank query has nothing to suggest (empty list, not an error).
    let (blank_status, _, blank_body) =
        send(&app, get(&format!("/api/games/{game}/card-names?q="))).await;
    assert_eq!(blank_status, StatusCode::OK);
    assert!(blank_body["data"].as_array().expect("data").is_empty());

    // The handler validates the game first, so an unknown game is a 404 — not a
    // collision with the `/cards/{id}` route registered alongside it.
    let (nf_status, _, _) = send(&app, get("/api/games/nope/card-names?q=Relic")).await;
    assert_eq!(nf_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cards_by_exact_name_returns_every_printing() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // "Dummy Reprinted Relic" has two printings; the exact-name filter returns both.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?name={}",
            url_encode("Dummy Reprinted Relic")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(2), "both printings: {body:?}");

    // A name nobody prints returns an empty page (200 with total 0), not an error.
    let (miss_status, _, miss_body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?name={}",
            url_encode("No Such Card")
        )),
    )
    .await;
    assert_eq!(miss_status, StatusCode::OK);
    assert_eq!(miss_body["total"].as_u64(), Some(0));
}

/// Regression: a colour search on the by-drop set view
/// (`GET /sets/sld/drops?q=c:rg`) took the dev server down (2026-07-01 report).
/// The by-drop endpoint must answer a searched request like any other list route.
#[tokio::test]
async fn set_drops_color_search_succeeds() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;

    // An `sld` set row plus coloured cards: collector number 2658 is in a known
    // drop ("Wild in Bloom"); 999999 isn't in the snapshot (folds into "Other").
    crate::test_support::card_set_model("sld")
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld set");
    for (id, cn, colors) in [(1, "2658", "R,G"), (2, "999999", "R")] {
        crate::entities::card::Model {
            set_code: "sld".into(),
            set_name: "Secret Lair Drop".into(),
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            colors: Some(colors.into()),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld card");
    }
    let app = crate::build_router(state);

    let (status, _, body) =
        send(&app, get("/api/games/mtg/sets/sld/drops?page=1&page_size=20&q=c%3Arg")).await;
    assert_eq!(status, StatusCode::OK, "drops search must succeed: {body:?}");
    // Only the R,G card matches c:rg (colour ⊇ {R,G}); its drop is the one group.
    let groups = body["data"].as_array().expect("drop groups");
    assert_eq!(groups.len(), 1, "one matching drop: {body:?}");
    assert_eq!(groups[0]["title"].as_str(), Some("Wild in Bloom"));
    assert_eq!(groups[0]["card_count"].as_u64(), Some(1));
}

/// The by-drop view's "filter drops by name" box (`?drop=`) narrows the response to the
/// drops whose curated title matches — case-insensitively — spanning the whole set (not
/// one page), and reports the filtered count so pagination stays correct.
#[tokio::test]
async fn set_drops_title_filter_narrows_by_drop_name() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;

    // Two cards in two different named drops: 2658 -> "Wild in Bloom", 168 -> "Inked".
    crate::test_support::card_set_model("sld")
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld set");
    for (id, cn) in [(1, "2658"), (2, "168")] {
        crate::entities::card::Model {
            set_code: "sld".into(),
            set_name: "Secret Lair Drop".into(),
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld card");
    }
    let app = crate::build_router(state);

    // "BLOOM" (any case) matches only the "Wild in Bloom" drop; the total is the
    // filtered drop count, not the set's total drops.
    let (status, _, body) = send(&app, get("/api/games/mtg/sets/sld/drops?drop=BLOOM")).await;
    assert_eq!(status, StatusCode::OK, "drop filter must succeed: {body:?}");
    let groups = body["data"].as_array().expect("drop groups");
    assert_eq!(groups.len(), 1, "one matching drop: {body:?}");
    assert_eq!(groups[0]["title"].as_str(), Some("Wild in Bloom"));
    assert_eq!(body["total"].as_u64(), Some(1), "total reflects the filtered drops");

    // A filter matching no drop title is an empty (still 200) page.
    let (status, _, body) = send(&app, get("/api/games/mtg/sets/sld/drops?drop=no-such-drop")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["total"].as_u64(), Some(0));
}

/// Each drop header carries a `cheapest_prints_usd` total: for each distinct card in the drop,
/// the price of its cheapest printing *anywhere* (not the Secret Lair printing), summed. This
/// exercises the cross-set floor (a cheap reprint wins), de-dup by gameplay identity, the
/// no-`oracle_id`/foil-only fallbacks, and the all-unpriced `null` (issue #456).
#[tokio::test]
async fn set_drops_report_cheapest_prints_total() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;

    crate::test_support::card_set_model("sld")
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld set");

    // sld printings across three drops (id, collector#, oracle_id, usd, usd_foil):
    // "Wild in Bloom" (2658..2662):
    //   2658 + 2659 are two printings of ONE card (or-vivien) — de-duped to a single card;
    //         both are pricey Secret Lair printings, but a cheap reprint below floors it at 2.00.
    //   2660 (or-sands) is foil-only -> 3.50.
    //   2661 has no oracle_id (no siblings) -> priced by its own finishes -> 4.00.
    //   2662 (or-unpriced) has no priced printing -> contributes nothing (and can't null the drop).
    // "Cats of Chaos" (2690): a lone unpriced card -> that drop totals null.
    // "Inked" (168): foil-only, no reprint -> 14.00.
    let sld: [(i32, &str, Option<&str>, Option<&str>, Option<&str>); 7] = [
        (1, "2658", Some("or-vivien"), Some("20.00"), Some("30.00")),
        (2, "2659", Some("or-vivien"), Some("25.00"), None),
        (3, "2660", Some("or-sands"), None, Some("3.50")),
        (4, "2661", None, Some("4.00"), Some("8.00")),
        (5, "2662", Some("or-unpriced"), None, None),
        (6, "2690", Some("or-cat"), None, None),
        (7, "168", Some("or-inked"), None, Some("14.00")),
    ];
    for (id, cn, oracle, usd, foil) in sld {
        crate::entities::card::Model {
            set_code: "sld".into(),
            set_name: "Secret Lair Drop".into(),
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            oracle_id: oracle.map(str::to_string),
            price_usd: usd.map(str::to_string),
            price_usd_foil: foil.map(str::to_string),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld card");
    }
    // A cheap reprint of Vivien in another set (same oracle_id) — the catalog-wide floor the
    // drop total must find instead of the $20+ Secret Lair printings above.
    crate::entities::card::Model {
        set_code: "m21".into(),
        set_name: "Core 2021".into(),
        collector_number: "100".into(),
        collector_number_int: Some(100),
        oracle_id: Some("or-vivien".into()),
        price_usd: Some("2.00".into()),
        price_usd_foil: Some("9.00".into()),
        ..crate::test_support::card_model(8)
    }
    .into_active_model()
    .insert(&state.db)
    .await
    .expect("insert reprint");

    let app = crate::build_router(state);

    let (status, _, body) = send(&app, get("/api/games/mtg/sets/sld/drops?page=1&page_size=20")).await;
    assert_eq!(status, StatusCode::OK, "drops must succeed: {body:?}");
    let groups = body["data"].as_array().expect("drop groups");
    let total = |title: &str| {
        groups
            .iter()
            .find(|g| g["title"] == title)
            .unwrap_or_else(|| panic!("{title} present: {body:?}"))["cheapest_prints_usd"]
            .clone()
    };

    // or-vivien floored at the 2.00 reprint (counted once, not per printing) + or-sands 3.50
    // + the no-oracle 2661 at 4.00 + nothing for the unpriced 2662 = 9.50.
    assert_eq!(total("Wild in Bloom").as_str(), Some("9.50"));
    // Foil-only, no reprint -> its foil price.
    assert_eq!(total("Inked").as_str(), Some("14.00"));
    // A drop with no priced printing reports null, not "0.00".
    assert!(total("Cats of Chaos").is_null(), "unpriced drop -> null: {body:?}");
}
