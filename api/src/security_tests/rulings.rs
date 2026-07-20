//! Public card-rulings ("Notes and Rules Information", issue #522) reads:
//! `/api/games/{game}/cards/{id}/rulings` is publicly readable + shared-cacheable, keyed
//! on the card's `oracle_id` (so every printing returns the same list), ordered oldest
//! first, and an unknown game/card is a `no-store` 404. Drives the real router in-process,
//! seeding card + ruling fixtures straight into the harness DB.

use super::harness::*;
use crate::entities::{card, card_ruling};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, NotSet};

/// Insert a card carrying a specific (optional) `oracle_id`. Rulings join on the
/// oracle id, not the card row, so the card only needs to exist and resolve.
async fn insert_card_with_oracle(
    db: &sea_orm::DatabaseConnection,
    external_id: &str,
    oracle_id: Option<&str>,
) {
    let now = Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        oracle_id: Set(oracle_id.map(str::to_string)),
        name: Set(format!("Card {external_id}")),
        set_code: Set("tst".to_string()),
        set_name: Set("Test Set".to_string()),
        collector_number: Set("1".to_string()),
        lang: Set("en".to_string()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert card");
}

/// Insert one `card_rulings` row for an oracle id.
async fn insert_ruling(
    db: &sea_orm::DatabaseConnection,
    oracle_id: &str,
    source: &str,
    published_at: &str,
    comment: &str,
) {
    let now = Utc::now();
    card_ruling::ActiveModel {
        id: NotSet,
        game: Set("mtg".to_string()),
        oracle_id: Set(oracle_id.to_string()),
        source: Set(source.to_string()),
        published_at: Set(published_at.to_string()),
        comment: Set(comment.to_string()),
        created_at: Set(now),
    }
    .insert(db)
    .await
    .expect("insert ruling");
}

#[tokio::test]
async fn card_rulings_are_publicly_readable_shared_cacheable_and_oldest_first() {
    let app = test_app().await;
    let db = &app.state.db;

    // Two printings share one oracle id; a third card has a different oracle id; a fourth
    // has none at all.
    insert_card_with_oracle(db, "print-a", Some("oracle-1")).await;
    insert_card_with_oracle(db, "print-b", Some("oracle-1")).await;
    insert_card_with_oracle(db, "other", Some("oracle-2")).await;
    insert_card_with_oracle(db, "tokenish", None).await;

    // Insert out of date order to prove the endpoint sorts oldest-first.
    insert_ruling(db, "oracle-1", "wotc", "2021-04-16", "The newer ruling.").await;
    insert_ruling(db, "oracle-1", "wotc", "2019-08-23", "The older ruling.").await;
    insert_ruling(
        db,
        "oracle-2",
        "scryfall",
        "2020-01-01",
        "A different card's ruling.",
    )
    .await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/print-a/rulings")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "card rulings must be browser + CDN cacheable"
    );
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2, "only this oracle id's rulings");
    // Oldest first, and the wire shape carries source / published_at / comment.
    assert_eq!(data[0]["published_at"], "2019-08-23");
    assert_eq!(data[0]["source"], "wotc");
    assert_eq!(data[0]["comment"], "The older ruling.");
    assert_eq!(data[1]["published_at"], "2021-04-16");

    // Every printing of the same card returns the same rulings (keyed on oracle id).
    let (_, _, body_b) = send(&app, get("/api/games/mtg/cards/print-b/rulings")).await;
    assert_eq!(body_b["data"], body["data"]);

    // A card with a different oracle id sees only its own ruling.
    let (_, _, body_other) = send(&app, get("/api/games/mtg/cards/other/rulings")).await;
    assert_eq!(body_other["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        body_other["data"][0]["comment"],
        "A different card's ruling."
    );

    // A card with no oracle id (and one with an oracle id but no rulings) -> a clean,
    // cacheable empty list.
    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/tokenish/rulings")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn unknown_game_and_card_rulings_are_no_store_404s() {
    let app = test_app().await;
    insert_card_with_oracle(&app.state.db, "known", Some("oracle-1")).await;

    for uri in [
        "/api/games/nope/cards/known/rulings",
        "/api/games/mtg/cards/missing/rulings",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "{uri} 404 must be no-store"
        );
    }
}
