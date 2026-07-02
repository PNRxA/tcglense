//! Shared fixtures for the crate's `#[cfg(test)]` modules: a canonical validated
//! [`Config`], a migrated in-memory SQLite connection, and canonical entity rows
//! (card, set, user, holding). Kept in one place so the unit and integration tests
//! build their state the same way; per-test tweaks use struct-update syntax
//! (`Config { field: …, ..test_config() }`).

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    prelude::DateTimeUtc,
};
use sea_orm_migration::MigratorTrait;

use crate::entities::prelude::CollectionItem;
use crate::entities::{card, card_set, collection_item, user};
use crate::{config::Config, migrator::Migrator};

/// A canonical, fully-populated [`Config`] for tests. All fields carry sane,
/// offline-safe defaults (in-memory DB, no card sync); a test that cares about a
/// particular field overrides just that one via `Config { field: …, ..test_config() }`.
pub(crate) fn test_config() -> Config {
    Config {
        database_url: "sqlite::memory:".to_string(),
        jwt_secret: "integration-test-signing-secret-0123456789".to_string(),
        access_token_expiry_minutes: 15,
        refresh_token_expiry_days: 30,
        cookie_secure: false,
        host: "127.0.0.1".to_string(),
        port: 8080,
        public_site_url: "https://tcglense.example".to_string(),
        data_dir: std::path::PathBuf::from("./data"),
        scryfall_user_agent: "TCGLense/test".to_string(),
        moxfield_user_agent: None,
        sync_on_startup: false,
        sync_interval_hours: 24,
        seed_dummy_data: false,
        cdn_mode: false,
    }
}

/// Connect to a fresh in-memory SQLite database and run all migrations.
///
/// The pool is pinned to a single connection. With `sqlite::memory:` every physical
/// connection is its own separate, empty database, so a multi-connection pool could
/// hand a caller an unmigrated DB; one connection keeps the migrated schema + data
/// consistent across every query (and any future concurrent one).
pub(crate) async fn migrated_memory_db() -> DatabaseConnection {
    // Reuse the app's connect options so tests get the same pragmas and the
    // registered REGEXP function; pin to one connection (see below).
    let mut opts = crate::db::connect_options("sqlite::memory:");
    opts.max_connections(1).min_connections(1);
    let db = Database::connect(opts)
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");
    db
}

/// The canonical all-defaults `mtg` card row (set `tst`, collector number = `id`,
/// fixed 2024-01-01 timestamps, everything else `None`/false). Tests override just
/// their meaningful fields via `card::Model { field: …, ..card_model(id) }`.
pub(crate) fn card_model(id: i32) -> card::Model {
    let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    card::Model {
        id,
        game: "mtg".into(),
        external_id: format!("ext-{id}"),
        oracle_id: None,
        name: format!("Card {id}"),
        set_code: "tst".into(),
        set_name: "TST".into(),
        collector_number: id.to_string(),
        collector_number_int: Some(id),
        rarity: None,
        lang: "en".into(),
        released_at: None,
        mana_cost: None,
        cmc: None,
        type_line: None,
        color_identity: None,
        colors: None,
        layout: None,
        oracle_text: None,
        power: None,
        toughness: None,
        loyalty: None,
        image_small: None,
        image_normal: None,
        image_large: None,
        image_art_crop: None,
        image_png: None,
        card_faces: None,
        price_usd: None,
        price_usd_foil: None,
        price_usd_etched: None,
        price_eur: None,
        price_tix: None,
        keywords: None,
        produced_mana: None,
        color_indicator: None,
        watermark: None,
        flavor_text: None,
        illustration_id: None,
        artist: None,
        artist_ids: None,
        border_color: None,
        frame: None,
        frame_effects: None,
        security_stamp: None,
        promo_types: None,
        finishes: None,
        defense: None,
        legalities: None,
        full_art: None,
        textless: None,
        oversized: None,
        promo: None,
        reprint: None,
        variation: None,
        booster: None,
        story_spotlight: None,
        content_warning: None,
        highres_image: None,
        reserved: None,
        game_changer: None,
        edhrec_rank: None,
        penny_rank: None,
        digital: false,
        created_at: ts,
        updated_at: ts,
    }
}

/// The canonical all-defaults `mtg` set row for `code` (name = upper-cased code,
/// fixed 2024-01-01 timestamps, everything else `None`/zero). Tests override just
/// their meaningful fields via struct-update, like [`card_model`].
pub(crate) fn card_set_model(code: &str) -> card_set::Model {
    let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    card_set::Model {
        id: 0,
        game: "mtg".into(),
        code: code.into(),
        name: code.to_uppercase(),
        set_type: None,
        released_at: None,
        card_count: 0,
        digital: false,
        icon_svg_uri: None,
        parent_set_code: None,
        external_id: None,
        created_at: ts,
        updated_at: ts,
    }
}

/// Insert a user (rows like refresh tokens and collection items FK to `users`) and
/// return its id.
pub(crate) async fn insert_user(db: &DatabaseConnection, email: &str) -> i32 {
    let now = Utc::now();
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set("x".to_string()),
        display_name: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert user")
    .id
}

/// Insert a minimal card and return its internal id.
pub(crate) async fn insert_card(db: &DatabaseConnection, external_id: &str) -> i32 {
    let now = Utc::now();
    let card = card::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(format!("Card {external_id}")),
        set_code: Set("tst".to_string()),
        set_name: Set("Test Set".to_string()),
        collector_number: Set("1".to_string()),
        lang: Set("en".to_string()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    card.insert(db).await.expect("insert card").id
}

/// Insert an owned-card holding for `(user, card)` with the given counts.
pub(crate) async fn insert_holding(
    db: &DatabaseConnection,
    user_id: i32,
    card_id: i32,
    q: i32,
    f: i32,
) {
    let now = Utc::now();
    collection_item::ActiveModel {
        user_id: Set(user_id),
        game: Set(crate::scryfall::GAME.to_string()),
        card_id: Set(card_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert holding");
}

/// The stored `(quantity, foil_quantity)` for one holding, `None` if unowned.
pub(crate) async fn owned_counts(
    db: &DatabaseConnection,
    user_id: i32,
    card_id: i32,
) -> Option<(i32, i32)> {
    CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::CardId.eq(card_id))
        .one(db)
        .await
        .expect("query holding")
        .map(|r| (r.quantity, r.foil_quantity))
}
