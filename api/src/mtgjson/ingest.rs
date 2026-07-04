//! Rebuild of the `sealed_contents` table from MTGJSON's `AllPrintings.json`.
//!
//! The flow mirrors [`crate::tcgcsv::ingest`]'s shape (version gate, `ingest_state`
//! bookkeeping, batched writes): fetch the file conditionally (skip on a `304`), resolve
//! every sealed product's contents into per-card membership rows ([`model::build_memberships`]),
//! map the external ids (TCGplayer product id -> `products.id`, Scryfall id ->
//! `cards.id`) onto our catalog, and **replace** the game's rows in one transaction so
//! stale membership never lingers. Version-gated on the file's HTTP `ETag` (stored in the
//! `(mtg, mtgjson_sealed_contents)` `ingest_state` row), so an unchanged file costs one
//! conditional request.
//!
//! Only products that resolve to our `products` table (by `tcgplayerProductId`) and cards
//! that resolve to our `cards` table (by Scryfall id) get rows; the rest are skipped and
//! tallied. Cross-set references whose card isn't in our catalog, and any product not on
//! TCGplayer, simply don't appear.

use std::collections::HashMap;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, Iterable, QueryFilter, QuerySelect,
    TransactionTrait,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use super::client::{FetchOutcome, fetch_all_printings};
use super::model::{self, RawMembership};
use super::progress::SyncProgress;
use super::{DATASET, GAME, MtgjsonError};
use crate::entities::prelude::{Card, IngestState, Product, SealedContent};
use crate::entities::{card, ingest_state, product, sealed_content};

/// Rows per external-id `IN` lookup — under SQLite's 32 766 bound-parameter limit.
const IN_CHUNK: usize = 900;

/// Rows per membership insert. Eight columns, so ~2000 rows ≈ 16k binds — under the limit.
const INSERT_BATCH: usize = 2000;

