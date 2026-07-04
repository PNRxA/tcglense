//! The one-time historic price backfill: walk TCGCSV's daily price archives from
//! the first available day forward and insert `card_price_history` rows for MTG
//! cards we already imported from Scryfall, joined on `tcgplayer_id`.
//!
//! It runs once (gated on an `ingest_state` row) and is **resumable per date**: the
//! last completed archive date is recorded after every day, so a crash resumes at
//! the next day rather than restarting. Existing `(game, card, date)` rows are
//! never overwritten (`ON CONFLICT DO NOTHING`), so a real Scryfall daily snapshot
//! always wins over a historic TCGCSV fill.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};
use sevenz_rust2::{ArchiveReader, Password};

use super::BackfillError;
use super::model::{DayPrice, PriceFile, aggregate_prices};
use super::progress::SyncProgress;
use super::{DATASET, GAME, MTG_CATEGORY_ID};
use crate::entities::prelude::{
    Card, CardPriceHistory, IngestState, Product, ProductPriceHistory,
};
use crate::entities::{card, card_price_history, ingest_state, product, product_price_history};

/// First day TCGCSV published a price archive.
fn first_archive_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 2, 8).expect("valid constant date")
}

/// Minimum spacing between archive downloads (TCGCSV asks for < 10k req/day; we do
/// ~1 request per archived day, so this is courtesy pacing).
const REQUEST_SPACING: Duration = Duration::from_millis(100);

/// Rows per history insert. A history row has 8 columns, so ~2000 rows ≈ 16k bound
/// parameters — comfortably under SQLite's 32 766 limit (matches the Scryfall
/// snapshot's batch size).
const INSERT_CHUNK: usize = 2000;

