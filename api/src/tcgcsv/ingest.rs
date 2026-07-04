//! Daily sync of MTG sealed products from TCGCSV into the `products` table.
//!
//! The sweep is: fetch every group (`/tcgplayer/1/groups`), then per group fetch its
//! products and prices, keep only **sealed** products (see [`super::classify`]),
//! classify each into a coarse `product_type`, and upsert them with their current
//! market prices. The whole sweep is **version-gated** on TCGCSV's `last-updated.txt`
//! (recorded in an `ingest_state` row keyed `(mtg, tcgcsv_products)`), so an unchanged
//! day costs one request. Requests are paced ~100 ms apart — a full sweep is ~900
//! requests, well under TCGCSV's ~10k/day budget.
//!
//! Mirrors `scryfall::ingest`'s shape (version gate, batched upserts with the
//! update-column list derived from the entity columns, `ingest_state` bookkeeping) but
//! reuses this module's [`BackfillError`](super::BackfillError) since it shares the
//! client and models.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, Iterable, QueryFilter,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use super::model::{Group, aggregate_prices, published_on_to_date};
use super::progress::SyncProgress;
use super::{BackfillError, GAME, MTG_CATEGORY_ID, PRODUCTS_DATASET};
use crate::entities::prelude::{IngestState, Product};
use crate::entities::{ingest_state, product};

/// Courtesy pacing between provider requests (TCGCSV asks for < 10k req/day; a full
/// sweep is ~900 requests, so this keeps us well-behaved).
const REQUEST_SPACING: Duration = Duration::from_millis(100);

/// Rows per product upsert. A product row has ~15 columns, so ~1000 rows ≈ 15k bound
/// parameters — under SQLite's 32 766 limit.
const PRODUCT_BATCH: usize = 1000;

