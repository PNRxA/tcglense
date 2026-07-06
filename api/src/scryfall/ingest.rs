//! Streaming import of Scryfall's `default_cards` bulk file into the `cards`
//! and `card_sets` tables, plus the `/sets` metadata.
//!
//! The bulk file is a single JSON array with **one object per line**, so we
//! stream it line-by-line (decompressing gzip on the fly) and upsert in batches
//! — memory stays bounded regardless of the ~500 MB download. Only paper cards
//! and non-digital sets are stored. Progress and completion are recorded in
//! `ingest_state` so the API/UI can report status and so an unchanged dataset
//! (same `updated_at`) is skipped on the next boot.

use std::collections::HashSet;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, Iterable, QueryFilter,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use super::client;
use super::map;
use super::model::{ScryfallCard, ScryfallSet};
use super::progress::ImportProgress;
use super::{DATASET, GAME, GAME_NAME};
use crate::datasets::SyncSource;
use crate::db::upsert_changed_guard;
use crate::entities::prelude::{Card, CardSet, IngestState};
use crate::entities::{card, card_set, ingest_state};

/// Rows per upsert. ~65 card columns × 400 ≈ 26k bound parameters, under SQLite's
/// default 32 766 parameter limit (drop this toward 350 if the column count grows).
pub(super) const CARD_BATCH: usize = 400;
const SET_BATCH: usize = 300;
/// Emit a progress update to `ingest_state` every this many flushed card batches.
const PROGRESS_EVERY: u32 = 25;
/// Push accumulated stream bytes to the progress bar in chunks this large, so it
/// stays smooth even across long runs of filtered lines (which never flush a
/// card batch) without locking the bar on every single line.
const BYTES_PER_TICK: u64 = 1_000_000;

/// Error type for the background import. Its `Display` wraps the inner
/// reqwest/io/db error verbatim and is for **logs only** — the caller
/// ([`crate::catalog::refresh_all`]) records it at `error` level. Anything persisted
/// into the client-visible `ingest_state.detail` (surfaced by the public
/// `GET /status` endpoint) must go through [`IngestError::public_detail`] instead, so
/// upstream URLs, filesystem paths, or SQL text never reach a client.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("{0}")]
    Other(String),
}

impl IngestError {
    /// A client-safe summary for `ingest_state.detail`. The `Http`/`Io`/`Db` variants
    /// wrap an external error whose `Display` can leak internal URLs, host paths, or SQL
    /// detail, so they collapse to a coarse category; `Other` is already a hand-written
    /// message, so it passes through. The full error stays in the logs.
    pub(super) fn public_detail(&self) -> String {
        match self {
            IngestError::Http(_) => "network error contacting the card-data source".to_string(),
            IngestError::Io(_) => "i/o error while importing card data".to_string(),
            IngestError::Db(_) => "database error while importing card data".to_string(),
            IngestError::Other(msg) => msg.clone(),
        }
    }
}

/// Import lifecycle recorded in `ingest_state.status`. `put_state` is the only
/// writer, and only ever records these three (a row is absent, not `"idle"`,
/// before the first import).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IngestStatus {
    Running,
    Complete,
    Error,
}

impl IngestStatus {
    fn as_str(self) -> &'static str {
        match self {
            IngestStatus::Running => "running",
            IngestStatus::Complete => "complete",
            IngestStatus::Error => "error",
        }
    }
}

/// Named fields for a `put_state` upsert. `Default` yields all-`None`/zero, so a
/// call site sets only the fields it means to record via `..Default::default()`.
#[derive(Default)]
pub(super) struct IngestStateUpdate {
    pub(super) source_updated_at: Option<String>,
    pub(super) detail: Option<String>,
    pub(super) sets_imported: i32,
    pub(super) cards_imported: i32,
    pub(super) started_at: Option<DateTimeUtc>,
    pub(super) finished_at: Option<DateTimeUtc>,
}

