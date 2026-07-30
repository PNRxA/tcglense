//! Collection / wish-list card-search `.txt` exports — the authed twins of the public
//! catalog export: auth gating, per-user (and per-surface) isolation, real held counts
//! with the ` *F*` foil marker, same-query-as-the-grid, and the `no-store` policy.

use super::harness::*;
use crate::test_support::url_encode;

/// Card lines are `N Name (SET) Number[ *F*]`; a `#` line only ever marks a failed export.
fn card_lines(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// One seeded card's export-relevant identity: `(external_id, name, SET, number)`.
async fn sample_cards(app: &Router, n: usize) -> Vec<(String, String, String, String)> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(status, StatusCode::OK, "listing seeded cards: {body:?}");
    let data = body["data"].as_array().expect("cards data array");
    assert!(data.len() >= n, "need >= {n} seeded cards");
    data.iter()
        .take(n)
        .map(|c| {
            (
                c["id"].as_str().expect("id").to_string(),
                c["name"].as_str().expect("name").to_string(),
                c["set_code"].as_str().expect("set_code").to_uppercase(),
                c["collector_number"]
                    .as_str()
                    .expect("collector_number")
                    .to_string(),
            )
        })
        .collect()
}

/// Set absolute held counts for one card on either holdings surface.
async fn hold_card(
    app: &Router,
    base: &str,
    token: &str,
    id: &str,
    quantity: i64,
    foil_quantity: i64,
) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/{base}/mtg/cards/{id}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": foil_quantity }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "hold card failed: {body:?}");
}