/// Sync MTG sealed-product memberships from MTGJSON, recording status in `ingest_state`.
/// On error the state row is best-effort marked `"error"` (so the next tick retries) and
/// the error is returned for the caller to log.
pub async fn refresh(db: &DatabaseConnection, http: &Client) -> Result<(), MtgjsonError> {
    match refresh_inner(db, http).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = mark_error(db, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(db: &DatabaseConnection, http: &Client) -> Result<(), MtgjsonError> {
    let existing = load_state(db).await?;
    let prior_etag = existing
        .as_ref()
        .filter(|s| s.status == "complete")
        .and_then(|s| s.source_updated_at.clone());

    let progress = SyncProgress::start("checking for updates");

    // Conditional fetch: a 304 (unchanged file) short-circuits the whole rebuild.
    let (etag, all) = match fetch_all_printings(http, prior_etag.as_deref()).await? {
        FetchOutcome::Unchanged => {
            drop(progress);
            tracing::info!("mtgjson sealed contents already up to date");
            return Ok(());
        }
        FetchOutcome::Fetched { etag, all } => (etag, all),
    };

    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    put_state(db, "running", None, "resolving contents", started, None, 0, 0).await?;

    // Resolve contents -> per-card membership rows off the async runtime (CPU-bound over
    // a big document). `all` is dropped when the closure returns, freeing the parse tree.
    progress.set_stage("resolving contents");
    let all = *all;
    let memberships: Vec<RawMembership> = tokio::task::spawn_blocking(move || {
        model::build_memberships(&all)
    })
    .await
    .map_err(|err| MtgjsonError::Join(err.to_string()))?;
    tracing::info!(rows = memberships.len(), "mtgjson: resolved membership rows");

    // Map external ids onto our catalog.
    let product_ext: Vec<String> = distinct(memberships.iter().map(|m| &m.tcgplayer_product_id));
    let card_ext: Vec<String> = distinct(memberships.iter().map(|m| &m.scryfall_id));
    progress.set_stage("matching to catalog");
    let products = resolve_products(db, &product_ext).await?;
    let cards = resolve_cards(db, &card_ext).await?;

    // Build rows for memberships whose product AND card are both in our catalog.
    let now = Utc::now();
    let mut models: Vec<sealed_content::ActiveModel> = Vec::new();
    for m in &memberships {
        let (Some(&product_id), Some(&card_id)) = (
            products.get(&m.tcgplayer_product_id),
            cards.get(&m.scryfall_id),
        ) else {
            continue;
        };
        models.push(sealed_content::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            product_id: Set(product_id),
            card_id: Set(card_id),
            membership: Set(m.membership.to_string()),
            foil: Set(m.foil),
            created_at: Set(now),
            updated_at: Set(now),
        });
    }
    let matched = models.len();
    progress.set_rows("writing", matched as u64);

    // Replace the game's rows in one transaction so a reader never sees a half-rebuilt
    // table and stale membership can't survive a product's contents changing.
    let txn = db.begin().await?;
    SealedContent::delete_many()
        .filter(sealed_content::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<sealed_content::ActiveModel> = iter.by_ref().take(INSERT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        // do_nothing on conflict is belt-and-braces: build_memberships already dedupes,
        // and the table was just cleared, so a conflict shouldn't occur.
        SealedContent::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([
                    sealed_content::Column::Game,
                    sealed_content::Column::ProductId,
                    sealed_content::Column::CardId,
                    sealed_content::Column::Membership,
                    sealed_content::Column::Foil,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&txn)
            .await?;
    }
    txn.commit().await?;

    drop(progress);
    let detail = format!(
        "{matched} memberships across {} products (from {} resolved rows)",
        products.len(),
        memberships.len()
    );
    put_state(
        db,
        "complete",
        etag.as_deref(),
        &detail,
        started,
        Some(Utc::now()),
        products.len() as i32,
        matched as i32,
    )
    .await?;
    tracing::info!(
        memberships = matched,
        products = products.len(),
        "mtgjson sealed contents sync complete"
    );
    Ok(())
}

/// Collect the distinct owned strings from an iterator of `&String`.
fn distinct<'a, I: Iterator<Item = &'a String>>(iter: I) -> Vec<String> {
    let set: std::collections::HashSet<&String> = iter.collect();
    set.into_iter().cloned().collect()
}

/// Resolve TCGplayer product ids -> internal `products.id` for the game, chunked under
/// SQLite's bind limit.
async fn resolve_products(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Product::find()
            .select_only()
            .column(product::Column::ExternalId)
            .column(product::Column::Id)
            .filter(product::Column::Game.eq(GAME))
            .filter(product::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Resolve Scryfall ids -> internal `cards.id` for the game, chunked under SQLite's bind
/// limit.
async fn resolve_cards(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Card::find()
            .select_only()
            .column(card::Column::ExternalId)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

// ----- ingest_state bookkeeping (dataset = mtgjson_sealed_contents) -----

async fn load_state(db: &DatabaseConnection) -> Result<Option<ingest_state::Model>, MtgjsonError> {
    Ok(IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(DATASET))
        .one(db)
        .await?)
}

#[allow(clippy::too_many_arguments)]
async fn put_state(
    db: &DatabaseConnection,
    status: &str,
    source_updated_at: Option<&str>,
    detail: &str,
    started_at: DateTimeUtc,
    finished_at: Option<DateTimeUtc>,
    products: i32,
    memberships: i32,
) -> Result<(), MtgjsonError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(source_updated_at.map(str::to_string)),
        status: Set(status.to_string()),
        detail: Set(Some(detail.to_string())),
        sets_imported: Set(products),
        cards_imported: Set(memberships),
        started_at: Set(Some(started_at)),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns(ingest_state::Column::iter().filter(|c| {
                    !matches!(
                        c,
                        ingest_state::Column::Id
                            | ingest_state::Column::Game
                            | ingest_state::Column::Dataset
                    )
                }))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

async fn mark_error(db: &DatabaseConnection, message: &str) -> Result<(), MtgjsonError> {
    let existing = load_state(db).await?;
    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    // Keep the last known-good ETag so a transient failure doesn't force a full re-fetch
    // *unless* the file also changed.
    let last = existing.and_then(|s| s.source_updated_at);
    let detail: String = message.chars().take(500).collect();
    put_state(db, "error", last.as_deref(), &detail, started, Some(Utc::now()), 0, 0).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::SealedContent;
    use crate::test_support::{insert_card, migrated_memory_db};
    use sea_orm::PaginatorTrait;

    /// Insert a product row and return its id (products carry only an external id + name).
    async fn insert_product(db: &DatabaseConnection, external_id: &str) -> i32 {
        let now = Utc::now();
        let model = product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Product {external_id}")),
            clean_name: Set(None),
            set_code: Set("set".to_string()),
            product_type: Set("bundle".to_string()),
            url: Set(None),
            image_url: Set(None),
            price_usd: Set(None),
            price_usd_foil: Set(None),
            released_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        product::Entity::insert(model)
            .exec(db)
            .await
            .unwrap()
            .last_insert_id
    }

    /// The resolve + write path: memberships whose product AND card resolve are written;
    /// the rest are skipped, and a re-run replaces (not duplicates) the rows.
    #[tokio::test]
    async fn resolve_and_write_replaces_rows() {
        let db = migrated_memory_db().await;
        let card_a = insert_card(&db, "sf-a").await; // scryfall id "sf-a"
        let _card_b = insert_card(&db, "sf-b").await;
        let product_id = insert_product(&db, "1001").await;

        // Two resolvable rows + one whose product isn't in our catalog (skipped).
        let memberships = vec![
            RawMembership {
                tcgplayer_product_id: "1001".to_string(),
                scryfall_id: "sf-a".to_string(),
                membership: "contains",
                foil: false,
            },
            RawMembership {
                tcgplayer_product_id: "1001".to_string(),
                scryfall_id: "sf-a".to_string(),
                membership: "booster",
                foil: true,
            },
            RawMembership {
                tcgplayer_product_id: "9999".to_string(), // no such product
                scryfall_id: "sf-b".to_string(),
                membership: "contains",
                foil: false,
            },
        ];

        let written = write_for_test(&db, &memberships).await;
        assert_eq!(written, 2, "only the two rows with a matched product are written");

        let rows = SealedContent::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.product_id == product_id && r.card_id == card_a));
        assert!(rows.iter().any(|r| r.membership == "contains" && !r.foil));
        assert!(rows.iter().any(|r| r.membership == "booster" && r.foil));

        // Re-run replaces rather than duplicating (the transaction wipes first).
        let written_again = write_for_test(&db, &memberships).await;
        assert_eq!(written_again, 2);
        let count = SealedContent::find().count(&db).await.unwrap();
        assert_eq!(count, 2, "re-run replaces, not duplicates");
    }

    /// Drives the resolve + transactional replace without the network fetch, so the DB
    /// path is testable offline. Returns the number of rows written.
    async fn write_for_test(db: &DatabaseConnection, memberships: &[RawMembership]) -> usize {
        let product_ext: Vec<String> =
            distinct(memberships.iter().map(|m| &m.tcgplayer_product_id));
        let card_ext: Vec<String> = distinct(memberships.iter().map(|m| &m.scryfall_id));
        let products = resolve_products(db, &product_ext).await.unwrap();
        let cards = resolve_cards(db, &card_ext).await.unwrap();

        let now = Utc::now();
        let mut models: Vec<sealed_content::ActiveModel> = Vec::new();
        for m in memberships {
            let (Some(&product_id), Some(&card_id)) =
                (products.get(&m.tcgplayer_product_id), cards.get(&m.scryfall_id))
            else {
                continue;
            };
            models.push(sealed_content::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(product_id),
                card_id: Set(card_id),
                membership: Set(m.membership.to_string()),
                foil: Set(m.foil),
                created_at: Set(now),
                updated_at: Set(now),
            });
        }
        let written = models.len();
        let txn = db.begin().await.unwrap();
        SealedContent::delete_many()
            .filter(sealed_content::Column::Game.eq(GAME))
            .exec(&txn)
            .await
            .unwrap();
        SealedContent::insert_many(models)
            .on_conflict(
                OnConflict::columns([
                    sealed_content::Column::Game,
                    sealed_content::Column::ProductId,
                    sealed_content::Column::CardId,
                    sealed_content::Column::Membership,
                    sealed_content::Column::Foil,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&txn)
            .await
            .unwrap();
        txn.commit().await.unwrap();
        written
    }
}
