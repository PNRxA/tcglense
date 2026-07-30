//! Public per-card art-tag reads: `/api/games/{game}/cards/{id}/art-tags` is publicly
//! readable + shared-cacheable, keyed on the card's `illustration_id` (so every printing
//! of the same painting returns the same list), ordered rarest-tag first, and an unknown
//! game/card is a `no-store` 404. Drives the real router in-process, seeding card +
//! art-tag fixtures straight into the harness DB.

use super::harness::*;
use crate::entities::{art_tag, card, card_art_tag};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, NotSet};

/// Insert a card carrying a specific (optional) `illustration_id`. Art tags join on the
/// artwork id, not the card row, so the card only needs to exist and resolve.
async fn insert_card_with_illustration(
    db: &sea_orm::DatabaseConnection,
    external_id: &str,
    illustration_id: Option<&str>,
) {
    let now = Utc::now();
    card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        illustration_id: Set(illustration_id.map(str::to_string)),
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

/// Insert an `art_tags` metadata row plus its `card_art_tags` mapping onto one artwork —
/// the shape the ingest leaves behind after hierarchy expansion.
async fn insert_tag(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    label: &str,
    taggings_count: i32,
    illustration_id: &str,
) {
    art_tag::ActiveModel {
        id: NotSet,
        game: Set("mtg".to_string()),
        scryfall_id: Set(format!("tag-{slug}")),
        slug: Set(slug.to_string()),
        label: Set(label.to_string()),
        description: Set(None),
        taggings_count: Set(taggings_count),
        created_at: Set(Utc::now()),
    }
    .insert(db)
    .await
    .expect("insert art tag");

    card_art_tag::ActiveModel {
        id: NotSet,
        game: Set("mtg".to_string()),
        tag_slug: Set(slug.to_string()),
        illustration_id: Set(illustration_id.to_string()),
    }
    .insert(db)
    .await
    .expect("insert art tag mapping");
}

#[tokio::test]
async fn card_art_tags_are_publicly_readable_shared_cacheable_and_specific_first() {
    let app = test_app().await;
    let db = &app.state.db;

    // Two printings reuse one painting; a third card has its own; a fourth has no
    // artwork identity at all.
    insert_card_with_illustration(db, "print-a", Some("art-1")).await;
    insert_card_with_illustration(db, "print-b", Some("art-1")).await;
    insert_card_with_illustration(db, "other", Some("art-2")).await;
    insert_card_with_illustration(db, "artless", None).await;

    // Insert broadest-first to prove the endpoint re-orders by how many artworks each
    // tag covers — the hierarchy ancestor (`animal`) must land behind the specific tag.
    insert_tag(db, "animal", "Animal", 900, "art-1").await;
    insert_tag(db, "squirrel", "Squirrel", 12, "art-1").await;
    insert_tag(db, "forest", "Forest", 400, "art-2").await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/print-a/art-tags")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "card art tags must be browser + CDN cacheable"
    );
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2, "only this artwork's tags");
    // Rarest (most specific) first, and the wire shape is the shared ArtTagEntry.
    assert_eq!(data[0]["slug"], "squirrel");
    assert_eq!(data[0]["label"], "Squirrel");
    assert_eq!(data[0]["count"], 12);
    assert_eq!(data[1]["slug"], "animal");

    // Every printing of the same painting returns the same tags (keyed on artwork).
    let (_, _, body_b) = send(&app, get("/api/games/mtg/cards/print-b/art-tags")).await;
    assert_eq!(body_b["data"], body["data"]);

    // A card with a different artwork sees only its own tag.
    let (_, _, body_other) = send(&app, get("/api/games/mtg/cards/other/art-tags")).await;
    assert_eq!(body_other["data"].as_array().unwrap().len(), 1);
    assert_eq!(body_other["data"][0]["slug"], "forest");

    // A card with no artwork identity -> a clean, cacheable empty list, not a 404.
    let (status, headers, body) = send(&app, get("/api/games/mtg/cards/artless/art-tags")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn unknown_game_and_card_art_tags_are_no_store_404s() {
    let app = test_app().await;
    insert_card_with_illustration(&app.state.db, "known", Some("art-1")).await;

    for uri in [
        "/api/games/nope/cards/known/art-tags",
        "/api/games/mtg/cards/missing/art-tags",
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
