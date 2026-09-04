//! The card preview (`GET /api/games/{game}/cards/preview`): the first rows of a card
//! search without the listing's `COUNT(*)`, for a surface that shows a handful of cards and
//! never reads the total (the keyword glossary's example panel). Drives the real router over
//! the seeded dummy catalog plus a few hand-inserted keyworded cards.
//!
//! What these pin: the public catalog cache posture; that the preview is the listing's own
//! first page (same filter, same sort, same rows in the same order) so the two can never
//! disagree; that `has_more` is honest from one row of over-fetch and the body carries no
//! `total`; that `limit` clamps; and that a bad game / query / sort answer the listing's own
//! status codes.

use sea_orm::{ActiveModelTrait, Set};

use super::harness::*;
use crate::entities::card;
use crate::handlers::cache::PUBLIC_CATALOG_CACHE;
use crate::test_support::url_encode;

/// Insert a card carrying `keywords` at a given price. The seed's cards carry no keyword
/// abilities, so the `kw:` cases need rows of their own.
async fn insert_keyworded_card(app: &TestApp, external_id: &str, keywords: &str, price: &str) {
    let now = chrono::Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(format!("Keyworded {external_id}")),
        set_code: Set("kwt".to_string()),
        set_name: Set("Keyword Test".to_string()),
        collector_number: Set(external_id.to_string()),
        lang: Set("en".to_string()),
        digital: Set(false),
        keywords: Set(Some(keywords.to_string())),
        price_usd: Set(Some(price.to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .expect("insert card");
}

fn ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|row| row["id"].as_str().expect("id").to_string())
        .collect()
}

#[tokio::test]
async fn preview_is_public_and_shared_cacheable() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    let (status, headers, body) =
        send(&app, get(&format!("/api/games/{game}/cards/preview"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(PUBLIC_CATALOG_CACHE),
        "the same for every visitor, so a CDN may store it"
    );
    assert!(headers.contains_key("etag"), "carries a validator");

    // The default handful, with more behind it — and no count of what's behind it.
    assert_eq!(body["data"].as_array().expect("array").len(), 8);
    assert_eq!(body["has_more"], true);
    assert!(
        body.get("total").is_none() && body.get("page").is_none(),
        "a preview is a SearchGroup, not a Page: {body}"
    );
}

#[tokio::test]
async fn preview_is_the_listings_first_rows_in_the_listings_order() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;
    insert_keyworded_card(&app, "kw-1", "Flying,Trample", "3.00").await;
    insert_keyworded_card(&app, "kw-2", "Flying", "12.50").await;
    insert_keyworded_card(&app, "kw-3", "Trample", "9.99").await;
    insert_keyworded_card(&app, "kw-4", "Flying,Haste", "0.25").await;

    // The glossary panel's exact request: priced kw: matches, priciest first.
    let q = url_encode("kw:\"Flying\"");
    let (status, _, preview) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards/preview?q={q}&sort=price&dir=desc&limit=2"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        ids(&preview),
        vec!["kw-2", "kw-1"],
        "priciest Flying cards first"
    );
    assert_eq!(preview["has_more"], true, "kw-4 is still behind the cut");

    // The listing's own page 1 for the same filter + sort: identical rows, identical order,
    // and it agrees there is more — the two are one query with different row caps.
    let (status, _, page) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards?q={q}&sort=price&dir=desc&page_size=2"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(ids(&page), ids(&preview));
    assert_eq!(page["has_more"], preview["has_more"]);
    assert_eq!(
        page["total"], 3,
        "the listing still counts; the preview never does"
    );

    // A wider cut past the last match: everything, nothing withheld.
    let (_, _, all) = send(
        &app,
        get(&format!(
            "/api/games/{game}/cards/preview?q={q}&sort=price&dir=desc&limit=5"
        )),
    )
    .await;
    assert_eq!(ids(&all), vec!["kw-2", "kw-1", "kw-4"]);
    assert_eq!(all["has_more"], false);

    // The exact-name scope the listing honours is honoured here too.
    let name = url_encode("Keyworded kw-3");
    let (_, _, named) = send(
        &app,
        get(&format!("/api/games/{game}/cards/preview?name={name}")),
    )
    .await;
    assert_eq!(ids(&named), vec!["kw-3"]);
    assert_eq!(named["has_more"], false);
}

#[tokio::test]
async fn preview_limit_clamps_and_has_more_is_honest() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // Below the floor: one row, not an empty (or unbounded) answer.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/cards/preview?limit=0")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["has_more"], true);

    // Above the ceiling: capped at the maximum, never a whole-catalog dump.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/{game}/cards/preview?limit=100000")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["data"].as_array().unwrap().len(),
        crate::handlers::catalog::MAX_PREVIEW_ROWS as usize
    );

    // A filter matching nothing: empty, and honestly so.
    let q = url_encode("kw:\"No Such Keyword\"");
    let (status, _, body) =
        send(&app, get(&format!("/api/games/{game}/cards/preview?q={q}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].as_array().unwrap().is_empty());
    assert_eq!(body["has_more"], false);
}

#[tokio::test]
async fn preview_answers_the_listings_errors() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    let (status, headers, _) = send(&app, get("/api/games/nope/cards/preview")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        cache_control(&headers),
        Some("no-store"),
        "errors are never cached"
    );

    // A malformed query and an unknown sort are the listing's 422s, not 500s.
    let q = url_encode("artists:abc");
    let (status, _, _) = send(&app, get(&format!("/api/games/{game}/cards/preview?q={q}"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = send(
        &app,
        get(&format!("/api/games/{game}/cards/preview?sort=sideways")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
