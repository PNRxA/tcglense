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
use crate::entities::{card, card_set, collection_item, product, user};
use crate::{config::Config, migrator::Migrator};

/// A canonical, fully-populated [`Config`] for tests. Every field carries a sane,
/// offline-safe default (in-memory DB, no card sync, no email key); a test that cares
/// about a particular field overrides just that one via
/// `Config { field: …, ..test_config() }`.
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
        tcgcsv_user_agent: "TCGLense/test".to_string(),
        price_backfill_enabled: false,
        price_backfill_days: 0,
        moxfield_user_agent: None,
        resend_api_key: None,
        email_from: "TCGLense <test@tcglense.example>".to_string(),
        turnstile_secret_key: None,
        turnstile_site_key: None,
        trust_proxy_headers: false,
        rate_limit_enabled: true,
        redis_url: None,
        sync_on_startup: false,
        sync_interval_hours: 24,
        seed_dummy_data: false,
        cdn_mode: false,
        web_root: None,
        prerender_all_user_agents: false,
        sync_from_upstream: false,
        dataset_mirror_url: "https://tcglense.example".to_string(),
        mirror_enabled: false,
        signups_enabled: true,
        signups_disabled_message: None,
        fingerprint_build_enabled: false,
        fingerprint_algo_version: 1,
        fingerprint_top_k: 8,
        fingerprint_max_distance: 96,
        fingerprint_import_enabled: false,
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
        tcgplayer_id: None,
        tcgplayer_etched_id: None,
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
        password_hash: Set(Some("x".to_string())),
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

/// Insert a sealed product and return its internal id. Mirrors [`insert_card`] for the
/// products catalog: fixed defaults, per-test tweaks passed as arguments.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_product(
    db: &DatabaseConnection,
    external_id: &str,
    name: &str,
    set_code: &str,
    product_type: &str,
    usd: Option<&str>,
) -> i32 {
    let now = Utc::now();
    product::ActiveModel {
        game: Set(crate::scryfall::GAME.to_string()),
        external_id: Set(external_id.to_string()),
        name: Set(name.to_string()),
        clean_name: Set(Some(name.to_string())),
        set_code: Set(set_code.to_string()),
        product_type: Set(product_type.to_string()),
        url: Set(Some(format!("https://www.tcgplayer.com/product/{external_id}"))),
        image_url: Set(Some(format!(
            "https://tcgplayer-cdn.tcgplayer.com/product/{external_id}_200w.jpg"
        ))),
        price_usd: Set(usd.map(str::to_string)),
        price_usd_foil: Set(None),
        released_at: Set(Some("2024-02-09".to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert product")
    .id
}

/// Set an existing product's curated MSRP (retail price) by external (TCGplayer) id. The
/// ingest normally sources this from the committed `msrp.json`; tests set it directly to
/// exercise the wire field without threading it through every [`insert_product`] call.
pub(crate) async fn set_product_msrp(db: &DatabaseConnection, external_id: &str, msrp: &str) {
    let id = product::Entity::find()
        .filter(product::Column::ExternalId.eq(external_id))
        .one(db)
        .await
        .expect("query product")
        .expect("product exists")
        .id;
    product::ActiveModel {
        id: Set(id),
        msrp: Set(Some(msrp.to_string())),
        ..Default::default()
    }
    .update(db)
    .await
    .expect("set product msrp");
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

/// Percent-encode a query value for a hand-built test request URI: everything
/// outside the RFC 3986 unreserved set (spaces, quotes, and the Scryfall operators
/// `!"…"`, `/…/`, `:`, `>`) is `%`-escaped. Shared by the search security tests and
/// the Postgres integration harness so both build request URIs the same way.
pub(crate) fn url_encode(input: &str) -> String {
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