/// Refresh MTG card data from Scryfall, recording status in `ingest_state`.
///
/// On error the state row is best-effort marked `"error"` so the next boot
/// retries, and the error is returned for logging by the caller.
pub async fn refresh(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
) -> Result<(), IngestError> {
    match refresh_inner(db, client, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = put_state(
                db,
                IngestStatus::Error,
                IngestStateUpdate {
                    detail: Some(truncate(&err.public_detail(), 500)),
                    finished_at: Some(Utc::now()),
                    ..Default::default()
                },
            )
            .await;
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
        .find(|b| b.kind == DATASET)
        .ok_or_else(|| {
            IngestError::Other(format!("scryfall bulk dataset '{DATASET}' not found"))
        })?;

    // Skip if we already imported this exact version.
    let existing = IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(DATASET))
        .one(db)
        .await?;
    if let Some(state) = &existing
        && state.status == IngestStatus::Complete.as_str()
        && state.source_updated_at.as_deref() == Some(entry.updated_at.as_str())
    {
        tracing::info!(updated_at = %entry.updated_at, "scryfall {DATASET} already up to date");
        return Ok(());
    }

    let started = Utc::now();
    // Live terminal progress: a spinner while set metadata is fetched, then a
    // determinate byte bar for the long card stream (see `super::progress`).
    // Dropping it (incl. on any `?` below) closes the span and clears the bar.
    let progress = ImportProgress::start(GAME_NAME);
    tracing::info!(
        updated_at = %entry.updated_at,
        size_mb = entry.size.unwrap_or(0) / 1_000_000,
        "importing scryfall {DATASET}"
    );
    put_state(
        db,
        IngestStatus::Running,
        IngestStateUpdate {
            source_updated_at: Some(entry.updated_at.clone()),
            detail: Some("importing sets".into()),
            started_at: Some(started),
            ..Default::default()
        },
    )
    .await?;

    // Sets first, so cards can reference stored sets.
    let sets = client::all_sets(client, &source.scryfall_sets_url()).await?;
    let paper_codes: HashSet<String> = sets
        .iter()
        .filter(|s| !s.digital.unwrap_or(false))
        .map(|s| s.code.to_lowercase())
        .collect();
    let sets_imported = import_sets(db, &sets).await?;
    put_state(
        db,
        IngestStatus::Running,
        IngestStateUpdate {
            source_updated_at: Some(entry.updated_at.clone()),
            detail: Some("importing cards".into()),
            sets_imported,
            started_at: Some(started),
            ..Default::default()
        },
    )
    .await?;

    // Switch the bar to its determinate phase now that the byte length is known.
    progress.begin_cards(entry.size);
    // In mirror mode the bulk file streams from the mirror (overriding the catalog's
    // embedded upstream `download_uri`, which points at Scryfall's own CDN); upstream
    // mode follows that `download_uri` directly.
    let download_url = source
        .scryfall_file_url(DATASET)
        .unwrap_or_else(|| entry.download_uri.clone());
    let cards_imported = import_cards(
        db,
        client,
        &download_url,
        &paper_codes,
        &entry.updated_at,
        sets_imported,
        started,
        &progress,
    )
    .await?;

    // A run that fetched everything but stored nothing means the data was bad
    // (e.g. the bulk download served a non-card body, or the format drifted). Fail
    // it rather than recording "complete", which would version-lock the empty state
    // and suppress re-import on the next boot.
    if cards_imported == 0 {
        return Err(IngestError::Other(format!(
            "import produced 0 cards from {sets_imported} sets; treating as failure to retry"
        )));
    }

    // Clear the progress bar before the completion line so it prints cleanly.
    drop(progress);
    put_state(
        db,
        IngestStatus::Complete,
        IngestStateUpdate {
            source_updated_at: Some(entry.updated_at.clone()),
            sets_imported,
            cards_imported,
            started_at: Some(started),
            finished_at: Some(Utc::now()),
            ..Default::default()
        },
    )
    .await?;
    tracing::info!(
        sets = sets_imported,
        cards = cards_imported,
        "scryfall import complete"
    );
    Ok(())
}

