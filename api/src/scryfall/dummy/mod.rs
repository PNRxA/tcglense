//! Deterministic dummy MTG catalog for offline development, CI, and tests.
//!
//! When `SEED_DUMMY_DATA` is set the server seeds this fabricated catalog instead
//! of streaming Scryfall's ~550 MB bulk file — no network, no images. The fake
//! sets/cards are built as the same `ScryfallSet`/`ScryfallCard` shapes the real
//! importer consumes and then run through the *exact* `ingest::import_sets` /
//! `map::map_card` / `ingest::flush_cards` / `ingest::put_state` path, so seeded
//! rows are byte-identical in shape to production rows (collector-number sort key,
//! comma-joined colours, faces JSON) and no upsert column list is duplicated.
//!
//! Everything is **deterministic**: ids, set codes, and collector numbers are fixed
//! (no clock-derived identities), and the fabricated year of daily price history is a
//! per-card *seeded* random walk (the RNG is seeded from the card id), so it looks
//! random yet reseeds to byte-identical values. The upserts key on
//! `(game, external_id)` / `(game, code)`, so re-seeding on every boot overwrites the
//! same rows rather than growing the catalog. Cards carry no image URLs, so the image
//! proxy is never hit and `has_image` resolves to false everywhere.
//!
//! The fabricated data lives in [`catalog`]; the price random walk in [`prices`];
//! this module orchestrates seeding it into the database.

mod catalog;
mod prices;
mod products;

use chrono::{Duration, Utc};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
    sea_query::OnConflict,
};

use super::GAME;
use super::ingest::{self, IngestError};
use super::map;
use super::price_history;
use crate::entities::prelude::{
    ArtTag, Card, CardArtTag, CardRuling, Product, ProductPriceHistory, SealedComponent,
    SealedContent,
};
use crate::entities::sealed_component::ComponentKind;
use crate::entities::sealed_content::Membership;
use crate::entities::{
    art_tag, card, card_art_tag, card_price_history, card_ruling, product, product_price_history,
    sealed_component, sealed_content,
};
use catalog::{dummy_cards, dummy_sets};
use prices::price_walk;
use products::dummy_products;

/// Synthetic `ingest_state.source_updated_at` recorded for a dummy seed. It never
/// equals a real Scryfall RFC3339 timestamp, so a later real sync's version gate
/// (`scryfall::ingest::refresh`) sees a mismatch and re-imports — dummy mode never
/// locks out a switch back to real data. The seed runs unconditionally on every boot,
/// so changing the generated data takes effect on the next restart with no version
/// bump needed; this value only needs to stay distinct from a real Scryfall timestamp.
const DUMMY_SOURCE_VERSION: &str = "dummy-seed-v1";

/// A year of fabricated price history seeded per card (one row per day, ending today),
/// so the chart shows a year of movement rather than a single flat point.
const PRICE_HISTORY_DAYS: i64 = 365;

