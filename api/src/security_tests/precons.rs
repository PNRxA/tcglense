//! The preconstructed-deck browser: the `/api/games/{game}/precons*` reads are public,
//! shared-cacheable and filter correctly, and copying one into your own decks is an
//! authenticated write that produces a deck the rest of the app understands.
//!
//! Drives the real router over the seeded dummy catalog (five precons — a Commander deck, a
//! small all-foil deck with no command zone, a starter deck with a sideboard, and two Jumpstart
//! themes), so the reads answer in the real wire shapes and the copy lands real cards.

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
    assert_eq!(body["total"], 5);
    let data = body["data"].as_array().expect("data array");

    // Newest first: the 2024 sets lead the 2019 `sld` deck.
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
    assert_eq!(body["data"][0]["deck_type"], "Dandan Deck");

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

/// The precon list shares the sealed list's per-word name rule, so it shares its guard: a long
/// `?q` is a 422, not a stack overflow that aborts the process. Both the flat and the grouped
/// read build through the same filter, so both are covered. See
/// `handlers::shared::search::every_word_matches`.
#[tokio::test]
async fn a_very_long_search_is_refused_rather_than_crashing_the_server() {
    let app = test_app_with_catalog().await;

    let long = vec!["a"; 5_000].join("+");
    for path in [
        format!("/api/games/mtg/precons?q={long}"),
        format!("/api/games/mtg/precons/groups?q={long}"),
    ] {
        let (status, _, _) = send(&app, get(&path)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
    }

    // Still serving, and a normal multi-word search is unaffected.
    let (status, _, body) = send(&app, get("/api/games/mtg/precons?q=commander%20dummy")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
}

/// `include_related` spans the set's whole catalog group, which is what the landing's grouped
/// "All decks" link rides. Driven from the **child** side (`tdmb`, a sub-set of `dmb` with no
/// precons of its own) because that's the real shape — a set's Commander sub-set carries the
/// decks while another sub-set carries none — and because it can't pass by accident: without
/// the span, `tdmb` matches nothing at all.
#[tokio::test]
async fn include_related_spans_the_sets_whole_group() {
    let app = test_app_with_catalog().await;

    // The root's own decks, and the sub-set's (none) — the two ends of the group.
    let (_, _, body) = send(&app, get("/api/games/mtg/precons?set=dmb")).await;
    let in_root = body["total"].as_i64().unwrap();
    assert!(in_root > 0, "the base set seeds precons");
    let (_, _, body) = send(&app, get("/api/games/mtg/precons?set=tdmb")).await;
    assert_eq!(
        body["total"], 0,
        "the token sub-set has no precons of its own"
    );

    // Spanning the group reaches the root's decks from either end.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/precons?set=tdmb&include_related=true"),
    )
    .await;
    assert_eq!(body["total"], in_root);
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/precons?set=dmb&include_related=true"),
    )
    .await;
    assert_eq!(body["total"], in_root);

    // The grouped view spans the same sets — the two must never disagree about what matches.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/precons/groups?set=tdmb&include_related=true"),
    )
    .await;
    let grouped: i64 = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["deck_count"].as_i64().unwrap())
        .sum();
    assert_eq!(grouped, in_root);

    // Without a `set` there is nothing to span, so the flag is a no-op rather than a filter.
    let (_, _, unfiltered) = send(&app, get("/api/games/mtg/precons")).await;
    let (_, _, body) = send(&app, get("/api/games/mtg/precons?include_related=true")).await;
    assert_eq!(body["total"], unfiltered["total"]);
}

/// A group ships **every** deck it counts — the grouped listings are deliberately uncapped
/// (see docs/tradeoffs.md § Preconstructed decks), so `deck_count` and `decks.len()` must not
/// drift apart. Seeds 40 decks of one type so the assertion has something to catch: a preview
/// cap was tried here and reverted, and this is what makes reintroducing one a deliberate act
/// with a failing test attached rather than a silent change to what a client is shown.
#[tokio::test]
async fn a_group_ships_every_deck_it_counts() {
    use crate::entities::precon_deck;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, NotSet};

    let app = test_app_with_catalog().await;
    let now = chrono::Utc::now();
    for n in 0..40 {
        precon_deck::ActiveModel {
            id: NotSet,
            game: Set("mtg".to_string()),
            slug: Set(format!("bulk-theme-{n}-dmb")),
            name: Set(format!("Bulk Theme {n}")),
            set_code: Set("dmb".to_string()),
            deck_type: Set("Bulk Theme".to_string()),
            released_at: Set(Some("2024-01-15".to_string())),
            color_identity: Set(None),
            card_count: Set(60),
            sideboard_count: Set(0),
            face_card_id: Set(None),
            product_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&app.state.db)
        .await
        .expect("insert bulk precon");
    }

    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/precons/groups?group=type&type=Bulk%20Theme"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let group = &body["data"][0];
    assert_eq!(group["title"], "Bulk Theme");
    assert_eq!(group["deck_count"], 40);
    assert_eq!(
        group["decks"].as_array().unwrap().len(),
        40,
        "a group must ship every deck it counts — no silent truncation"
    );
}