/// Upsert all non-digital (paper) sets. Returns the number stored.
pub(super) async fn import_sets(
    db: &DatabaseConnection,
    sets: &[ScryfallSet],
) -> Result<i32, IngestError> {
    let now = Utc::now();
    let models: Vec<card_set::ActiveModel> = sets
        .iter()
        .filter(|s| !s.digital.unwrap_or(false))
        .map(|s| map::map_set(s, now))
        .collect();
    let count = models.len() as i32;

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card_set::ActiveModel> = iter.by_ref().take(SET_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        CardSet::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([card_set::Column::Game, card_set::Column::Code])
                    // Update every provider-owned column — all but the
                    // identity/conflict keys and created_at.
                    .update_columns(card_set::Column::iter().filter(|c| {
                        !matches!(
                            c,
                            card_set::Column::Id
                                | card_set::Column::Game
                                | card_set::Column::Code
                                | card_set::Column::CreatedAt
                        )
                    }))
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
    }
    Ok(count)
}

/// Stream the bulk card file and upsert paper cards in batches. Returns the
/// number stored.
#[allow(clippy::too_many_arguments)]
async fn import_cards(
    db: &DatabaseConnection,
    client: &Client,
    url: &str,
    paper_codes: &HashSet<String>,
    updated_at: &str,
    sets_imported: i32,
    started: DateTimeUtc,
    progress: &ImportProgress,
) -> Result<i32, IngestError> {
    let stream = client::download_stream(client, url).await?;
    let mut lines = BufReader::with_capacity(64 * 1024, StreamReader::new(stream)).lines();

    let now = Utc::now();
    let mut batch: Vec<card::ActiveModel> = Vec::with_capacity(CARD_BATCH);
    let mut total: i32 = 0;
    let mut batches_since_progress: u32 = 0;
    let mut skipped_parse: u64 = 0;
    let mut bytes_since_tick: u64 = 0;

    while let Some(raw) = lines.next_line().await? {
        // Account every byte read (incl. the stripped newline) toward the byte
        // bar, pushing in ~1 MB ticks. Doing it here — before the paper filter —
        // keeps the bar moving across long runs of filtered lines that never
        // flush a card batch.
        bytes_since_tick += raw.len() as u64 + 1;
        if bytes_since_tick >= BYTES_PER_TICK {
            progress.add_bytes(bytes_since_tick);
            bytes_since_tick = 0;
        }

        // Each element sits on its own line, terminated by a comma except the
        // last; the array brackets get their own lines.
        let line = raw.trim();
        let line = line.strip_suffix(',').unwrap_or(line).trim();
        if line.is_empty() || line == "[" || line == "]" {
            continue;
        }
        let scry: ScryfallCard = match serde_json::from_str(line) {
            Ok(card) => card,
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable card line");
                }
                continue;
            }
        };

        // Paper only, and only cards whose set we actually stored.
        let set_code = scry.set.to_lowercase();
        if !scry.games.iter().any(|g| g == "paper") || !paper_codes.contains(&set_code) {
            continue;
        }
        batch.push(map::map_card(scry, now));

        if batch.len() >= CARD_BATCH {
            let n = batch.len() as i32;
            flush_cards(db, std::mem::take(&mut batch)).await?;
            total += n;
            batch.reserve(CARD_BATCH);
            progress.set_cards(total as u64);
            batches_since_progress += 1;
            if batches_since_progress >= PROGRESS_EVERY {
                batches_since_progress = 0;
                let _ = put_state(
                    db,
                    IngestStatus::Running,
                    IngestStateUpdate {
                        source_updated_at: Some(updated_at.to_string()),
                        detail: Some(format!("imported {total} cards")),
                        sets_imported,
                        cards_imported: total,
                        started_at: Some(started),
                        ..Default::default()
                    },
                )
                .await;
            }
        }
    }

    if !batch.is_empty() {
        let n = batch.len() as i32;
        flush_cards(db, batch).await?;
        total += n;
        progress.set_cards(total as u64);
    }
    // Push any bytes counted since the last tick so the bar reaches its end.
    if bytes_since_tick > 0 {
        progress.add_bytes(bytes_since_tick);
    }
    if skipped_parse > 0 {
        tracing::warn!(count = skipped_parse, "skipped unparseable card lines");
    }
    Ok(total)
}