/// Seed `PRICE_HISTORY_DAYS` of fabricated daily price history per seeded card,
/// reusing the real `(game, card_id, as_of_date)` unique key and upsert helper. Reads
/// the just-seeded `cards` rows for their ids and base prices (the same shape the live
/// `snapshot_prices` reads), then writes one walked row per card per day ending today.
/// Returns the number of rows written.
///
/// A reseed on the **same day** is byte-identical: the same dates and per-card seed
/// reproduce the same values, and the upsert preserves `created_at`. Like the dummy
/// seed generally, this is upsert-only (never deletes), and the window ends at "today",
/// so a long-lived on-disk dummy DB rebooted on a *later* calendar day re-stamps the
/// shifted older dates with fresh walk values and leaves rows past the year mark in
/// place — harmless drift on fabricated offline data; point `SEED_DUMMY_DATA` at a
/// fresh/dedicated DB as the module already advises.
async fn seed_price_history(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let cards = price_history::load_price_columns(db, GAME).await?;

    let today = Utc::now().date_naive();
    let now = Utc::now();
    let days = PRICE_HISTORY_DAYS as usize;
    let mut models: Vec<card_price_history::ActiveModel> = Vec::with_capacity(cards.len() * days);
    for (card_id, usd, usd_foil, eur, tix) in &cards {
        // Seed the walk from the card id so every card has its own reproducible series
        // (independent of iteration order) and a reseed upserts identical values.
        let mut rng = StdRng::seed_from_u64(*card_id as u64);
        let usd_series = price_walk(usd, &mut rng, days);
        let foil_series = price_walk(usd_foil, &mut rng, days);
        let eur_series = price_walk(eur, &mut rng, days);
        let tix_series = price_walk(tix, &mut rng, days);
        for d in 0..days {
            let as_of = price_history::format_date(today - Duration::days(d as i64));
            models.push(card_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                card_id: Set(*card_id),
                as_of_date: Set(as_of),
                price_usd: Set(usd_series[d].clone()),
                price_usd_foil: Set(foil_series[d].clone()),
                price_eur: Set(eur_series[d].clone()),
                price_tix: Set(tix_series[d].clone()),
                created_at: Set(now),
            });
        }
    }

    let total = models.len() as u64;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card_price_history::ActiveModel> = iter
            .by_ref()
            .take(price_history::PRICE_HISTORY_BATCH)
            .collect();
        if chunk.is_empty() {
            break;
        }
        price_history::upsert_price_history(db, chunk).await?;
    }
    Ok(total)
}