/// Run the historic price backfill for MTG. Idempotent and gated: once the
/// `ingest_state` row for `(mtg, tcgcsv_price_backfill)` is `complete` it returns
/// immediately. `days_cap` (`0` = all) limits the walk to the most recent N archive
/// days. Errors are returned for the caller to log; the state row is best-effort
/// marked `error` so a later boot retries from where it left off.
pub async fn run(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    days_cap: u32,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    match run_inner(db, http, user_agent, days_cap, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = mark_error(db, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn run_inner(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    days_cap: u32,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    let base_url = source.tcgcsv_base_url();
    let existing = load_state(db).await?;
    if existing.as_ref().map(|s| s.status.as_str()) == Some("complete") {
        tracing::info!("tcgcsv price backfill already complete; skipping");
        return Ok(());
    }

    // Join key: every MTG card that carries a TCGplayer product id. Built once and
    // reused across every archived day.
    let map = load_tcgplayer_map(db).await?;
    if map.is_empty() {
        // Cards haven't been imported (or none carry a tcgplayer_id) yet. Leave the
        // state row untouched so the next boot — after a card sync — retries.
        tracing::warn!("tcgcsv backfill: no cards with a tcgplayer_id yet; deferring");
        return Ok(());
    }
    // The parallel join key for sealed products: `productId -> products.id`. Empty when
    // products haven't been synced yet (the backfill runs once, and tasks.rs orders the
    // first product sync before it, so normally it's populated) — an empty map just
    // means no product rows are backfilled, which is fine.
    let product_map = load_product_map(db).await?;
    tracing::info!(
        cards = map.len(),
        products = product_map.len(),
        "tcgcsv backfill: built tcgplayer_id maps"
    );

    let today = Utc::now().date_naive();
    // Candidate window: [start, today]. `days_cap` bounds it to the most recent N
    // days; resumption skips everything up to and including the last completed date.
    let mut start = first_archive_date();
    if days_cap > 0 {
        let cap_start = today - chrono::Duration::days(i64::from(days_cap) - 1);
        start = start.max(cap_start);
    }
    if let Some(last) = existing
        .as_ref()
        .and_then(|s| s.source_updated_at.as_deref())
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        start = start.max(last + chrono::Duration::days(1));
    }

    let started = existing.and_then(|s| s.started_at).unwrap_or_else(Utc::now);
    if start > today {
        // Nothing left to do (an already-finished window resumed, or cap already met).
        finish(db, started, "no archive days in range", 0).await?;
        return Ok(());
    }

    tracing::info!(from = %start, to = %today, "tcgcsv backfill: walking price archives");
    put_running(db, started, None, &format!("starting at {start}"), 0).await?;

    // Live terminal progress: a determinate bar over every candidate archive day in
    // `[start, today]` (incl. the days with no archive, which still count as one step),
    // with a running backfilled-row tally (see `super::progress`). `start <= today` is
    // guaranteed by the guard above, so the length is at least 1.
    let total_days = (today - start).num_days().max(0) as u64 + 1;
    let progress = SyncProgress::start_backfill(total_days);

    let mut total_rows: i64 = 0;
    let mut date = start;
    while date <= today {
        tokio::time::sleep(REQUEST_SPACING).await;

        match super::client::fetch_archive(http, &base_url, user_agent, date).await {
            Ok(Some(bytes)) => {
                let rows_written = process_day(db, bytes.to_vec(), date, &map, &product_map).await?;
                total_rows = total_rows.saturating_add(rows_written as i64);
                tracing::debug!(date = %date, rows = rows_written, "tcgcsv day backfilled");
            }
            Ok(None) => {
                tracing::debug!(date = %date, "tcgcsv: no archive for day (404), skipping");
            }
            Err(err) => return Err(err),
        }

        // Record progress after every day (incl. a 404 day) so a crash resumes at
        // the next date rather than re-fetching the whole range.
        put_running(
            db,
            started,
            Some(date),
            &format!("backfilled through {date}"),
            total_rows,
        )
        .await?;
        progress.inc();
        progress.set_count(total_rows.max(0) as u64);
        date += chrono::Duration::days(1);
    }

    // Clear the progress bar before the completion line so it prints cleanly.
    drop(progress);
    finish(
        db,
        started,
        &format!("backfilled {total_rows} price rows"),
        total_rows,
    )
    .await?;
    tracing::info!(rows = total_rows, "tcgcsv price backfill complete");
    Ok(())
}

/// Decompress one day's archive (blocking CPU work → `spawn_blocking`), join its
/// prices onto our cards **and** sealed products, and insert the new history rows into
/// both `card_price_history` and `product_price_history`. Returns total rows inserted.
async fn process_day(
    db: &DatabaseConnection,
    bytes: Vec<u8>,
    date: NaiveDate,
    map: &HashMap<i64, i32>,
    product_map: &HashMap<i64, i32>,
) -> Result<u64, BackfillError> {
    // PPMd decompression + JSON parsing is synchronous and CPU-bound; keep it off
    // the async runtime. Returns the per-product aggregate for the day. The same
    // `productId`-keyed aggregate feeds both the card and product joins.
    let aggregate =
        tokio::task::spawn_blocking(move || extract_and_aggregate(&bytes, MTG_CATEGORY_ID))
            .await??;

    let now = Utc::now();
    let date_str = date.format("%Y-%m-%d").to_string();

    // Cards.
    let card_rows = build_day_rows(map, &aggregate, &date_str, now);
    let mut written: u64 = 0;
    let mut iter = card_rows.into_iter();
    loop {
        let chunk: Vec<card_price_history::ActiveModel> =
            iter.by_ref().take(INSERT_CHUNK).collect();
        if chunk.is_empty() {
            break;
        }
        written += insert_history(db, chunk).await?;
    }

    // Sealed products (empty when no products are synced yet).
    let product_rows = build_product_day_rows(product_map, &aggregate, &date_str, now);
    let mut iter = product_rows.into_iter();
    loop {
        let chunk: Vec<product_price_history::ActiveModel> =
            iter.by_ref().take(INSERT_CHUNK).collect();
        if chunk.is_empty() {
            break;
        }
        written += insert_product_history(db, chunk).await?;
    }

    Ok(written)
}

/// Turn a day's per-product aggregate into card-history `ActiveModel`s for the cards
/// we actually hold (whose `tcgplayer_id` is present in `map`) that carry at least one
/// price. `eur`/`tix` stay `NULL` — TCGCSV is USD-only.
fn build_day_rows(
    map: &HashMap<i64, i32>,
    aggregate: &HashMap<i64, DayPrice>,
    date_str: &str,
    now: DateTimeUtc,
) -> Vec<card_price_history::ActiveModel> {
    aggregate
        .iter()
        .filter_map(|(product_id, day)| {
            let card_id = *map.get(product_id)?;
            if day.usd.is_none() && day.usd_foil.is_none() {
                return None;
            }
            Some(card_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                card_id: Set(card_id),
                as_of_date: Set(date_str.to_string()),
                price_usd: Set(day.usd.clone()),
                price_usd_foil: Set(day.usd_foil.clone()),
                price_eur: Set(None),
                price_tix: Set(None),
                created_at: Set(now),
            })
        })
        .collect()
}

/// The sealed-product mirror of [`build_day_rows`]: a day's aggregate joined onto our
/// `products` rows by `productId`, into `product_price_history` `ActiveModel`s.
fn build_product_day_rows(
    product_map: &HashMap<i64, i32>,
    aggregate: &HashMap<i64, DayPrice>,
    date_str: &str,
    now: DateTimeUtc,
) -> Vec<product_price_history::ActiveModel> {
    aggregate
        .iter()
        .filter_map(|(product_id, day)| {
            let internal_id = *product_map.get(product_id)?;
            if day.usd.is_none() && day.usd_foil.is_none() {
                return None;
            }
            Some(product_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(internal_id),
                as_of_date: Set(date_str.to_string()),
                price_usd: Set(day.usd.clone()),
                price_usd_foil: Set(day.usd_foil.clone()),
                created_at: Set(now),
            })
        })
        .collect()
}