pub(super) async fn flush_cards(
    db: &DatabaseConnection,
    batch: Vec<card::ActiveModel>,
) -> Result<(), IngestError> {
    if batch.is_empty() {
        return Ok(());
    }
    Card::insert_many(batch)
        .on_conflict(
            OnConflict::columns([card::Column::Game, card::Column::ExternalId])
                // Update every provider-owned column — all but the
                // identity/conflict keys and created_at.
                .update_columns(card::Column::iter().filter(|c| {
                    !matches!(
                        c,
                        card::Column::Id
                            | card::Column::Game
                            | card::Column::ExternalId
                            | card::Column::CreatedAt
                    )
                }))
                // Skip the write entirely when the row is unchanged: on the daily
                // re-sync most of the ~100k cards are byte-identical, and rewriting them
                // would churn a new MVCC tuple + all indexes for nothing. `updated_at`
                // stays in the SET list (a real change still bumps it) but is excluded
                // from the compare, or its always-`now()` value would make every row look
                // changed and defeat the guard — so `cards.updated_at` now means "last
                // time a datum actually changed", not "last sync touch" (read nowhere
                // today; sitemap lastmod uses ingest_state/released_at).
                .action_and_where(upsert_changed_guard::<card::Column>("cards", |c| {
                    matches!(
                        c,
                        card::Column::Id
                            | card::Column::Game
                            | card::Column::ExternalId
                            | card::Column::CreatedAt
                            | card::Column::UpdatedAt
                    )
                }))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Upsert the single `ingest_state` row for `(GAME, DATASET)`. `status` stays a
/// required argument so `..Default::default()` on `update` can never silently
/// drop it.
pub(super) async fn put_state(
    db: &DatabaseConnection,
    status: IngestStatus,
    update: IngestStateUpdate,
) -> Result<(), IngestError> {
    let IngestStateUpdate {
        source_updated_at,
        detail,
        sets_imported,
        cards_imported,
        started_at,
        finished_at,
    } = update;
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(source_updated_at),
        status: Set(status.as_str().to_string()),
        detail: Set(detail),
        sets_imported: Set(sets_imported),
        cards_imported: Set(cards_imported),
        started_at: Set(started_at),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                // Update every column but the identity/conflict keys (id/game/dataset).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5).chars().count(), 6); // 5 chars + ellipsis
    }

    #[test]
    fn public_detail_never_leaks_the_wrapped_error() {
        // Io / Db wrap an external error whose `Display` can carry host paths or SQL
        // detail; `public_detail` (persisted into the client-visible `ingest_state.detail`)
        // must collapse them to a fixed category rather than echo the inner text.
        let io = IngestError::Io(std::io::Error::other("/secret/host/path leaked"));
        assert_eq!(io.public_detail(), "i/o error while importing card data");
        assert!(!io.public_detail().contains("secret"));

        let db = IngestError::Db(sea_orm::DbErr::Custom(
            "SELECT secret_column FROM internal_table".to_string(),
        ));
        assert_eq!(db.public_detail(), "database error while importing card data");
        assert!(!db.public_detail().contains("secret"));
        // The `Display` (log-only) still carries the full detail for operators.
        assert!(db.to_string().contains("secret_column"));

        // `Other` is a hand-written message, so it passes through unchanged.
        let other =
            IngestError::Other("scryfall bulk dataset 'default_cards' not found".to_string());
        assert_eq!(
            other.public_detail(),
            "scryfall bulk dataset 'default_cards' not found"
        );
    }
}
