//! Public search route — injection-safe, malformed -> 422 (not 500).

use super::harness::*;
use crate::test_support::url_encode;

#[tokio::test]
async fn search_is_injection_safe_and_maps_bad_queries_to_422() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // A baseline listing has data (the seed populated the catalog).
    let (base_status, _, base_body) = send(
        &app,
        get(&format!("/api/games/{game}/cards?page=1&page_size=5")),
    )
    .await;
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
    let (inj_status, _, _) = send(&app, get(&format!("/api/games/{game}/cards?q={encoded}"))).await;
    assert_eq!(inj_status, StatusCode::OK);

    let (after_status, _, after_body) = send(
        &app,
        get(&format!("/api/games/{game}/cards?page=1&page_size=5")),
    )
    .await;
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
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/card-names?q=Reprinted")),
    )
    .await;
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

/// End-to-end (issue #479): a bare `cn:` number matches the number itself *and* its
/// single-letter `#XXXz` variants, but never a digit-suffixed or longer number; `cn=`
/// and an already-suffixed `cn:` stay exact. Runs the compiled SQL against real SQLite.
#[tokio::test]
async fn collector_number_bare_matches_letter_variants() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;
    crate::test_support::card_set_model("tst")
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert set");

    // 123 + two letter variants match; the digit-suffixed 1230/1234 and the star
    // variant 123★ must not (only ASCII letters count as the trailing "z").
    for (id, cn) in [
        (1, "123"),
        (2, "123a"),
        (3, "123b"),
        (4, "1230"),
        (5, "1234"),
        (6, "123★"),
    ] {
        crate::entities::card::Model {
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert card");
    }
    let app = crate::build_router(state);

    for (q, expected) in [
        ("cn:123", vec!["123", "123a", "123b"]),
        ("cn=123", vec!["123"]),
        ("cn:123a", vec!["123a"]),
    ] {
        let (status, _, body) = send(
            &app,
            get(&format!("/api/games/mtg/cards?q={}", url_encode(q))),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "q={q}: {body:?}");
        let mut got: Vec<&str> = body["data"]
            .as_array()
            .expect("data")
            .iter()
            .map(|c| c["collector_number"].as_str().expect("collector_number"))
            .collect();
        got.sort();
        assert_eq!(got, expected, "q={q}: {body:?}");
    }
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

    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/sets/sld/drops?page=1&page_size=20&q=c%3Arg"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "drops search must succeed: {body:?}"
    );
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
    assert_eq!(
        body["total"].as_u64(),
        Some(1),
        "total reflects the filtered drops"
    );

    // A filter matching no drop title is an empty (still 200) page.
    let (status, _, body) =
        send(&app, get("/api/games/mtg/sets/sld/drops?drop=no-such-drop")).await;
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

    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/sets/sld/drops?page=1&page_size=20"),
    )
    .await;
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
    assert!(
        total("Cats of Chaos").is_null(),
        "unpriced drop -> null: {body:?}"
    );
}

/// Each drop header carries a `released_at` derived from its cards: a drop's cards share one
/// street date, so the response reports the most common non-null date (a stray reprint carrying
/// a different date is outvoted), and a drop whose cards carry no date reports null.
#[tokio::test]
async fn set_drops_report_release_date() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;

    crate::test_support::card_set_model("sld")
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld set");

    // sld printings across three drops (id, collector#, released_at):
    // "Wild in Bloom" (2658..2660): two cards dated the street date + one stray reprint date
    //   -> the drop reports the modal (street) date, not the stray.
    // "Inked" (168): a single dated card -> that date.
    // "Cats of Chaos" (2690): a card with no date -> the drop reports null.
    let sld: [(i32, &str, Option<&str>); 5] = [
        (1, "2658", Some("2026-07-27")),
        (2, "2659", Some("2026-07-27")),
        (3, "2660", Some("2019-01-01")),
        (4, "168", Some("2024-05-01")),
        (5, "2690", None),
    ];
    for (id, cn, released) in sld {
        crate::entities::card::Model {
            set_code: "sld".into(),
            set_name: "Secret Lair Drop".into(),
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            released_at: released.map(str::to_string),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert sld card");
    }

    let app = crate::build_router(state);

    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/sets/sld/drops?page=1&page_size=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drops must succeed: {body:?}");
    let groups = body["data"].as_array().expect("drop groups");
    let released = |title: &str| {
        groups
            .iter()
            .find(|g| g["title"] == title)
            .unwrap_or_else(|| panic!("{title} present: {body:?}"))["released_at"]
            .clone()
    };

    // Two cards vote for the street date, one stray reprint date is outvoted.
    assert_eq!(released("Wild in Bloom").as_str(), Some("2026-07-27"));
    assert_eq!(released("Inked").as_str(), Some("2024-05-01"));
    // A drop whose cards carry no date reports null.
    assert!(
        released("Cats of Chaos").is_null(),
        "dateless drop -> null: {body:?}"
    );
}