/// Insert history rows, **skipping** any `(game, card, date)` that already exists
/// (`ON CONFLICT DO NOTHING`) so a real Scryfall snapshot is never overwritten.
/// Returns the number of rows actually inserted.
async fn insert_history(
    db: &DatabaseConnection,
    rows: Vec<card_price_history::ActiveModel>,
) -> Result<u64, BackfillError> {
    if rows.is_empty() {
        return Ok(0);
    }
    let result = CardPriceHistory::insert_many(rows)
        .on_conflict(
            OnConflict::columns([
                card_price_history::Column::Game,
                card_price_history::Column::CardId,
                card_price_history::Column::AsOfDate,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec_without_returning(db)
        .await;
    match result {
        Ok(affected) => Ok(affected),
        // Every row in the chunk conflicted (all already present) — not an error.
        Err(DbErr::RecordNotInserted) => Ok(0),
        Err(err) => Err(err.into()),
    }
}

/// Insert product history rows, **skipping** any `(game, product, date)` that already
/// exists (`ON CONFLICT DO NOTHING`) so a real daily snapshot is never overwritten.
/// Returns the number of rows actually inserted.
async fn insert_product_history(
    db: &DatabaseConnection,
    rows: Vec<product_price_history::ActiveModel>,
) -> Result<u64, BackfillError> {
    if rows.is_empty() {
        return Ok(0);
    }
    let result = ProductPriceHistory::insert_many(rows)
        .on_conflict(
            OnConflict::columns([
                product_price_history::Column::Game,
                product_price_history::Column::ProductId,
                product_price_history::Column::AsOfDate,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec_without_returning(db)
        .await;
    match result {
        Ok(affected) => Ok(affected),
        Err(DbErr::RecordNotInserted) => Ok(0),
        Err(err) => Err(err.into()),
    }
}

/// Extract only category `category_id`'s `prices` files from a day's archive and
/// fold them into one per-product aggregate. Pure/synchronous (no DB, no network):
/// decodes the solid PPMd block once, streaming each entry.
fn extract_and_aggregate(
    bytes: &[u8],
    category_id: u32,
) -> Result<HashMap<i64, DayPrice>, BackfillError> {
    let mut reader = ArchiveReader::new(Cursor::new(bytes), Password::empty())
        .map_err(|e| BackfillError::Archive(e.to_string()))?;

    let mut records = Vec::new();
    reader
        .for_each_entries(|entry, entry_reader| {
            // A solid block must be read in order: every entry's stream has to be
            // fully consumed (even ones we skip) to keep the decoder aligned.
            if entry.is_directory() || !is_prices_entry(entry.name(), category_id) {
                io::copy(entry_reader, &mut io::sink())?;
                return Ok(true);
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry_reader.read_to_end(&mut buf)?;
            match serde_json::from_slice::<PriceFile>(&buf) {
                Ok(file) => records.extend(file.results),
                // A single malformed prices file shouldn't abort the whole day.
                Err(err) => tracing::warn!(entry = entry.name(), error = %err, "skipping bad prices file"),
            }
            Ok(true)
        })
        .map_err(|e| BackfillError::Archive(e.to_string()))?;

    Ok(aggregate_prices(records))
}

/// Whether an archive entry is a `prices` file for `category_id`. Entry paths look
/// like `{date}/{categoryId}/{groupId}/prices`, so the category is the segment two
/// before the final `prices` segment.
fn is_prices_entry(name: &str, category_id: u32) -> bool {
    let parts: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    parts.len() >= 3
        && parts[parts.len() - 1] == "prices"
        && parts[parts.len() - 3] == category_id.to_string()
}

/// Build the `tcgplayer_id -> cards.id` map for the game (only cards that carry an
/// id). Keyed by `i64` to match the archive's `productId`.
async fn load_tcgplayer_map(
    db: &DatabaseConnection,
) -> Result<HashMap<i64, i32>, BackfillError> {
    let rows: Vec<(i32, Option<i32>)> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::TcgplayerId)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::TcgplayerId.is_not_null())
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, tcg)| tcg.map(|t| (i64::from(t), id)))
        .collect())
}

/// Build the `productId -> products.id` map for the game. The `products.external_id`
/// stores the TCGplayer `productId` as a string, so this parses it back to the `i64`
/// the archive aggregate is keyed by (skipping any unparseable id).
async fn load_product_map(
    db: &DatabaseConnection,
) -> Result<HashMap<i64, i32>, BackfillError> {
    let rows: Vec<(i32, String)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::ExternalId)
        .filter(product::Column::Game.eq(GAME))
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, ext)| ext.parse::<i64>().ok().map(|pid| (pid, id)))
        .collect())
}