/// Sync MTG sealed products from TCGCSV, recording status in `ingest_state`. On error
/// the state row is best-effort marked `"error"` (so the next tick retries) and the
/// error is returned for the caller to log.
pub async fn refresh(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    match refresh_inner(db, http, user_agent, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = mark_error(db, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    let base_url = source.tcgcsv_base_url();
    // Version gate: one cheap request. Skip the whole sweep if TCGCSV hasn't refreshed
    // since our last complete sync.
    let remote_version = super::client::last_updated(http, &base_url, user_agent).await?;
    let existing = load_state(db).await?;
    if let Some(state) = &existing
        && state.status == "complete"
        && state.source_updated_at.as_deref() == Some(remote_version.as_str())
    {
        tracing::info!(version = %remote_version, "tcgcsv products already up to date");
        return Ok(());
    }

    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    put_state(db, "running", None, "fetching groups", started, None, 0, 0).await?;

    let groups = super::client::fetch_groups(http, &base_url, user_agent, MTG_CATEGORY_ID)
        .await?
        .results;
    tracing::info!(groups = groups.len(), "tcgcsv products: sweeping groups");

    let now = Utc::now();
    let mut total_products: i32 = 0;
    let groups_total = groups.len() as i32;
    // Live terminal progress: a determinate bar over the groups being swept, with a
    // running sealed-product tally (see `super::progress`). Dropping it (incl. on any
    // `?` below) closes the span and clears the bar.
    let progress = SyncProgress::start_products(groups_total as u64);
    for (i, group) in groups.iter().enumerate() {
        tokio::time::sleep(REQUEST_SPACING).await;
        let products =
            super::client::fetch_products(http, &base_url, user_agent, MTG_CATEGORY_ID, group.group_id)
                .await?
                .results;

        tokio::time::sleep(REQUEST_SPACING).await;
        let prices = aggregate_prices(
            super::client::fetch_prices(http, &base_url, user_agent, MTG_CATEGORY_ID, group.group_id)
                .await?
                .results,
        );

        let models = build_group_products(group, products, &prices, now);
        let sealed = models.len();
        total_products += upsert_products(db, models).await? as i32;
        progress.inc();
        progress.set_count(total_products as u64);
        tracing::debug!(
            group = group.group_id,
            name = group.name.as_deref().unwrap_or(""),
            sealed,
            "tcgcsv products: swept group"
        );

        // Periodic progress so the sweep is observable and a crash resumes cleanly
        // (the version gate re-runs the whole sweep — idempotent upserts make that safe).
        if i % 25 == 0 {
            put_state(
                db,
                "running",
                None,
                &format!("swept {} of {groups_total} groups", i + 1),
                started,
                None,
                groups_total,
                total_products,
            )
            .await?;
        }
    }

    // Clear the progress bar before the completion line so it prints cleanly.
    drop(progress);
    put_state(
        db,
        "complete",
        Some(&remote_version),
        &format!("imported {total_products} sealed products from {groups_total} groups"),
        started,
        Some(Utc::now()),
        groups_total,
        total_products,
    )
    .await?;
    tracing::info!(
        products = total_products,
        groups = groups_total,
        "tcgcsv products sync complete"
    );
    Ok(())
}

/// Build product `ActiveModel`s for a group's **sealed** products, attaching each
/// product's current market prices from `prices`. Cards (products with a `Rarity`/
/// `Number` attribute) are filtered out. Pure so it's unit-testable without a DB.
fn build_group_products(
    group: &Group,
    products: Vec<super::model::Product>,
    prices: &HashMap<i64, super::model::DayPrice>,
    now: DateTimeUtc,
) -> Vec<product::ActiveModel> {
    let set_code = group
        .abbreviation
        .as_deref()
        .map(|a| a.trim().to_lowercase())
        .unwrap_or_default();
    let released_at = published_on_to_date(group.published_on.as_deref());

    products
        .into_iter()
        .filter(|p| super::classify::is_sealed(&p.extended_data))
        .map(|p| {
            let product_type = super::classify::classify_product_type(&p.name);
            let day = prices.get(&p.product_id);
            product::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                external_id: Set(p.product_id.to_string()),
                name: Set(p.name),
                clean_name: Set(p.clean_name),
                set_code: Set(set_code.clone()),
                product_type: Set(product_type.to_string()),
                url: Set(p.url),
                image_url: Set(p.image_url),
                price_usd: Set(day.and_then(|d| d.usd.clone())),
                price_usd_foil: Set(day.and_then(|d| d.usd_foil.clone())),
                released_at: Set(released_at.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            }
        })
        .collect()
}

/// Batched upsert on `(game, external_id)`, updating every provider-owned column (all
/// but the identity/conflict keys and `created_at`). Returns the number of rows sent.
async fn upsert_products(
    db: &DatabaseConnection,
    models: Vec<product::ActiveModel>,
) -> Result<u64, BackfillError> {
    let mut total: u64 = 0;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<product::ActiveModel> = iter.by_ref().take(PRODUCT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        total += chunk.len() as u64;
        Product::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([product::Column::Game, product::Column::ExternalId])
                    .update_columns(product::Column::iter().filter(|c| {
                        !matches!(
                            c,
                            product::Column::Id
                                | product::Column::Game
                                | product::Column::ExternalId
                                | product::Column::CreatedAt
                        )
                    }))
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

// ----- ingest_state bookkeeping (dataset = tcgcsv_products) -----

async fn load_state(
    db: &DatabaseConnection,
) -> Result<Option<ingest_state::Model>, BackfillError> {
    Ok(IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(PRODUCTS_DATASET))
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
    groups: i32,
    products: i32,
) -> Result<(), BackfillError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(PRODUCTS_DATASET.to_string()),
        source_updated_at: Set(source_updated_at.map(str::to_string)),
        status: Set(status.to_string()),
        detail: Set(Some(detail.to_string())),
        sets_imported: Set(groups),
        cards_imported: Set(products),
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

async fn mark_error(db: &DatabaseConnection, message: &str) -> Result<(), BackfillError> {
    let existing = load_state(db).await?;
    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    let last = existing.and_then(|s| s.source_updated_at);
    let detail: String = message.chars().take(500).collect();
    put_state(db, "error", last.as_deref(), &detail, started, Some(Utc::now()), 0, 0).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::Product;
    use crate::tcgcsv::model::{DayPrice, ExtendedData, Product as SrcProduct};
    use sea_orm::EntityTrait;

    fn src_product(id: i64, name: &str, extended: &[&str]) -> SrcProduct {
        SrcProduct {
            product_id: id,
            name: name.to_string(),
            clean_name: Some(name.to_string()),
            image_url: Some(format!("https://img/{id}.jpg")),
            url: Some(format!("https://www.tcgplayer.com/product/{id}")),
            extended_data: extended
                .iter()
                .map(|n| ExtendedData {
                    name: n.to_string(),
                })
                .collect(),
        }
    }

    fn group() -> Group {
        Group {
            group_id: 2377,
            name: Some("Murders at Karlov Manor".to_string()),
            abbreviation: Some("MKM".to_string()),
            published_on: Some("2024-02-09T00:00:00".to_string()),
        }
    }

    #[test]
    fn builds_only_sealed_products_with_prices_and_metadata() {
        let products = vec![
            // Sealed (no Rarity/Number): kept, with prices attached.
            src_product(100, "Collector Booster Box", &["UPC"]),
            // A single card (has Rarity + Number): filtered out.
            src_product(200, "Some Rare Card", &["Rarity", "Number"]),
            // Sealed with no price row: kept, prices NULL.
            src_product(300, "Bundle", &[]),
        ];
        let mut prices: HashMap<i64, DayPrice> = HashMap::new();
        prices.insert(
            100,
            DayPrice {
                usd: Some("199.99".into()),
                usd_foil: None,
            },
        );
        let now = Utc::now();
        let models = build_group_products(&group(), products, &prices, now);

        assert_eq!(models.len(), 2, "the single card is filtered out");
        let box_model = models
            .iter()
            .find(|m| m.external_id.as_ref() == "100")
            .expect("sealed box present");
        assert_eq!(box_model.set_code.as_ref(), "mkm");
        assert_eq!(box_model.product_type.as_ref(), "collector_display");
        assert_eq!(box_model.released_at.as_ref().as_deref(), Some("2024-02-09"));
        assert_eq!(box_model.price_usd.as_ref().as_deref(), Some("199.99"));
        assert!(box_model.price_usd_foil.as_ref().is_none());

        let bundle = models
            .iter()
            .find(|m| m.external_id.as_ref() == "300")
            .expect("sealed bundle present");
        assert_eq!(bundle.product_type.as_ref(), "bundle");
        assert!(bundle.price_usd.as_ref().is_none());
    }

    #[test]
    fn missing_abbreviation_yields_blank_set_code() {
        let group = Group {
            abbreviation: None,
            published_on: None,
            ..group()
        };
        let models = build_group_products(
            &group,
            vec![src_product(1, "Booster Box", &[])],
            &HashMap::new(),
            Utc::now(),
        );
        assert_eq!(models[0].set_code.as_ref(), "");
        assert!(models[0].released_at.as_ref().is_none());
    }

    #[tokio::test]
    async fn upsert_products_is_idempotent_on_game_external_id() {
        let db = crate::test_support::migrated_memory_db().await;
        let now = Utc::now();

        let first = build_group_products(
            &group(),
            vec![src_product(100, "Collector Booster Box", &["UPC"])],
            &HashMap::new(),
            now,
        );
        upsert_products(&db, first).await.expect("first upsert");

        // Re-sweep with a changed price: upserts the same row rather than duplicating.
        let mut prices: HashMap<i64, DayPrice> = HashMap::new();
        prices.insert(
            100,
            DayPrice {
                usd: Some("149.99".into()),
                usd_foil: None,
            },
        );
        let second = build_group_products(
            &group(),
            vec![src_product(100, "Collector Booster Box", &["UPC"])],
            &prices,
            now,
        );
        upsert_products(&db, second).await.expect("second upsert");

        let all = Product::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1, "same (game, external_id) upserts, not duplicates");
        assert_eq!(all[0].price_usd.as_deref(), Some("149.99"));
    }
}