/// End-to-end (issue #140): `art:` (and aliases) match by tagged **artwork** against
/// the seeded dummy art tags — a tag matches every printing sharing the tagged
/// illustration, the ingest-expanded ancestor tag matches the same artworks, negation
/// stays total, an unknown tag matches nothing, and oracle tags remain 422.
#[tokio::test]
async fn art_tag_search_matches_by_artwork() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Baseline catalog size, for the negation check below.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/cards?page_size=1"))).await;
    assert_eq!(status, StatusCode::OK);
    let seeded_total = body["total"].as_u64().expect("total");

    // `relic` tags the artwork the two "Dummy Reprinted Relic" printings share, so all
    // three filter aliases return both printings; `object` is the seeded ancestor tag
    // carrying the same (hierarchy-expanded) artwork row.
    for q in ["art:relic", "arttag:relic", "atag:relic", "art:object"] {
        let (status, _, body) = send(
            &app,
            get(&format!("/api/games/{game}/cards?q={}", url_encode(q))),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{q}");
        assert_eq!(body["total"].as_u64(), Some(2), "{q}: {body:?}");
    }

    // `squirrel` tags a single unrelated artwork — exactly one card.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={}",
            url_encode("atag:squirrel")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(1), "{body:?}");

    // The same artwork carries both `relic` and its ancestor: requiring one while
    // negating the other matches nothing.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={}",
            url_encode("art:relic -atag:object")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(0), "{body:?}");

    // Negation is total: every card except the two tagged printings (cards with no
    // illustration at all count as "not tagged", mirroring Scryfall).
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={}",
            url_encode("-art:relic")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(seeded_total - 2), "{body:?}");

    // An unknown tag simply matches nothing (200, not an error), mirroring Scryfall.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={}",
            url_encode("art:no-such-tag")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(0));

    // Oracle tags are still deliberately unsupported (422).
    let (status, _, _) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={}",
            url_encode("otag:removal")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// The art-tag lookup endpoint: blank `q` returns the full slug-ordered vocabulary