// ----- ingest_state bookkeeping (dataset = tcgcsv_price_backfill) -----

async fn load_state(
    db: &DatabaseConnection,
) -> Result<Option<ingest_state::Model>, BackfillError> {
    Ok(IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(DATASET))
        .one(db)
        .await?)
}

/// Upsert the backfill's `ingest_state` row. `last_date` is stored in
/// `source_updated_at` as the resume cursor (the last completed archive date).
async fn put_state(
    db: &DatabaseConnection,
    status: &str,
    last_date: Option<&str>,
    detail: &str,
    started_at: DateTimeUtc,
    finished_at: Option<DateTimeUtc>,
    rows_total: i64,
) -> Result<(), BackfillError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(last_date.map(str::to_string)),
        status: Set(status.to_string()),
        detail: Set(Some(detail.to_string())),
        sets_imported: Set(0),
        // Cumulative history rows inserted; clamped to i32's range for the column.
        cards_imported: Set(rows_total.min(i64::from(i32::MAX)) as i32),
        started_at: Set(Some(started_at)),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns([
                    ingest_state::Column::SourceUpdatedAt,
                    ingest_state::Column::Status,
                    ingest_state::Column::Detail,
                    ingest_state::Column::CardsImported,
                    ingest_state::Column::StartedAt,
                    ingest_state::Column::FinishedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

async fn put_running(
    db: &DatabaseConnection,
    started: DateTimeUtc,
    last_date: Option<NaiveDate>,
    detail: &str,
    rows_total: i64,
) -> Result<(), BackfillError> {
    let last = last_date.map(|d| d.format("%Y-%m-%d").to_string());
    put_state(db, "running", last.as_deref(), detail, started, None, rows_total).await
}

async fn finish(
    db: &DatabaseConnection,
    started: DateTimeUtc,
    detail: &str,
    rows_total: i64,
) -> Result<(), BackfillError> {
    // Preserve the resume cursor and the cumulative row count already stored (a fresh
    // read avoids clobbering them when this run added nothing, e.g. a resumed finish).
    let existing = load_state(db).await?;
    let last = existing.as_ref().and_then(|s| s.source_updated_at.clone());
    let total = rows_total.max(existing.map(|s| i64::from(s.cards_imported)).unwrap_or(0));
    put_state(
        db,
        "complete",
        last.as_deref(),
        detail,
        started,
        Some(Utc::now()),
        total,
    )
    .await
}

async fn mark_error(db: &DatabaseConnection, message: &str) -> Result<(), BackfillError> {
    let existing = load_state(db).await?;
    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    let last = existing.and_then(|s| s.source_updated_at);
    let detail: String = message.chars().take(500).collect();
    put_state(db, "error", last.as_deref(), &detail, started, Some(Utc::now()), 0).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, EntityTrait};

    #[test]
    fn recognises_category_prices_entries() {
        // Category 1 (Magic) prices file: matched.
        assert!(is_prices_entry("2024-02-08/1/2377/prices", 1));
        assert!(is_prices_entry("2024-02-08\\1\\2377\\prices", 1));
        // Another category's prices file: not matched.
        assert!(!is_prices_entry("2024-02-08/2/100/prices", 1));
        // A group directory or a non-prices file: not matched.
        assert!(!is_prices_entry("2024-02-08/1/2377/", 1));
        assert!(!is_prices_entry("2024-02-08/1/2377/products", 1));
    }

    #[test]
    fn builds_rows_only_for_owned_priced_products() {
        let map: HashMap<i64, i32> = HashMap::from([(100, 7), (200, 8)]);
        let mut agg: HashMap<i64, DayPrice> = HashMap::new();
        agg.insert(
            100,
            DayPrice {
                usd: Some("0.25".into()),
                usd_foil: Some("1.50".into()),
            },
        );
        // Owned but no price → dropped.
        agg.insert(200, DayPrice { usd: None, usd_foil: None });
        // Priced but not owned (no card) → dropped.
        agg.insert(
            999,
            DayPrice {
                usd: Some("9.00".into()),
                usd_foil: None,
            },
        );
        let now = Utc::now();
        let rows = build_day_rows(&map, &agg, "2024-02-08", now);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.card_id.as_ref(), &7);
        assert_eq!(row.as_of_date.as_ref(), "2024-02-08");
        assert_eq!(row.price_usd.as_ref().as_deref(), Some("0.25"));
        assert_eq!(row.price_usd_foil.as_ref().as_deref(), Some("1.50"));
        assert!(row.price_eur.as_ref().is_none());
    }

    #[test]
    fn builds_product_rows_only_for_matched_priced_products() {
        // Product map: productId 100 -> product row 42; 200 -> 43 (owned but unpriced);
        // 999 is priced but not in our products table.
        let product_map: HashMap<i64, i32> = HashMap::from([(100, 42), (200, 43)]);
        let mut agg: HashMap<i64, DayPrice> = HashMap::new();
        agg.insert(
            100,
            DayPrice {
                usd: Some("199.99".into()),
                usd_foil: None,
            },
        );
        agg.insert(200, DayPrice { usd: None, usd_foil: None });
        agg.insert(
            999,
            DayPrice {
                usd: Some("9.00".into()),
                usd_foil: None,
            },
        );
        let rows = build_product_day_rows(&product_map, &agg, "2024-02-08", Utc::now());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].product_id.as_ref(), &42);
        assert_eq!(rows[0].price_usd.as_ref().as_deref(), Some("199.99"));
    }

    #[tokio::test]
    async fn insert_history_skips_existing_card_date_rows() {
        use crate::entities::prelude::CardPriceHistory;

        let db = crate::test_support::migrated_memory_db().await;
        let card_id = crate::test_support::insert_card(&db, "ext-a").await;
        let now = Utc::now();

        // Simulate an existing Scryfall daily snapshot for this (card, date).
        card_price_history::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            card_id: Set(card_id),
            as_of_date: Set("2024-02-08".to_string()),
            price_usd: Set(Some("5.00".to_string())),
            price_usd_foil: Set(None),
            price_eur: Set(Some("4.00".to_string())),
            price_tix: Set(None),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert existing snapshot");

        // Backfill the same (card, date) with different prices — must be skipped.
        let rows = vec![card_price_history::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            card_id: Set(card_id),
            as_of_date: Set("2024-02-08".to_string()),
            price_usd: Set(Some("0.25".to_string())),
            price_usd_foil: Set(Some("1.50".to_string())),
            price_eur: Set(None),
            price_tix: Set(None),
            created_at: Set(now),
        }];
        let written = insert_history(&db, rows).await.expect("insert ok");
        assert_eq!(written, 0, "existing (card,date) must not be overwritten");

        // The original snapshot is intact.
        let all = CardPriceHistory::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].price_usd.as_deref(), Some("5.00"));
        assert_eq!(all[0].price_eur.as_deref(), Some("4.00"));

        // A new (card, date) is inserted normally.
        let new_rows = vec![card_price_history::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            card_id: Set(card_id),
            as_of_date: Set("2024-02-09".to_string()),
            price_usd: Set(Some("0.30".to_string())),
            price_usd_foil: Set(None),
            price_eur: Set(None),
            price_tix: Set(None),
            created_at: Set(now),
        }];
        let written = insert_history(&db, new_rows).await.expect("insert ok");
        assert_eq!(written, 1, "a fresh (card,date) is inserted");
    }
}