/// The whole point of the precon analysis mirror: a published decklist must report the SAME
/// verdicts the deck you get from "Copy to my decks" reports. They are two different section
/// vocabularies over the same cards — the page synthesises one section per board, the copy
/// files the mainboard into type buckets — so this proves the thing that actually matters:
/// both zone-split identically, because `rules::deck_zone` reads the section NAME.
///
/// If the synthesised names ever stop landing in the zones they claim, this catches it as a
/// disagreement rather than as a plausible-looking wrong answer.
#[tokio::test]
async fn a_precons_analysis_matches_the_deck_you_copy_from_it() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "precon-analyst@example.com", PW).await;

    // The precon page's answers, anonymous.
    let (status, headers, precon_legality) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}/legality")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "anonymous read: {precon_legality:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a precon's analysis is public catalog data, so it is CDN-cacheable"
    );
    let (_, _, precon_bracket) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}/bracket")),
    )
    .await;
    let (_, _, precon_stats) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}/stats")),
    )
    .await;

    // Now copy it and ask the deck the same three questions.
    let (_, _, deck) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/decks/mtg/precons/{COMMANDER_SLUG}/copy"),
            &access,
            json!({}),
        ),
    )
    .await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let (_, _, deck_legality) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    let (_, _, deck_bracket) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/bracket"), &access),
    )
    .await;
    let (_, _, deck_stats) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/stats"), &access),
    )
    .await;

    // Both nullable answers ride `DataBody`, like every other mirror — unwrap once so a
    // comparison can't pass by reading two absent fields as equal.
    let precon_legality = &precon_legality["data"];
    let deck_legality = &deck_legality["data"];
    let precon_bracket = &precon_bracket["data"];
    let deck_bracket = &deck_bracket["data"];
    assert!(
        precon_legality.is_object(),
        "a Commander Deck states its format, so it is judged: {precon_legality:?}"
    );
    assert!(
        precon_bracket.is_object(),
        "and the bracket ladder is defined for it: {precon_bracket:?}"
    );

    // The verdict, its construction breaches and the bracket must be identical.
    assert_eq!(
        precon_legality["format_key"], deck_legality["format_key"],
        "precon {precon_legality:?} vs copy {deck_legality:?}"
    );
    assert_eq!(
        precon_legality["violations"], deck_legality["violations"],
        "the deck-wide construction breaches must agree — this is what the zone split decides"
    );
    // Which names are a problem, why, and how many copies — compared exactly. NOT the
    // issue's `card_id`, which is "one printing (for keys/links)" and is therefore whichever
    // printing of that name the surface's own row order reached first: this loader tie-breaks
    // on the catalog's stable `cards.id`, while a copied deck's rows are ordered by the
    // insertion order `plan_sections` produced. A name held in two sets (the seeded
    // "Dummy White Sentinel" is in both `dmb` and `dmu`) legitimately reports a different
    // representative printing on each surface; the verdict about it is identical.
    let names = |v: &serde_json::Value| -> Vec<(String, String, i64)> {
        v["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .map(|i| {
                (
                    i["name"].as_str().unwrap_or_default().to_string(),
                    i["status"].as_str().unwrap_or_default().to_string(),
                    i["quantity"].as_i64().unwrap_or_default(),
                )
            })
            .collect()
    };
    assert_eq!(
        names(precon_legality),
        names(deck_legality),
        "the same cards must be flagged, for the same reason, in the same numbers"
    );
    assert!(
        !names(precon_legality).is_empty(),
        "the seeded precon is deliberately illegal, so this comparison has something to compare"
    );
    assert_eq!(
        precon_bracket["bracket"], deck_bracket["bracket"],
        "precon {precon_bracket:?} vs copy {deck_bracket:?}"
    );
    assert_eq!(precon_bracket["categories"], deck_bracket["categories"]);

    // And the composition: same cards, so the same copies in the deck proper.
    assert_eq!(
        precon_stats["deck"]["total_copies"], deck_stats["deck"]["total_copies"],
        "precon {:?} vs copy {:?}",
        precon_stats["deck"], deck_stats["deck"]
    );
}

