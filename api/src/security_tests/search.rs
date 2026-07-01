//! Public search route — injection-safe, malformed -> 422 (not 500).

use super::harness::*;

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

/// Percent-encode a query value (only what these tests need: the injection chars).
fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