/// Seed the fabricated sealed products (upsert on `(game, external_id)`), returning
/// the number written. Idempotent like the card seed.
async fn seed_products(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let models = dummy_products();
    let total = models.len() as u64;
    Product::insert_many(models)
        .on_conflict(
            OnConflict::columns([product::Column::Game, product::Column::ExternalId])
                .update_columns([
                    product::Column::Name,
                    product::Column::CleanName,
                    product::Column::SetCode,
                    product::Column::ProductType,
                    product::Column::Url,
                    product::Column::ImageUrl,
                    product::Column::PriceUsd,
                    product::Column::PriceUsdFoil,
                    product::Column::ReleasedAt,
                    product::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(total)
}

/// Seed a year of fabricated daily price history per seeded product, mirroring
/// [`seed_price_history`] over the `products` rows (USD + foil only). Reads the
/// just-seeded products for their ids + base prices, walks each per-product seeded
/// series, and upserts on `(game, product_id, as_of_date)`.
async fn seed_product_price_history(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let products: Vec<(i32, Option<String>, Option<String>)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::PriceUsd)
        .column(product::Column::PriceUsdFoil)
        .filter(product::Column::Game.eq(GAME))
        .into_tuple()
        .all(db)
        .await?;

    let today = Utc::now().date_naive();
    let now = Utc::now();
    let days = PRICE_HISTORY_DAYS as usize;
    let mut models: Vec<product_price_history::ActiveModel> =
        Vec::with_capacity(products.len() * days);
    for (product_id, usd, usd_foil) in &products {
        // Seed off the product id (offset so it doesn't collide with a card id's walk).
        let mut rng = StdRng::seed_from_u64(*product_id as u64 ^ 0x5EA1ED);
        let usd_series = price_walk(usd, &mut rng, days);
        let foil_series = price_walk(usd_foil, &mut rng, days);
        for d in 0..days {
            let as_of = price_history::format_date(today - Duration::days(d as i64));
            models.push(product_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(*product_id),
                as_of_date: Set(as_of),
                price_usd: Set(usd_series[d].clone()),
                price_usd_foil: Set(foil_series[d].clone()),
                created_at: Set(now),
            });
        }
    }

    let total = models.len() as u64;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<product_price_history::ActiveModel> = iter
            .by_ref()
            .take(price_history::PRICE_HISTORY_BATCH)
            .collect();
        if chunk.is_empty() {
            break;
        }
        ProductPriceHistory::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([
                    product_price_history::Column::Game,
                    product_price_history::Column::ProductId,
                    product_price_history::Column::AsOfDate,
                ])
                .update_columns([
                    product_price_history::Column::PriceUsd,
                    product_price_history::Column::PriceUsdFoil,
                ])
                .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

/// Seed a handful of card -> sealed-product memberships across the dummy catalog, so
/// the card-detail "Sealed products" section (found in / can be pulled from / may be
/// in) has data to render fully offline. Resolves the fabricated card/product external
/// ids to the internal `cards.id` / `products.id` the just-seeded rows carry (so it
/// must run after both are seeded), wipes the game's rows, then inserts fresh —
/// mirroring the real ingest's wholesale rebuild, so a reseed is idempotent. Returns
/// the number of rows written.
async fn seed_sealed_contents(db: &DatabaseConnection) -> Result<u64, IngestError> {
    // (card external id, product external id, membership, foil-only). Product ids match
    // `dummy::products` (900001 collector box, 900002 play pack, 900003 bundle, 900004
    // commander deck, 900005 draft box); card ids match `dummy::catalog`.
    let mut seed: Vec<(String, &'static str, Membership, bool)> = Vec::new();
    // Found in: the reprinted relic + the prerelease promo ship in the base-set bundle;
    // a few cards make up the Universe commander deck.
    for card_ext in ["dummy-dmb-0080", "dummy-dmb-0078"] {
        seed.push((card_ext.to_string(), "900003", Membership::Contains, false));
    }
    for card_ext in ["dummy-dmu-0001", "dummy-dmu-0002", "dummy-dmu-0013"] {
        seed.push((card_ext.to_string(), "900004", Membership::Contains, false));
    }
    // Can be pulled from: base-set boosters (collector box + play pack) and the Universe
    // draft box. The foil-only showcase is a foil pull from the collector box.
    for n in 1..=10 {
        seed.push((
            format!("dummy-dmb-{n:04}"),
            "900001",
            Membership::Booster,
            false,
        ));
    }
    for n in 1..=5 {
        seed.push((
            format!("dummy-dmb-{n:04}"),
            "900002",
            Membership::Booster,
            false,
        ));
    }
    for n in 1..=8 {
        seed.push((
            format!("dummy-dmu-{n:04}"),
            "900005",
            Membership::Booster,
            false,
        ));
    }
    seed.push((
        "dummy-dmb-0079".to_string(),
        "900001",
        Membership::Booster,
        true,
    ));
    // May be in: the starlit promo is a randomized foil box insert; the werewolf a
    // randomized bundle insert.
    seed.push((
        "dummy-dmb-0077".to_string(),
        "900001",
        Membership::Variable,
        true,
    ));
    seed.push((
        "dummy-dmb-0076".to_string(),
        "900003",
        Membership::Variable,
        false,
    ));

    // Resolve external ids -> internal ids from the just-seeded rows.
    let card_exts: Vec<String> = seed.iter().map(|(c, ..)| c.clone()).collect();
    let card_ids: std::collections::HashMap<String, i32> = Card::find()
        .select_only()
        .column(card::Column::ExternalId)
        .column(card::Column::Id)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::ExternalId.is_in(card_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    let product_exts: Vec<String> = seed.iter().map(|(_, p, ..)| p.to_string()).collect();
    let product_ids: std::collections::HashMap<String, i32> = Product::find()
        .select_only()
        .column(product::Column::ExternalId)
        .column(product::Column::Id)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::ExternalId.is_in(product_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    let now = Utc::now();
    let models: Vec<sealed_content::ActiveModel> = seed
        .into_iter()
        .filter_map(|(card_ext, product_ext, membership, foil)| {
            let card_id = *card_ids.get(&card_ext)?;
            let product_id = *product_ids.get(product_ext)?;
            Some(sealed_content::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(product_id),
                card_id: Set(card_id),
                membership: Set(membership.as_str().to_string()),
                foil: Set(foil),
                created_at: Set(now),
                updated_at: Set(now),
            })
        })
        .collect();

    // Wholesale rebuild (delete-then-insert) keeps a reseed idempotent, matching the
    // real ingest's semantics.
    SealedContent::delete_many()
        .filter(sealed_content::Column::Game.eq(GAME))
        .exec(db)
        .await?;
    let total = models.len() as u64;
    if !models.is_empty() {
        SealedContent::insert_many(models)
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

/// Seed a couple of sealed-product **compositions** ("what's in the box"), so the
/// sealed-detail "What's in the box" section has data offline: the base-set bundle lists
/// its play boosters (linked to the pack product), a foil promo (linked to the card), and
/// two physical extras; the collector box lists its packs. Resolves the fabricated
/// product/card external ids to internal ids the same way [`seed_sealed_contents`] does
/// (so it runs after both are seeded), wipes the game's rows, then inserts fresh. Returns
/// the number of rows written.
async fn seed_sealed_components(db: &DatabaseConnection) -> Result<u64, IngestError> {
    // (product ext, position, kind, name, quantity, child product ext, child card ext).
    // Product ids match `dummy::products`; card ids match `dummy::catalog`.
    let seed: Vec<(
        &str,
        i32,
        ComponentKind,
        &str,
        i32,
        Option<&str>,
        Option<&str>,
    )> = vec![
        // The base-set bundle (900003): 6 play boosters (linked to the pack product 900002),
        // a foil promo (linked to a card), a spindown, and a storage box.
        (
            "900003",
            0,
            ComponentKind::Sealed,
            "Dummy Base Set Play Booster Pack",
            6,
            Some("900002"),
            None,
        ),
        (
            "900003",
            1,
            ComponentKind::Card,
            "Dummy Prerelease Promo",
            1,
            None,
            Some("dummy-dmb-0078"),
        ),
        (
            "900003",
            2,
            ComponentKind::Other,
            "Spindown life counter",
            1,
            None,
            None,
        ),
        (
            "900003",
            3,
            ComponentKind::Other,
            "Card storage box",
            1,
            None,
            None,
        ),
        // The collector booster box (900001): 12 booster packs (linked to the pack product).
        (
            "900001",
            0,
            ComponentKind::Sealed,
            "Dummy Base Set Play Booster Pack",
            12,
            Some("900002"),
            None,
        ),
    ];

    // Resolve product + card external ids -> internal ids from the just-seeded rows.
    let product_exts: Vec<String> = seed
        .iter()
        .flat_map(|(parent, _, _, _, _, child, _)| {
            std::iter::once(parent.to_string()).chain(child.map(str::to_string))
        })
        .collect();
    let product_ids: std::collections::HashMap<String, i32> = Product::find()
        .select_only()
        .column(product::Column::ExternalId)
        .column(product::Column::Id)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::ExternalId.is_in(product_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    let card_exts: Vec<String> = seed
        .iter()
        .filter_map(|(_, _, _, _, _, _, card)| card.map(str::to_string))
        .collect();
    let card_ids: std::collections::HashMap<String, i32> = Card::find()
        .select_only()
        .column(card::Column::ExternalId)
        .column(card::Column::Id)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::ExternalId.is_in(card_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    let now = Utc::now();
    let models: Vec<sealed_component::ActiveModel> = seed
        .into_iter()
        .filter_map(
            |(parent, position, kind, name, quantity, child_product, child_card)| {
                let product_id = *product_ids.get(parent)?;
                Some(sealed_component::ActiveModel {
                    id: NotSet,
                    game: Set(GAME.to_string()),
                    product_id: Set(product_id),
                    position: Set(position),
                    kind: Set(kind.as_str().to_string()),
                    name: Set(name.to_string()),
                    quantity: Set(quantity),
                    child_product_id: Set(child_product.and_then(|c| product_ids.get(c).copied())),
                    child_card_id: Set(child_card.and_then(|c| card_ids.get(c).copied())),
                    created_at: Set(now),
                    updated_at: Set(now),
                })
            },
        )
        .collect();

    SealedComponent::delete_many()
        .filter(sealed_component::Column::Game.eq(GAME))
        .exec(db)
        .await?;
    let total = models.len() as u64;
    if !models.is_empty() {
        SealedComponent::insert_many(models)
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

/// Seed a few dummy rulings ("Notes and Rules Information", issue #522) for the reprinted
/// card's shared gameplay identity, so the card-detail rulings section renders offline for
/// both of its printings (proving rulings are keyed on `oracle_id`, not the printing).
/// Wipes the game's rows then inserts fresh, mirroring the real ingest's wholesale rebuild,
/// so a reseed is idempotent. Returns the number written.
async fn seed_rulings(db: &DatabaseConnection) -> Result<u64, IngestError> {
    // (source, published_at, comment). Two Wizards rulings on different dates plus a
    // Scryfall note, so the offline section shows the ordered, multi-source shape.
    let rulings: &[(&str, &str, &str)] = &[
        (
            "wotc",
            "2024-01-01",
            "Dummy Reprinted Relic's ability triggers only once each turn, even if several \
             artifacts enter the battlefield at the same time.",
        ),
        (
            "wotc",
            "2024-06-15",
            "If Dummy Reprinted Relic leaves the battlefield before its ability resolves, the \
             ability still resolves.",
        ),
        (
            "scryfall",
            "2024-06-15",
            "This is fabricated offline ruling text — real rulings come from Scryfall's \
             `rulings` bulk data.",
        ),
    ];

    let now = Utc::now();
    let models: Vec<card_ruling::ActiveModel> = rulings
        .iter()
        .map(|(source, published_at, comment)| card_ruling::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            oracle_id: Set(catalog::REPRINT_ORACLE_ID.to_string()),
            source: Set(source.to_string()),
            published_at: Set(published_at.to_string()),
            comment: Set(comment.to_string()),
            created_at: Set(now),
        })
        .collect();

    CardRuling::delete_many()
        .filter(card_ruling::Column::Game.eq(GAME))
        .exec(db)
        .await?;
    let total = models.len() as u64;
    if !models.is_empty() {
        CardRuling::insert_many(models)
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

/// Seed a few dummy Tagger art tags (issue #140) over the seeded artworks, so the
/// `art:` search filter and the art-tag autocomplete/browser work offline. The shape
/// mirrors what the real ingest produces post-expansion: `relic` tags the reprint
/// pair's shared illustration, its ancestor `object` carries the same (expanded) row,
/// and `squirrel` tags an unrelated artwork. **Upsert-only** like the rest of the
/// dummy seed (never deletes — the module contract): keyed on the same unique indexes
/// the real ingest's tables carry (`(game, slug)` / `(game, tag_slug,
/// illustration_id)`), so a reseed overwrites its own three tags and leaves anything
/// else alone. Returns the number of tag + mapping rows written.
async fn seed_art_tags(db: &DatabaseConnection) -> Result<u64, IngestError> {
    // (scryfall_id, slug, label, description, illustration_id)
    let tags: &[(&str, &str, &str, Option<&str>, &str)] = &[
        (
            "dummy-art-tag-0001",
            "relic",
            "Relic",
            Some("Fabricated offline tag — real tags come from Scryfall's `art_tags` bulk data."),
            catalog::REPRINT_ILLUSTRATION_ID,
        ),
        (
            "dummy-art-tag-0002",
            "object",
            "Object",
            None,
            catalog::REPRINT_ILLUSTRATION_ID,
        ),
        (
            "dummy-art-tag-0003",
            "squirrel",
            "Squirrel",
            None,
            catalog::BASE_ONE_ILLUSTRATION_ID,
        ),
    ];

    let now = Utc::now();
    let tag_models: Vec<art_tag::ActiveModel> = tags
        .iter()
        .map(
            |(scryfall_id, slug, label, description, _)| art_tag::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                scryfall_id: Set(scryfall_id.to_string()),
                slug: Set(slug.to_string()),
                label: Set(label.to_string()),
                description: Set(description.map(str::to_string)),
                taggings_count: Set(1),
                created_at: Set(now),
            },
        )
        .collect();
    let mapping_models: Vec<card_art_tag::ActiveModel> = tags
        .iter()
        .map(
            |(_, slug, _, _, illustration_id)| card_art_tag::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                tag_slug: Set(slug.to_string()),
                illustration_id: Set(illustration_id.to_string()),
            },
        )
        .collect();

    let total = (tag_models.len() + mapping_models.len()) as u64;
    ArtTag::insert_many(tag_models)
        .on_conflict(
            OnConflict::columns([art_tag::Column::Game, art_tag::Column::Slug])
                .update_columns([
                    art_tag::Column::ScryfallId,
                    art_tag::Column::Label,
                    art_tag::Column::Description,
                    art_tag::Column::TaggingsCount,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    CardArtTag::insert_many(mapping_models)
        .on_conflict(
            OnConflict::columns([
                card_art_tag::Column::Game,
                card_art_tag::Column::TagSlug,
                card_art_tag::Column::IllustrationId,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(total)
}

/// Seed the dummy MTG catalog, recording status in `ingest_state`. On failure the
/// state row is best-effort marked `"error"` (mirroring `super::ingest::refresh`) so
/// `GET /status` stays honest, and the error is returned for the caller to log.
pub async fn seed(db: &DatabaseConnection) -> Result<(), IngestError> {
    match seed_inner(db).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest::put_state(
                db,
                ingest::IngestStatus::Error,
                ingest::IngestStateUpdate {
                    detail: Some(ingest::truncate(&err.public_detail(), 500)),
                    finished_at: Some(Utc::now()),
                    ..Default::default()
                },
            )
            .await;
            Err(err)
        }
    }
}

async fn seed_inner(db: &DatabaseConnection) -> Result<(), IngestError> {
    let started = Utc::now();
    let cards = dummy_cards();
    let sets = dummy_sets(&cards);
    tracing::info!(
        sets = sets.len(),
        cards = cards.len(),
        "seeding dummy {GAME} catalog"
    );

    // Reuse the real set/card mapping + upsert path so dummy rows are shaped exactly
    // like imported ones (and no on_conflict column list is duplicated here).
    let sets_imported = ingest::import_sets(db, &sets).await?;

    let now = Utc::now();
    let models: Vec<card::ActiveModel> = cards.into_iter().map(|c| map::map_card(c, now)).collect();
    let cards_imported = models.len() as i32;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card::ActiveModel> = iter.by_ref().take(ingest::CARD_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        ingest::flush_cards(db, chunk).await?;
    }

    // Seed a year of price history so the chart shows a real trend offline.
    let history_rows = seed_price_history(db).await?;
    tracing::info!(rows = history_rows, "seeded dummy price history");

    // Sealed products + their year of price history, so the product routes have data.
    let products_seeded = seed_products(db).await?;
    let product_history_rows = seed_product_price_history(db).await?;
    tracing::info!(
        products = products_seeded,
        rows = product_history_rows,
        "seeded dummy sealed products"
    );

    // Card -> sealed-product memberships, so the card-detail "Sealed products" section
    // has data offline. Runs after both cards and products are seeded (it joins them).
    let membership_rows = seed_sealed_contents(db).await?;
    tracing::info!(
        rows = membership_rows,
        "seeded dummy sealed-product memberships"
    );

    // Sealed-product compositions ("what's in the box"), so the sealed-detail contents
    // section renders offline. Also joins cards + products, so it runs after both.
    let component_rows = seed_sealed_components(db).await?;
    tracing::info!(
        rows = component_rows,
        "seeded dummy sealed-product components"
    );

    // Card rulings ("Notes and Rules Information"), so the card-detail rulings section
    // renders offline. Keyed on the reprinted card's oracle id (seeded above).
    let ruling_rows = seed_rulings(db).await?;
    tracing::info!(rows = ruling_rows, "seeded dummy card rulings");

    // Tagger art tags over the seeded artworks, so `art:` searches and the tag
    // browser work offline. Runs after the cards (it tags their illustrations).
    let art_tag_rows = seed_art_tags(db).await?;
    tracing::info!(rows = art_tag_rows, "seeded dummy art tags");

    // Record ONE ingest_state row under the same dataset key the real importer uses,
    // because `ingest_status` loads it with `.one()` filtered only by game (not
    // dataset) and so relies on there being exactly one row per game. A synthetic
    // source version lets a later real sync detect the change and re-import. Do not
    // give dummy its own dataset key — that would create a second row and make the
    // status route ambiguous.
    ingest::put_state(
        db,
        ingest::IngestStatus::Complete,
        ingest::IngestStateUpdate {
            source_updated_at: Some(DUMMY_SOURCE_VERSION.to_string()),
            detail: Some("seeded dummy offline catalog".to_string()),
            sets_imported,
            cards_imported,
            started_at: Some(started),
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(
        sets = sets_imported,
        cards = cards_imported,
        "dummy catalog seed complete"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seeds_multi_day_price_history_and_reseed_is_idempotent() {
        use crate::entities::card_price_history;
        use crate::entities::prelude::{Card, CardPriceHistory};
        use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect};

        let db = crate::test_support::migrated_memory_db().await;

        seed(&db).await.expect("seed succeeds");

        let card_count = dummy_cards().len() as u64;
        let expected_rows = card_count * PRICE_HISTORY_DAYS as u64;

        // One history row per (card, day).
        let rows = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(
            rows, expected_rows,
            "expected {PRICE_HISTORY_DAYS} days per card"
        );

        // The series spans exactly `PRICE_HISTORY_DAYS` distinct dates.
        let dates: Vec<String> = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::AsOfDate)
            .filter(card_price_history::Column::Game.eq(GAME))
            .distinct()
            .into_tuple()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(dates.len(), PRICE_HISTORY_DAYS as usize);

        // End-to-end anchoring: the newest (today) history point equals the card's
        // current price, confirming day offset 0 maps to the series' first value.
        let today = price_history::format_date(Utc::now().date_naive());
        let sample = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::PriceUsd.is_not_null())
            .one(&db)
            .await
            .unwrap()
            .expect("a card with a usd price");
        let today_row = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .filter(card_price_history::Column::CardId.eq(sample.id))
            .filter(card_price_history::Column::AsOfDate.eq(today))
            .one(&db)
            .await
            .unwrap()
            .expect("a price row dated today");
        assert_eq!(
            today_row.price_usd, sample.price_usd,
            "today's history must equal the card's current price"
        );

        // Reseeding upserts on the unique key rather than duplicating rows.
        seed(&db).await.expect("reseed succeeds");
        let rows_again = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(
            rows_again, expected_rows,
            "reseed must not duplicate price history"
        );
    }

    #[tokio::test]
    async fn seed_populates_catalog_and_reseed_is_idempotent() {
        use crate::entities::prelude::{Card, CardSet, IngestState};
        use crate::entities::{card_set, ingest_state};
        use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

        let db = crate::test_support::migrated_memory_db().await;

        seed(&db).await.expect("seed succeeds");

        let expected_cards = dummy_cards().len() as u64;
        let expected_sets = dummy_sets(&dummy_cards()).len() as u64;
        let cards = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards, expected_cards);
        assert_eq!(sets, expected_sets);

        // The status row is one-per-game, marked complete, with honest counts.
        let state = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .one(&db)
            .await
            .unwrap()
            .expect("an ingest_state row");
        assert_eq!(state.status, "complete");
        assert_eq!(state.cards_imported as u64, expected_cards);
        assert_eq!(state.sets_imported as u64, expected_sets);

        // The multi-faced card round-trips with faces JSON and no stored image.
        let dfc = Card::find()
            .filter(card::Column::Layout.eq("transform"))
            .one(&db)
            .await
            .unwrap()
            .expect("a transform card");
        assert!(dfc.card_faces.is_some());
        assert!(dfc.image_normal.is_none() && dfc.image_small.is_none());

        // A non-numeric collector number maps to a NULL collector_number_int — the
        // NULLS-LAST sort the set-cards read path relies on.
        let promo = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::CollectorNumber.eq("★"))
            .one(&db)
            .await
            .unwrap()
            .expect("the ★ promo card");
        assert!(promo.collector_number_int.is_none());

        // Re-seeding upserts the same rows rather than inserting duplicates, and keeps
        // exactly one ingest_state row per game (the status route's `.one()` needs it).
        seed(&db).await.expect("reseed succeeds");
        let cards_again = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets_again = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards_again, expected_cards, "reseed must be idempotent");
        assert_eq!(sets_again, expected_sets, "reseed must not add sets");
        let state_rows = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            state_rows.len(),
            1,
            "reseed must upsert the single status row"
        );
    }
}