/// The synthesised sections are the SPA's own vocabulary, and the stats panel round-trips their
/// ids through `?sections=` — so the ids the response advertises must be the ones it accepts.
#[tokio::test]
async fn precon_stats_sections_are_the_spa_vocabulary() {
    let app = test_app_with_catalog().await;

    let (_, _, stats) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}/stats")),
    )
    .await;
    // The seeded Commander precon has a command zone (0) and a mainboard (1); the library
    // defaults to the deck proper minus the command zone.
    let default_ids: Vec<i64> = stats["default_library_section_ids"]
        .as_array()
        .expect("default library")
        .iter()
        .map(|v| v.as_i64().expect("id"))
        .collect();
    assert!(
        !default_ids.contains(&0),
        "the command zone is not shuffled into the library: {default_ids:?}"
    );
    assert!(default_ids.contains(&1), "the deck is: {default_ids:?}");

    // Those ids are accepted back, which is what the panel's checkboxes do.
    let (status, _, scoped) = send(
        &app,
        get(&format!(
            "/api/games/mtg/precons/{COMMANDER_SLUG}/stats?sections=1"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(scoped["library"]["total_copies"].as_i64().unwrap() > 0);
}

/// A seedless goldfish must not be cached: the roll is random, so the response is not a
/// function of its URL, and these routes carry a CDN TTL of an hour plus a day of
/// stale-while-revalidate. A seeded one is reproducible and cacheable.
#[tokio::test]
async fn a_seedless_precon_goldfish_is_never_cached() {
    let app = test_app_with_catalog().await;

    let (status, headers, hand) = send(
        &app,
        get(&format!("/api/games/mtg/precons/{COMMANDER_SLUG}/goldfish")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hand:?}");
    assert_eq!(
        cache_control(&headers),
        Some("no-store"),
        "a random hand pinned at a CDN would be everyone's hand"
    );

    // Seeded: reproducible, and therefore safe to cache.
    let seeded = format!("/api/games/mtg/precons/{COMMANDER_SLUG}/goldfish?seed=42");
    let (_, headers, first) = send(&app, get(&seeded)).await;
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    let (_, _, again) = send(&app, get(&seeded)).await;
    assert_eq!(first["hand"], again["hand"], "a seed reproduces its hand");
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
    assert_eq!(body["data"]["total"], 5);
    let types: Vec<&str> = body["data"]["types"]
        .as_array()
        .expect("types")
        .iter()
        .map(|t| t["type"].as_str().expect("type"))
        .collect();
    assert!(types.contains(&"Commander Deck"));
    assert!(types.contains(&"Dandan Deck"));
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
async fn precons_group_by_set_and_paginate_by_group() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/precons/groups")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    // The page counts SETS, not decks: five seeded precons across three sets.
    assert_eq!(body["total"], 3);
    let groups = body["data"].as_array().expect("groups");
    assert_eq!(groups.len(), 3);

    // Newest SET first: dmu (2024-06) then dmb (2024-01) then sld (2019).
    let codes: Vec<&str> = groups
        .iter()
        .map(|g| g["slug"].as_str().expect("slug"))
        .collect();
    assert_eq!(codes, vec!["dmu", "dmb", "sld"]);
    assert_eq!(groups[0]["title"], "Dummy Universe");
    assert_eq!(
        groups[0]["set_code"], "dmu",
        "a set group links to its own page"
    );
    assert_eq!(groups[0]["deck_count"], 1);
    // Each group carries its decks in full, tile facets and all.
    let deck = &groups[0]["decks"][0];
    assert_eq!(deck["slug"], COMMANDER_SLUG);
    assert!(deck["face_card"]["card_id"].as_str().is_some());

    // A group is never split across a page boundary: one set per page here.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/precons/groups?page_size=1&page=2"),
    )
    .await;
    assert_eq!(body["total"], 3, "the total still counts sets");
    assert_eq!(body["data"].as_array().expect("groups").len(), 1);
    assert_eq!(body["data"][0]["slug"], "dmb");
    assert_eq!(body["has_more"], true);
}

#[tokio::test]
async fn precons_group_by_deck_type_biggest_category_first() {
    let app = test_app_with_catalog().await;

    let (status, _, body) = send(&app, get("/api/games/mtg/precons/groups?group=type")).await;
    assert_eq!(status, StatusCode::OK);
    // One group per deck type the game has, not per set.
    assert_eq!(body["total"], 4);
    let groups = body["data"].as_array().expect("groups");
    let titles: Vec<&str> = groups
        .iter()
        .map(|g| g["title"].as_str().expect("title"))
        .collect();
    assert!(titles.contains(&"Commander Deck"), "{titles:?}");
    assert!(titles.contains(&"Dandan Deck"), "{titles:?}");
    // Biggest category first: two Jumpstart themes outrank every one-deck type.
    assert_eq!(titles[0], "Jumpstart", "{titles:?}");
    assert_eq!(groups[0]["deck_count"], 2);
    // Every group carries its decks, anchor-safe slug, and no set link/date (a type has
    // neither) — the two things the set grouping fills in.
    let commander = groups
        .iter()
        .find(|g| g["title"] == "Commander Deck")
        .expect("a Commander Deck group");
    assert_eq!(commander["slug"], "commander-deck");
    assert!(commander["set_code"].is_null());
    assert!(commander["released_at"].is_null());
    assert_eq!(commander["deck_count"], 1);
    assert_eq!(commander["decks"][0]["slug"], COMMANDER_SLUG);

    // Scoping to a set groups *that set's* decks by type — the set page's default view, and
    // the reason it exists: the base set's three decks split into two named sections.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/precons/groups?group=type&set=dmb"),
    )
    .await;
    assert_eq!(body["total"], 2);
    let titles: Vec<&str> = body["data"]
        .as_array()
        .expect("groups")
        .iter()
        .map(|g| g["title"].as_str().expect("title"))
        .collect();
    assert_eq!(titles, vec!["Jumpstart", "Starter Deck"]);
}

#[tokio::test]
async fn grouped_and_flat_views_agree_on_what_matches() {
    let app = test_app_with_catalog().await;

    // Every view shares one filter builder, so a filter must select the same decks whichever
    // layout is asked for — only the grouping may differ.
    for query in [
        "?type=Commander%20Deck",
        "?q=dummy%20starter",
        "?set=sld",
        "?group=type",
        "?q=dummy&group=type",
    ] {
        let (_, _, flat) = send(&app, get(&format!("/api/games/mtg/precons{query}"))).await;
        let (_, _, grouped) =
            send(&app, get(&format!("/api/games/mtg/precons/groups{query}"))).await;
        // Compared as *sets*: the claim is which decks match, not the sequence. A grouping is
        // free to reorder (by-type leads with the biggest category), which is exactly the part
        // a shared filter builder is not responsible for.
        let mut flat_slugs: Vec<&str> = flat["data"]
            .as_array()
            .expect("decks")
            .iter()
            .map(|d| d["slug"].as_str().expect("slug"))
            .collect();
        let mut grouped_slugs: Vec<&str> = grouped["data"]
            .as_array()
            .expect("groups")
            .iter()
            .flat_map(|g| g["decks"].as_array().expect("decks"))
            .map(|d| d["slug"].as_str().expect("slug"))
            .collect();
        flat_slugs.sort_unstable();
        grouped_slugs.sort_unstable();
        assert_eq!(flat_slugs, grouped_slugs, "disagreement on {query}");
        assert!(!flat_slugs.is_empty(), "{query} matched nothing to compare");
    }
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
        "/api/games/nope/precons/groups",
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

    // The seeded precon lists one printing in both finishes (the Jumpstart/bundle shape): the
    // copy must hold it as ONE card with both counts, not two rows — which `deck_cards`'
    // unique `(deck, card, section)` key would reject outright.
    let lands: Vec<&Value> = cards
        .iter()
        .filter(|c| c["card"]["id"] == "dummy-dmb-0001")
        .collect();
    assert_eq!(
        lands.len(),
        1,
        "the mixed-finish printing folded: {lands:?}"
    );
    assert_eq!(lands[0]["quantity"], 20);
    assert_eq!(lands[0]["foil_quantity"], 1);

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
