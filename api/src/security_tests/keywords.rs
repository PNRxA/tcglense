//! The public rules-keyword glossary read (`/api/games/{game}/keywords`): served without
//! auth, shared-cacheable and `ETag`-validated like every other catalog read, honest about
//! an unknown game, and shaped the way the SPA's text matcher expects. Drives the real
//! router in-process.
//!
//! The glossary is a static table rather than a query, so these cases are about the
//! *contract* — cache posture, wire shape, and the invariants the SPA depends on. The
//! table's own well-formedness is asserted in `crate::catalog::keywords`.

use super::harness::*;
use crate::handlers::cache::PUBLIC_CATALOG_CACHE;

#[tokio::test]
async fn keyword_glossary_is_public_and_shared_cacheable() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    let (status, headers, body) = send(&app, get(&format!("/api/games/{game}/keywords"))).await;
    assert_eq!(status, StatusCode::OK);
    // Public catalog posture: a CDN may store it, and it carries a validator so a stale
    // entry revalidates with headers instead of the whole body.
    assert_eq!(
        headers.get("cache-control").unwrap().to_str().unwrap(),
        PUBLIC_CATALOG_CACHE
    );
    assert!(headers.contains_key("etag"), "should carry an ETag");

    let entries = body["data"].as_array().expect("data array");
    assert!(
        entries.len() > 200,
        "expected the full glossary, got {}",
        entries.len()
    );
}

#[tokio::test]
async fn keyword_entries_carry_everything_the_spa_needs() {
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/keywords"))).await;
    assert_eq!(status, StatusCode::OK);
    let entries = body["data"].as_array().expect("data array");

    let vigilance = entries
        .iter()
        .find(|entry| entry["name"] == "Vigilance")
        .expect("Vigilance should be in the glossary");
    assert_eq!(vigilance["slug"], "vigilance");
    assert_eq!(vigilance["kind"], "ability");
    assert_eq!(vigilance["match_mode"], "anywhere");
    assert_eq!(vigilance["parameterized"], false);
    assert!(
        vigilance["text"].as_str().unwrap().len() > 20,
        "{vigilance:?}"
    );

    // A parameterised keyword and one of each remaining kind, so the enums are actually
    // exercised on the wire rather than only in Rust.
    let ward = entries.iter().find(|e| e["name"] == "Ward").expect("Ward");
    assert_eq!(ward["parameterized"], true);
    assert!(entries.iter().any(|e| e["kind"] == "action"));
    assert!(entries.iter().any(|e| e["kind"] == "ability_word"));

    // Ordered by name, so the SPA's A-Z index needs no re-sort.
    let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "glossary should arrive name-ordered");
}

#[tokio::test]
async fn keyword_glossary_needs_no_catalog_data() {
    // `test_app` seeds no cards: the glossary must still answer in full. This is what
    // lets a fresh self-host explain card text before its first sync completes.
    let game = crate::scryfall::GAME;
    let app = test_app().await;

    let (status, _, body) = send(&app, get(&format!("/api/games/{game}/keywords"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body["data"].as_array().expect("data").is_empty());
}

#[tokio::test]
async fn unknown_game_is_a_no_store_404() {
    let app = test_app().await;

    let (status, headers, _) = send(&app, get("/api/games/nope/keywords")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A negative result must never be pinned by a shared cache.
    assert_eq!(
        headers.get("cache-control").unwrap().to_str().unwrap(),
        "no-store"
    );
}
