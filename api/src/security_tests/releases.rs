//! The release calendar (issue #679): `GET /api/games/{game}/releases` is a public,
//! shared-cacheable catalog read that lists exactly what the release heads-ups would notify
//! about — the same predicate, through `catalog::releases` — with each set carrying the
//! precons and sealed products it ships, and the Secret Lair drops read off `sld` alone.
//!
//! Drives the real router over the seeded dummy catalog: two top-level expansions (`dmb`
//! 2024-01-15 with a token child set, `dmu` 2024-06-20 with a Commander precon + products),
//! the `sld` box set (never listed per set; its "Eldraine Wonderland" drop is dated
//! 2026-09-25), and the Zeta stand-in `slz` (a top-level `box` set released 2026-09-02, the
//! `sl`-prefix upgrade).

use super::harness::*;

#[tokio::test]
async fn release_calendar_is_public_and_shared_cacheable() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(
        &app,
        get("/api/games/mtg/releases?from=2024-06-01&to=2024-06-30"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "the calendar is the same for every visitor, so it is browser + CDN cacheable"
    );
    assert!(
        headers.contains_key("etag"),
        "a cacheable catalog success carries an ETag validator"
    );
    // The resolved window is echoed back.
    assert_eq!(body["from"], "2024-06-01");
    assert_eq!(body["to"], "2024-06-30");
}

#[tokio::test]
async fn release_calendar_nests_what_a_set_ships() {
    let app = test_app_with_catalog().await;

    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/releases?from=2024-06-01&to=2024-06-30"),
    )
    .await;
    let sets = body["sets"].as_array().expect("sets array");
    assert_eq!(sets.len(), 1, "{sets:?}");
    let dmu = &sets[0];
    assert_eq!(dmu["set"]["code"], "dmu");
    assert_eq!(dmu["set"]["name"], "Dummy Universe");
    assert_eq!(dmu["released_at"], "2024-06-20");
    assert_eq!(dmu["secret_lair"], false);
    // The nested set is the `/sets` payload — the same shape a set tile already renders.
    assert!(dmu["set"]["card_count"].as_i64().unwrap() > 0);
    assert!(dmu["set"]["has_drops"].is_boolean());

    // The Commander precon and the two sealed products filed under `dmu` ride along, in the
    // shapes their own listings publish (a tile needs no second request).
    let precons = dmu["precons"].as_array().expect("precons");
    assert_eq!(precons.len(), 1, "{precons:?}");
    assert_eq!(precons[0]["slug"], "dummy-universe-commander-dmu");
    assert_eq!(precons[0]["deck_type"], "Commander Deck");
    assert!(precons[0]["face_card"]["card_id"].as_str().is_some());
    let products = dmu["products"].as_array().expect("products");
    let names: Vec<&str> = products
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Dummy Universe Commander Deck",
            "Dummy Universe Draft Booster Box"
        ],
        "name-ascending, the set's own products only"
    );
    assert_eq!(products[0]["id"], "900004");
    assert_eq!(products[0]["set_name"], "Dummy Universe");

    // No Secret Lair cards release in June 2024, so no drops.
    assert_eq!(body["secret_lair_drops"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn release_calendar_lists_one_entry_per_theme() {
    let app = test_app_with_catalog().await;

    // January 2024: `dmb` and its token child `tdmb` share a date. One entry — the child folds
    // into its parent — and `tdmb` never appears on its own.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/releases?from=2024-01-01&to=2024-01-31"),
    )
    .await;
    let codes: Vec<&str> = body["sets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["set"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["dmb"]);

    // December 2019: the `sld` set's own release date. It is never listed as a set — its
    // releases are its drops.
    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/releases?from=2019-12-01&to=2019-12-31"),
    )
    .await;
    assert_eq!(body["sets"].as_array().unwrap().len(), 0, "{body}");
}

#[tokio::test]
async fn release_calendar_classifies_secret_lair_sets_and_drops() {
    let app = test_app_with_catalog().await;

    let (_, _, body) = send(
        &app,
        get("/api/games/mtg/releases?from=2026-09-01&to=2026-09-30"),
    )
    .await;

    // The Zeta stand-in: a top-level `box` set the allow-list admits, upgraded to a Secret
    // Lair release by its `sl` code — as the heads-up would classify it.
    let sets = body["sets"].as_array().expect("sets");
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0]["set"]["code"], "slz");
    assert_eq!(sets[0]["released_at"], "2026-09-02");
    assert_eq!(sets[0]["secret_lair"], true);

    // The `sld` drop whose cards release this month, dated off those cards. It is the only
    // drop: `slz`'s treatment sections are never drops, however the gallery groups them.
    let drops = body["secret_lair_drops"].as_array().expect("drops");
    assert_eq!(drops.len(), 1, "{drops:?}");
    assert_eq!(drops[0]["title"], "Eldraine Wonderland");
    assert_eq!(drops[0]["slug"], "eldraine-wonderland");
    assert_eq!(drops[0]["set_code"], "sld");
    assert_eq!(drops[0]["released_at"], "2026-09-25");
    assert!(drops[0]["products"].is_array());
}

#[tokio::test]
async fn release_calendar_defaults_and_validates_its_window() {
    let app = test_app_with_catalog().await;

    // No bounds: today through today + 90 days, echoed back.
    let (status, _, body) = send(&app, get("/api/games/mtg/releases")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let from: chrono::NaiveDate = body["from"].as_str().unwrap().parse().unwrap();
    let to: chrono::NaiveDate = body["to"].as_str().unwrap().parse().unwrap();
    assert_eq!((to - from).num_days(), 90);

    for (uri, why) in [
        ("/api/games/mtg/releases?from=nope", "a malformed `from`"),
        ("/api/games/mtg/releases?to=2026-1-1", "a malformed `to`"),
        (
            "/api/games/mtg/releases?from=2026-02-01&to=2026-01-31",
            "`to` before `from`",
        ),
        (
            "/api/games/mtg/releases?from=2026-01-01&to=2027-06-01",
            "a window wider than a year",
        ),
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{why}: {uri}");
        assert_eq!(
            cache_control(&headers),
            Some(crate::handlers::cache::NO_STORE),
            "an error is never CDN-pinned: {uri}"
        );
    }

    let (status, _, _) = send(&app, get("/api/games/nope/releases")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
