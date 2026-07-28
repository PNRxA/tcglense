//! Public card-search `.txt` export — same search as the grid, bounded, and never a
//! silent truncation. Also pins the route shape: `export` is a static segment, so it
//! must never be swallowed by the sibling `/cards/{id}` route.

use super::harness::*;
use crate::test_support::url_encode;

/// Card lines are `1 Name (SET) Number`; the `#` comment (if any) is the truncation note.
fn card_lines(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

#[tokio::test]
async fn export_returns_a_plain_text_attachment_of_every_match() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The JSON listing reports the match count; the export must contain exactly that
    // many rows — the whole result set, not just the first page of 60.
    let (list_status, _, list_body) = send(
        &app,
        get(&format!("/api/games/{game}/cards?page=1&page_size=1")),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    let total = list_body["total"].as_u64().expect("total");
    assert!(total > 1, "dummy catalog should have seeded several cards");

    let (status, headers, body) =
        send_text(&app, get(&format!("/api/games/{game}/cards/export"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-cards.txt\""),
        "the browser must save the file rather than render it",
    );
    let lines = card_lines(&body);
    assert_eq!(lines.len() as u64, total, "every match is exported");
    // Every line is the importable `1 Name (SET) Number` grammar.
    for line in &lines {
        assert!(
            line.starts_with("1 ") && line.contains('(') && line.contains(')'),
            "unexpected export line: {line}"
        );
    }
    // A complete export is pure card lines — the `#` note only appears when truncated.
    assert!(!body.contains('#'), "an untruncated export carries no note");
}

#[tokio::test]
async fn export_honours_the_same_search_and_sort_as_the_listing() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Pick a filter that matches a strict subset, then assert the export and the JSON
    // listing agree on both the membership and the order — the export must be the same
    // query the grid renders, never a second implementation.
    let query = url_encode("t:instant");
    let listing = format!("/api/games/{game}/cards?q={query}&sort=name&dir=asc&page_size=200");
    let (list_status, _, list_body) = send(&app, get(&listing)).await;
    assert_eq!(list_status, StatusCode::OK);
    let names: Vec<String> = list_body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|card| card["name"].as_str().expect("name").to_string())
        .collect();
    assert!(!names.is_empty(), "the filter should match some cards");
    assert!(
        (names.len() as u64) < list_body_total(&app, game).await,
        "the filter should narrow the catalog"
    );

    let (status, _, body) = send_text(
        &app,
        get(&format!(
            "/api/games/{game}/cards/export?q={query}&sort=name&dir=asc"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exported: Vec<String> = card_lines(&body)
        .iter()
        .map(|line| {
            // `1 <name> (SET) <number>` -> the name between the leading count and the set.
            let without_count = line.strip_prefix("1 ").expect("leading quantity");
            without_count[..without_count.rfind(" (").expect("set marker")].to_string()
        })
        .collect();
    assert_eq!(exported, names, "same cards, same order as the grid");
}

/// The unfiltered catalog total, for "the filter actually narrowed things" assertions.
async fn list_body_total(app: &TestApp, game: &str) -> u64 {
    let (_, _, body) = send(app, get(&format!("/api/games/{game}/cards?page_size=1"))).await;
    body["total"].as_u64().expect("total")
}

#[tokio::test]
async fn names_format_dedupes_printings() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The dummy catalog reprints "Dummy Reprinted Relic" across two sets. The default
    // shape lists each printing; `names` folds them to the one card a human would write.
    let query = url_encode("Dummy Reprinted Relic");
    let (_, _, text) = send_text(
        &app,
        get(&format!("/api/games/{game}/cards/export?q={query}")),
    )
    .await;
    assert!(
        card_lines(&text).len() > 1,
        "the default shape keeps every printing: {text}"
    );

    let (status, headers, names) = send_text(
        &app,
        get(&format!(
            "/api/games/{game}/cards/export?q={query}&format=names"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-card-names.txt\""),
    );
    assert_eq!(card_lines(&names), vec!["Dummy Reprinted Relic"]);
}

#[tokio::test]
async fn a_set_export_is_scoped_to_the_set_and_names_it() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    let (list_status, _, list_body) = send(
        &app,
        get(&format!("/api/games/{game}/sets/dmb/cards?page_size=1")),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    let set_total = list_body["total"].as_u64().expect("total");

    let (status, headers, body) = send_text(
        &app,
        get(&format!("/api/games/{game}/sets/dmb/cards/export")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"tcglense-mtg-dmb-cards.txt\""),
    );
    let lines = card_lines(&body);
    assert_eq!(lines.len() as u64, set_total);
    assert!(
        lines.iter().all(|line| line.contains("(DMB)")),
        "every row belongs to the requested set: {body}"
    );

    // The set is resolved before the format, so an unknown set is a 404 even with a
    // bogus `?format=` — a missing thing outranks a malformed param.
    let (nf_status, _, _) = send(
        &app,
        get(&format!(
            "/api/games/{game}/sets/nope/cards/export?format=bogus"
        )),
    )
    .await;
    assert_eq!(nf_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn export_rejects_unknown_games_formats_and_queries() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Unknown game -> 404 (the `export` segment never reaches a card-id lookup).
    let (nf_status, _, _) = send(&app, get("/api/games/nope/cards/export")).await;
    assert_eq!(nf_status, StatusCode::NOT_FOUND);

    // Unknown format -> 422 with a message, not a silent fall back to the default.
    let (bad_format, _, bad_body) = send(
        &app,
        get(&format!("/api/games/{game}/cards/export?format=csv")),
    )
    .await;
    assert_eq!(bad_format, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        bad_body["error"]
            .as_str()
            .is_some_and(|error| error.contains("csv")),
        "the error names the rejected format: {bad_body:?}"
    );

    // A malformed search is a 422 here exactly as it is on the listing, never a 500.
    let (bad_query, _, _) = send(
        &app,
        get(&format!("/api/games/{game}/cards/export?q=boguskey:1")),
    )
    .await;
    assert_eq!(bad_query, StatusCode::UNPROCESSABLE_ENTITY);

    // An injection payload stays a harmless literal name search.
    let injection = url_encode("'; DROP TABLE cards;--");
    let (inj_status, _, _) = send_text(
        &app,
        get(&format!("/api/games/{game}/cards/export?q={injection}")),
    )
    .await;
    assert_eq!(inj_status, StatusCode::OK);
    let (after_status, _, after_body) =
        send(&app, get(&format!("/api/games/{game}/cards?page_size=1"))).await;
    assert_eq!(after_status, StatusCode::OK);
    assert!(after_body["total"].as_u64().is_some_and(|total| total > 0));
}

#[tokio::test]
async fn export_is_a_public_cdn_cacheable_read() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The export is a pure function of public catalog data + the query string, so it
    // sits in the public catalog group and gets that group's shared-cache policy —
    // not `no-store`, and not the holdings policy.
    let (status, headers, _) =
        send_text(&app, get(&format!("/api/games/{game}/cards/export"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
    );
}
