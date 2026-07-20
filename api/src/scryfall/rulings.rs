//! Streaming import of Scryfall's `rulings` bulk file into the `card_rulings` table.
//!
//! Rulings are the "Notes and Rules Information" shown on a card (issue #522) — official
//! clarifications keyed by `oracle_id` (the gameplay identity shared across every
//! printing). Like the card bulk file, the rulings file is a single JSON array with one
//! object per line, so it streams line-by-line with bounded memory. We keep only rulings
//! whose `oracle_id` matches a card we actually store (paper cards), then swap the whole
//! game's rulings in one transaction so a refresh is atomic — a reader never sees a
//! half-rebuilt list, and there's no natural per-ruling id to upsert on. Version-gated on
//! the bulk file's `updated_at` via `ingest_state` `(mtg, rulings)`, so an unchanged
//! dataset is skipped on the next tick.

use std::collections::HashSet;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, TransactionTrait,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use super::client;
use super::ingest::IngestError;
use super::model::ScryfallRuling;
use super::{DATASET_RULINGS, GAME};
use crate::catalog::ingest_state::{self, StateFields};
use crate::datasets::SyncSource;
use crate::entities::prelude::{Card, CardRuling};
use crate::entities::{card, card_ruling};

/// Rows per insert batch. `card_rulings` has 6 columns, so 1000 × 6 = 6k bound
/// parameters — comfortably under SQLite's 32 766-parameter limit.
const RULING_BATCH: usize = 1000;

/// A ruling reduced to the fields we store, after parsing + filtering.
struct Row {
    oracle_id: String,
    source: String,
    published_at: String,
    comment: String,
}

/// Refresh MTG card rulings from Scryfall, recording status in `ingest_state`.
///
/// On error the `(mtg, rulings)` state row is best-effort marked `"error"` so the next
/// boot retries, and the error is returned for logging by the caller.
pub async fn refresh(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
) -> Result<(), IngestError> {
    match refresh_inner(db, client, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest_state::mark_error(db, GAME, DATASET_RULINGS, &err.public_detail()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
) -> Result<(), IngestError> {
    let entry = client::bulk_data(client, &source.scryfall_bulk_data_url())
        .await?
        .into_iter()
        .find(|b| b.kind == DATASET_RULINGS)
        .ok_or_else(|| {
            IngestError::Other(format!(
                "scryfall bulk dataset '{DATASET_RULINGS}' not found"
            ))
        })?;

    // Skip if we already imported this exact version.
    if let Some(state) = ingest_state::load(db, GAME, DATASET_RULINGS).await?
        && state.status == "complete"
        && state.source_updated_at.as_deref() == Some(entry.updated_at.as_str())
    {
        tracing::info!(updated_at = %entry.updated_at, "scryfall {DATASET_RULINGS} already up to date");
        return Ok(());
    }

    let started = Utc::now();
    tracing::info!(
        updated_at = %entry.updated_at,
        size_mb = entry.size.unwrap_or(0) / 1_000_000,
        "importing scryfall {DATASET_RULINGS}"
    );
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET_RULINGS,
            status: "running",
            source_updated_at: Some(&entry.updated_at),
            detail: "importing rulings",
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: None,
        },
    )
    .await?;

    // Rulings key on `oracle_id`; keep only those for cards we actually store, so the
    // table stays scoped to the (paper) catalog. Loading the distinct set first also
    // bounds how many rows the stream collects.
    let known = known_oracle_ids(db).await?;

    // In mirror mode the file streams from the mirror (overriding the catalog's embedded
    // upstream `download_uri`); upstream mode follows that `download_uri` directly.
    let download_url = source
        .scryfall_file_url(DATASET_RULINGS)
        .unwrap_or_else(|| entry.download_uri.clone());
    let rows = collect_rulings(client, &download_url, &known).await?;
    let count = rows.len() as i32;

    // A run that fetched everything but matched nothing while we DO hold cards means the
    // download was bad (empty/garbage body, or a format drift). Fail it — before touching
    // the table — rather than wiping good rulings and version-locking the empty state.
    if count == 0 && !known.is_empty() {
        return Err(IngestError::Other(
            "rulings import produced 0 rows despite stored cards; treating as failure to retry"
                .to_string(),
        ));
    }

    replace_rulings(db, rows).await?;

    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET_RULINGS,
            status: "complete",
            source_updated_at: Some(&entry.updated_at),
            detail: &format!("imported {count} rulings"),
            sets_imported: 0,
            cards_imported: count,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(rulings = count, "scryfall rulings import complete");
    Ok(())
}

/// The game's distinct, non-null card `oracle_id`s, for scoping rulings to stored cards.
async fn known_oracle_ids(db: &DatabaseConnection) -> Result<HashSet<String>, IngestError> {
    let ids = Card::find()
        .select_only()
        .column(card::Column::OracleId)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::OracleId.is_not_null())
        .distinct()
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .collect();
    Ok(ids)
}

/// Stream the bulk rulings file and collect the rulings for a stored card into memory
/// (bounded by the filter). Each element sits on its own line, comma-terminated except
/// the last, with the array brackets on their own lines — same shape as the card file.
async fn collect_rulings(
    client: &Client,
    url: &str,
    known: &HashSet<String>,
) -> Result<Vec<Row>, IngestError> {
    let stream = client::download_stream(client, url).await?;
    let mut lines = BufReader::with_capacity(64 * 1024, StreamReader::new(stream)).lines();

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped_parse: u64 = 0;
    while let Some(raw) = lines.next_line().await? {
        let line = raw.trim();
        let line = line.strip_suffix(',').unwrap_or(line).trim();
        if line.is_empty() || line == "[" || line == "]" {
            continue;
        }
        let ruling: ScryfallRuling = match serde_json::from_str(line) {
            Ok(ruling) => ruling,
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable ruling line");
                }
                continue;
            }
        };
        // Keep only complete rulings (an `oracle_id` + a non-empty `comment`) for a card
        // we store.
        let (Some(oracle_id), Some(comment)) = (ruling.oracle_id, ruling.comment) else {
            continue;
        };
        if comment.is_empty() || !known.contains(&oracle_id) {
            continue;
        }
        rows.push(Row {
            oracle_id,
            source: ruling.source.unwrap_or_default(),
            published_at: ruling.published_at.unwrap_or_default(),
            comment,
        });
    }
    if skipped_parse > 0 {
        tracing::warn!(count = skipped_parse, "skipped unparseable ruling lines");
    }
    Ok(rows)
}

/// Swap the whole game's rulings atomically: delete every row + re-insert the fresh set
/// in one transaction, so a concurrent reader sees either the old list or the new one,
/// never a half-rebuilt one. The network download already finished, so the transaction is
/// DB-only and short-lived.
async fn replace_rulings(db: &DatabaseConnection, rows: Vec<Row>) -> Result<(), IngestError> {
    let now = Utc::now();
    let txn = db.begin().await?;
    CardRuling::delete_many()
        .filter(card_ruling::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    let mut iter = rows.into_iter();
    loop {
        let chunk: Vec<card_ruling::ActiveModel> = iter
            .by_ref()
            .take(RULING_BATCH)
            .map(|r| card_ruling::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                oracle_id: Set(r.oracle_id),
                source: Set(r.source),
                published_at: Set(r.published_at),
                comment: Set(r.comment),
                created_at: Set(now),
            })
            .collect();
        if chunk.is_empty() {
            break;
        }
        CardRuling::insert_many(chunk)
            .exec_without_returning(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(())
}