#[tokio::test]
async fn holdings_card_exports_require_authentication() {
    let app = test_app_with_catalog().await;

    for base in ["collection", "wishlist"] {
        // Per-user data: no bearer token -> 401, and never shared-cached.
        let (status, headers, _) = send(&app, get(&format!("/api/{base}/mtg/cards/export"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{base} export");
        assert_eq!(cache_control(&headers), Some("no-store"));
    }
}

#[tokio::test]
async fn collection_export_is_the_callers_cards_with_real_counts() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "exporter@example.com", "password123").await;
    let (other_token, _) = register(&app, "other@example.com", "password123").await;

    let cards = sample_cards(&app, 3).await;
    let (id0, name0, set0, num0) = &cards[0];
    let (id1, name1, set1, num1) = &cards[1];
    let (id2, _, set2, num2) = &cards[2];

    // The caller owns two cards (one with a foil split, one foil-only); somebody else
    // owns a third that must never leak into the caller's file.
    hold_card(&app, "collection", &token, id0, 2, 1).await;
    hold_card(&app, "collection", &token, id1, 0, 3).await;
    hold_card(&app, "collection", &other_token, id2, 5, 0).await;

    let (status, headers, body) = send_text(
        &app,
        get_with_bearer("/api/collection/mtg/cards/export", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-collection-cards.txt\""),
        "the browser must save the file rather than render it",
    );
    // Per-user data: never shared-cached, and streamed (chunk-framed, no validator).
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(headers.get("content-length"), None);
    assert_eq!(headers.get("etag"), None);

    // Default order is the listing's recency order (most recently updated first): the
    // foil-only card was written last, so its single ` *F*` line leads; the split
    // holding renders its regular line then its foil line.
    assert_eq!(
        card_lines(&body),
        vec![
            format!("3 {name1} ({set1}) {num1} *F*"),
            format!("2 {name0} ({set0}) {num0}"),
            format!("1 {name0} ({set0}) {num0} *F*"),
        ],
        "exactly the caller's holdings, real counts, foil tagged"
    );
    // The other user's card is absent — the export is scoped by the token's user.
    assert!(
        !body.contains(&format!("({set2}) {num2}")),
        "another user's holding leaked into the export: {body}"
    );
}

#[tokio::test]
async fn collection_export_honours_the_same_search_scope_and_sort_as_the_listing() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "search-export@example.com", "password123").await;

    for (id, _, _, _) in sample_cards(&app, 8).await {
        hold_card(&app, "collection", &token, &id, 1, 0).await;
    }

    // Name-sorted export vs the identically-parameterised JSON listing: same rows,
    // same order — the export must be the very query the grid ran.
    let (status, _, listing) = send(
        &app,
        get_with_bearer(
            "/api/collection/mtg?sort=name&dir=asc&page_size=200",
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected: Vec<String> = listing["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|entry| {
            format!(
                "1 {} ({}) {}",
                entry["card"]["name"].as_str().expect("name"),
                entry["card"]["set_code"]
                    .as_str()
                    .expect("set_code")
                    .to_uppercase(),
                entry["card"]["collector_number"]
                    .as_str()
                    .expect("collector_number"),
            )
        })
        .collect();
    assert_eq!(expected.len(), 8);

    let (status, _, body) = send_text(
        &app,
        get_with_bearer("/api/collection/mtg/cards/export?sort=name&dir=asc", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        card_lines(&body),
        expected,
        "same cards, same order as the listing"
    );

    // A `q` filter narrows the export exactly as it narrows the listing.
    let first_name = listing["data"][0]["card"]["name"].as_str().expect("name");
    let query = url_encode(first_name);
    let (_, _, filtered_listing) = send(
        &app,
        get_with_bearer(
            &format!("/api/collection/mtg?q={query}&page_size=200"),
            &token,
        ),
    )
    .await;
    let filtered_total = filtered_listing["total"].as_u64().expect("total");
    assert!(filtered_total >= 1 && filtered_total < 8, "q must narrow");
    let (status, _, filtered) = send_text(
        &app,
        get_with_bearer(
            &format!("/api/collection/mtg/cards/export?q={query}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card_lines(&filtered).len() as u64, filtered_total);

    // A `?set=` scope keeps only that set's holdings, mirroring the listing's scope.
    let a_set = listing["data"][0]["card"]["set_code"]
        .as_str()
        .expect("set_code")
        .to_string();
    let (_, _, scoped_listing) = send(
        &app,
        get_with_bearer(
            &format!("/api/collection/mtg?set={a_set}&page_size=200"),
            &token,
        ),
    )
    .await;
    let scoped_total = scoped_listing["total"].as_u64().expect("total");
    assert!(scoped_total >= 1);
    let (status, _, scoped) = send_text(
        &app,
        get_with_bearer(
            &format!("/api/collection/mtg/cards/export?set={a_set}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let scoped_lines = card_lines(&scoped);
    assert_eq!(scoped_lines.len() as u64, scoped_total);
    let marker = format!("({})", a_set.to_uppercase());
    assert!(
        scoped_lines.iter().all(|line| line.contains(&marker)),
        "every exported row belongs to the scoped set: {scoped}"
    );
}

#[tokio::test]
async fn wishlist_export_mirrors_the_twin_and_stays_per_surface() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "wanter@example.com", "password123").await;

    let cards = sample_cards(&app, 2).await;
    let (id0, name0, set0, num0) = &cards[0];
    let (id1, _, set1, num1) = &cards[1];

    // Want one card; *own* (collection) another. The two surfaces are independent
    // tables, so the wish-list export must carry only the wanted card.
    hold_card(&app, "wishlist", &token, id0, 1, 2).await;
    hold_card(&app, "collection", &token, id1, 4, 0).await;

    let (status, headers, body) = send_text(
        &app,
        get_with_bearer("/api/wishlist/mtg/cards/export", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-wishlist-cards.txt\""),
    );
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(
        card_lines(&body),
        vec![
            format!("1 {name0} ({set0}) {num0}"),
            format!("2 {name0} ({set0}) {num0} *F*"),
        ],
    );
    assert!(
        !body.contains(&format!("({set1}) {num1}")),
        "a collection-only holding leaked into the wish-list export: {body}"
    );
}

#[tokio::test]
async fn names_format_dedupes_wanted_printings() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "names@example.com", "password123").await;

    // The dummy catalog reprints "Dummy Reprinted Relic" across two sets; want both
    // printings, then ask for the de-duplicated names shape.
    let query = url_encode("Dummy Reprinted Relic");
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/mtg/cards?q={query}&page_size=10")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let printings: Vec<String> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|c| c["id"].as_str().expect("id").to_string())
        .collect();
    assert!(printings.len() > 1, "the fixture reprint must span sets");
    for id in &printings {
        hold_card(&app, "wishlist", &token, id, 1, 0).await;
    }

    let (status, headers, names) = send_text(
        &app,
        get_with_bearer("/api/wishlist/mtg/cards/export?format=names", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-wishlist-card-names.txt\""),
    );
    assert_eq!(card_lines(&names), vec!["Dummy Reprinted Relic"]);
}

#[tokio::test]
async fn an_empty_holdings_export_is_an_empty_file() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "empty@example.com", "password123").await;

    for base in ["collection", "wishlist"] {
        let (status, _, body) = send_text(
            &app,
            get_with_bearer(&format!("/api/{base}/mtg/cards/export"), &token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{base} export");
        assert_eq!(
            body, "",
            "nothing held exports as an empty file, not an error"
        );
    }
}

#[tokio::test]
async fn holdings_export_rejects_unknown_games_formats_and_queries() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "rejects@example.com", "password123").await;

    // Unknown game -> 404 (the `export` segment never reaches a card-id lookup).
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/nope/cards/export", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for base in ["collection", "wishlist"] {
        // Unknown format -> 422 with a message, not a silent fall back to the default.
        let (status, _, body) = send(
            &app,
            get_with_bearer(&format!("/api/{base}/mtg/cards/export?format=csv"), &token),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{base} format");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains("csv")),
            "the error names the rejected format: {body:?}"
        );

        // A malformed search is a 422 here exactly as it is on the listing, never a 500.
        let (status, _, _) = send(
            &app,
            get_with_bearer(
                &format!("/api/{base}/mtg/cards/export?q=boguskey:1"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{base} query");

        // An unknown sort key is a 422 too, mirroring the listing.
        let (status, _, _) = send(
            &app,
            get_with_bearer(&format!("/api/{base}/mtg/cards/export?sort=bogus"), &token),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{base} sort");
    }
}
