//! Visual card scanner (`POST /api/games/{game}/scan`): auth gating, `no-store`,
//! fingerprint validation, the empty-index and beyond-radius paths, and a real match
//! round-trip — all driving the real router in-process over the seeded catalog.

use super::harness::*;
use crate::catalog::fingerprints;

/// The scanner algo version in the test config (see `test_support::test_config`).
const ALGO_VERSION: i32 = 1;

fn scan_body(fingerprint: &[u8]) -> Value {
    json!({ "fingerprints": [fingerprint] })
}

/// One real seeded card external id.
async fn one_card_id(app: &Router) -> String {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=1")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "listing seeded cards failed: {body:?}"
    );
    body["data"][0]["id"].as_str().expect("card id").to_string()
}

/// Insert a fingerprint for `external_id` and load it into the live match index (the
/// in-process harness never runs `tasks::start`, so the index starts empty).
async fn index_card(app: &TestApp, external_id: &str, hash: &[u8]) {
    fingerprints::upsert(
        &app.state.db,
        "mtg",
        external_id,
        0,
        ALGO_VERSION,
        hash,
        "small",
        "test-source-hash",
    )
    .await
    .expect("insert fingerprint");
    let index = fingerprints::load_index(&app.state.db, ALGO_VERSION)
        .await
        .expect("load index");
    app.state.set_fingerprint_index(index);
}

#[tokio::test]
async fn scan_requires_authentication() {
    let app = test_app_with_catalog().await;
    // No bearer token -> 401, and the response must never be shared-cached.
    let (status, headers, _) = send(
        &app,
        json_post("/api/games/mtg/scan", scan_body(&[0u8; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn scan_rejects_a_wrong_length_fingerprint() {
    // The length check fires before the index check, so no index is needed: a 16-byte
    // fingerprint (half a hash) is a 422, never a 500 or a silent mismatch.
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let (status, headers, body) = send(
        &app,
        json_with_bearer("POST", "/api/games/mtg/scan", &token, scan_body(&[0u8; 16])),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn scan_rejects_an_empty_fingerprint_list() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/games/mtg/scan",
            &token,
            json!({ "fingerprints": [] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body:?}");
}

#[tokio::test]
async fn scan_matches_across_variant_fingerprints() {
    // The client sends several variant hashes of one card; the card's distance is the
    // MIN across them. A far variant plus the exact hash must still match at distance 0.
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let id = one_card_id(&app).await;
    let hash = [0x5Au8; 32];
    index_card(&app, &id, &hash).await;

    let far = [0xFFu8; 32];
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/games/mtg/scan",
            &token,
            json!({ "fingerprints": [far, hash] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let data = body["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["card"]["id"].as_str().unwrap(), id);
    assert_eq!(data[0]["distance"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn scan_without_an_index_is_404() {
    // A valid, well-formed request but no fingerprint index built/imported yet: a 404
    // that is distinct from "matched nothing" (an empty 200), so the client can tell
    // "scanner unavailable here" apart from "card not recognised".
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let (status, headers, body) = send(
        &app,
        json_with_bearer("POST", "/api/games/mtg/scan", &token, scan_body(&[0u8; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn scan_matches_the_indexed_card() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let id = one_card_id(&app).await;

    let hash = [0xA5u8; 32];
    index_card(&app, &id, &hash).await;

    // The exact hash -> distance 0, and the top (only) match is that card, in the full
    // catalog `Card` shape.
    let (status, headers, body) = send(
        &app,
        json_with_bearer("POST", "/api/games/mtg/scan", &token, scan_body(&hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    let data = body["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["card"]["id"].as_str().unwrap(), id);
    assert_eq!(data[0]["distance"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn scan_returns_no_match_beyond_the_radius() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "scanner@example.com", "password123").await;
    let id = one_card_id(&app).await;
    index_card(&app, &id, &[0x00u8; 32]).await;

    // A hash the full 256 bits away (all ones vs all zeros) is far beyond the confidence
    // radius, so it resolves to no match (an empty 200) rather than a distant false hit.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/games/mtg/scan",
            &token,
            scan_body(&[0xFFu8; 32]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert!(body["data"].as_array().expect("data array").is_empty());
}