/// (the tag-browser payload), `q` ranks starts-with matches first, `limit` caps, and
/// an unknown game is 404.
#[tokio::test]
async fn art_tag_lookup_lists_and_ranks_tags() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Blank q -> the whole seeded vocabulary, ordered by slug.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/art-tags"))).await;
    assert_eq!(status, StatusCode::OK);
    let tags = body["data"].as_array().expect("data array");
    let slugs: Vec<&str> = tags.iter().filter_map(|t| t["slug"].as_str()).collect();
    let mut sorted = slugs.clone();
    sorted.sort_unstable();
    assert_eq!(slugs, sorted, "the full listing is slug-ordered: {body:?}");
    for expected in ["object", "relic", "squirrel"] {
        assert!(slugs.contains(&expected), "{expected} missing: {body:?}");
    }
    // The entry shape: label and the artwork count, straight off the seeded metadata.
    let relic = tags
        .iter()
        .find(|t| t["slug"] == "relic")
        .expect("relic entry");
    assert_eq!(relic["label"].as_str(), Some("Relic"));
    assert_eq!(relic["count"].as_i64(), Some(14));

    // `re` prefixes `relic` and merely appears inside `squirrel` (and, via its label,
    // `no-creature`): starts-with ranks first, then the contains-matches by count desc.
    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/art-tags?q=re"))).await;
    assert_eq!(status, StatusCode::OK);
    let slugs: Vec<&str> = body["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|t| t["slug"].as_str())
        .collect();
    assert_eq!(slugs, ["relic", "no-creature", "squirrel"], "{body:?}");

    // Labels match too (case-insensitively), and `limit` caps the suggestions.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/art-tags?q=Rel&limit=1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().expect("data").len(), 1);

    // The handler validates the game first: unknown game -> 404.
    let (status, _, _) = send(&app, get("/api/games/nope/art-tags")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The catalog-listing foil-variant fold, end to end on the page that reported it
/// (`/cards/mtg/sets/sld?drop=miku`).
///
/// Scryfall models six of "Hatsune Miku: Sakura Superstar"'s printings as a nonfoil card
/// plus a separate foil object one star along (`1587` / `1587★`), and the drop snapshot
/// lists both numbers — so the grid showed each card twice, the second tile carrying the
/// same foil price the enrichment had already copied onto the first. The fold keeps one
/// tile per printing while leaving every star that *isn't* a folded duplicate alone, and
/// the star's own Scryfall id keeps resolving on the detail route.
#[tokio::test]
async fn sld_drops_fold_a_foil_star_onto_its_nonfoil_base() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let state = test_state().await;
    // Real stored `card_count`s, since the tile's count is Scryfall's set-object count less
    // whatever this fold hides: `sld` stores its 3 objects (one of which folds), `9ed` its 2
    // (neither folds), and `10e`'s row lags the cards it counts at 0 — the stale-sync case the
    // subtraction has to clamp rather than publish as `-1`.
    for (id, code, card_count) in [(1, "sld", 3), (2, "9ed", 2), (3, "10e", 0)] {
        crate::entities::card_set::Model {
            id,
            card_count,
            ..crate::test_support::card_set_model(code)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert set");
    }

    // 1587/1587★ ("Shelter") are both in the Sakura Superstar drop's snapshot entry and differ
    // only by the rainbow-foil treatment; 796★ ("Mana Vault", Fallout: Vault Boy) is a real
    // orphan — `sld` has no 796 at all. 9ed 188/188★ is the population the fold must NOT
    // touch: the foil is black-bordered where the nonfoil is white, so they are two printings
    // a visitor can tell apart and `border:black` queries directly. 10e 3/3★ is a plain
    // attribute-identical fold in a set whose stored count lags it.
    type Row = (
        i32,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    );
    let rows: [Row; 7] = [
        (
            1,
            "sld",
            "1587",
            "nonfoil",
            "ora-shelter",
            "borderless",
            None,
            Some("6.26"),
            Some("12.33"),
        ),
        (
            2,
            "sld",
            "1587★",
            "foil",
            "ora-shelter",
            "borderless",
            Some("rainbowfoil"),
            None,
            Some("12.33"),
        ),
        (
            3,
            "sld",
            "796★",
            "foil",
            "ora-vault",
            "black",
            None,
            None,
            Some("40.00"),
        ),
        (
            4,
            "9ed",
            "188",
            "nonfoil",
            "ora-chariot",
            "white",
            None,
            Some("0.30"),
            None,
        ),
        (
            5,
            "9ed",
            "188★",
            "foil",
            "ora-chariot",
            "black",
            None,
            None,
            Some("9.00"),
        ),
        (
            6,
            "10e",
            "3",
            "nonfoil",
            "ora-angel",
            "black",
            None,
            Some("0.10"),
            None,
        ),
        (
            7,
            "10e",
            "3★",
            "foil",
            "ora-angel",
            "black",
            None,
            None,
            Some("1.00"),
        ),
    ];
    for (id, set, cn, finishes, oracle, border, promo, usd, usd_foil) in rows {
        crate::entities::card::Model {
            external_id: format!("ext-{id}"),
            set_code: set.into(),
            set_name: match set {
                "sld" => "Secret Lair Drop".into(),
                "10e" => "Tenth Edition".into(),
                _ => "Ninth Edition".into(),
            },
            collector_number: cn.into(),
            collector_number_int: cn.trim_end_matches('★').parse().ok(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle.into()),
            border_color: Some(border.into()),
            promo_types: promo.map(str::to_string),
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert card");
    }
    // The fold is decided once by the sync-tick pass and persisted, so the fixture runs it the
    // way a real instance does before serving a listing.
    crate::scryfall::refresh_foil_variant_folds(&state.db, "mtg")
        .await
        .expect("fold pass");
    let app = crate::build_router(state);

    let numbers = |body: &serde_json::Value, path: &str| -> Vec<String> {
        let mut got: Vec<String> = body[path]
            .as_array()
            .expect("cards")
            .iter()
            .map(|c| c["collector_number"].as_str().expect("number").to_string())
            .collect();
        got.sort();
        got
    };

    // The set grid lists the base once; the orphan star is untouched.
    let (status, _, body) = send(&app, get("/api/games/mtg/sets/sld/cards?page_size=50")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(numbers(&body, "data"), ["1587", "796★"], "{body:?}");
    assert_eq!(body["total"].as_u64(), Some(2), "{body:?}");

    // So does the by-drop view the report came from — including its `card_count`, which
    // used to count the star as a second card in the drop.
    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/sets/sld/drops?page_size=50&drop=sakura"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let groups = body["data"].as_array().expect("drop groups");
    assert_eq!(groups.len(), 1, "one matching drop: {body:?}");
    assert_eq!(groups[0]["card_count"].as_u64(), Some(1), "{body:?}");
    assert_eq!(numbers(&groups[0], "cards"), ["1587"], "{body:?}");

    // The 9th-Edition pair is left alone: both printings still list, and the black-bordered
    // foil still answers the search that is the whole reason to keep it.
    let (status, _, body) = send(&app, get("/api/games/mtg/sets/9ed/cards?page_size=50")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(numbers(&body, "data"), ["188", "188★"], "{body:?}");
    let (status, _, body) =
        send(&app, get("/api/games/mtg/cards?q=set%3A9ed+border%3Ablack")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(numbers(&body, "data"), ["188★"], "{body:?}");

    // `is:foil` still finds the base: its foil is real, it just lives on the folded star.
    // The orphan star and the unfolded 9ed foil answer it on their own `finishes`.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards?q=is%3Afoil")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        numbers(&body, "data"),
        ["1587", "188★", "3", "796★"],
        "{body:?}"
    );

    // So does `is:rainbowfoil` — that token only ever lived on the star we folded away.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards?q=is%3Arainbowfoil")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(numbers(&body, "data"), ["1587"], "{body:?}");

    // The set tile's card count follows the grid it links to rather than Scryfall's object
    // count, so a folded row can't make the header overstate what a visitor can page through.
    let (status, _, list) = send(&app, get("/api/games/mtg/sets")).await;
    assert_eq!(status, StatusCode::OK, "{list:?}");
    let counts: std::collections::HashMap<&str, i64> = list["data"]
        .as_array()
        .expect("sets")
        .iter()
        .map(|s| {
            (
                s["code"].as_str().expect("code"),
                s["card_count"].as_i64().expect("card_count"),
            )
        })
        .collect();
    assert_eq!(
        counts.get("sld"),
        Some(&2),
        "3 stored objects, one of them folded away: {list:?}"
    );
    assert_eq!(
        counts.get("9ed"),
        Some(&2),
        "nothing folded in 9ed, so its stored count stands: {list:?}"
    );
    // A `card_sets` row that lags the cards it counts (0 stored, 1 folded) clamps at zero —
    // a partial sync must never publish a negative "N cards".
    assert_eq!(
        counts.get("10e"),
        Some(&0),
        "the subtraction is floored: {list:?}"
    );

    // And one set's own page answers the same number as its tile in the list — they are two
    // reads of one datum, so they adjust through the same seam.
    let (status, _, body) = send(&app, get("/api/games/mtg/sets/sld")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        body["card_count"].as_i64(),
        counts.get("sld").copied(),
        "the set read and the set list must agree: {body:?}"
    );

    // Folding is presentation-only: the star's Scryfall id still resolves, so existing
    // links, holdings and provider imports that name it keep working.
    let (status, _, body) = send(&app, get("/api/games/mtg/cards/ext-2")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["collector_number"].as_str(), Some("1587★"), "{body:?}");
}
