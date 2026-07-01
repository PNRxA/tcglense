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
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
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
use crate::entities::prelude::{Card, CardSet, IngestState};
use crate::entities::{card, card_set, ingest_state};

/// Rows per upsert. ~34 card columns × 400 ≈ 13.6k bound parameters, comfortably
/// under SQLite's default 32 766 parameter limit.
pub(super) const CARD_BATCH: usize = 400;
const SET_BATCH: usize = 300;
/// Emit a progress update to `ingest_state` every this many flushed card batches.
const PROGRESS_EVERY: u32 = 25;
/// Push accumulated stream bytes to the progress bar in chunks this large, so it
/// stays smooth even across long runs of filtered lines (which never flush a
/// card batch) without locking the bar on every single line.
const BYTES_PER_TICK: u64 = 1_000_000;

/// Error type for the background import. Logged, never surfaced to a request.
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

/// Refresh MTG card data from Scryfall, recording status in `ingest_state`.
///
/// On error the state row is best-effort marked `"error"` so the next boot
/// retries, and the error is returned for logging by the caller.
pub async fn refresh(db: &DatabaseConnection, client: &Client) -> Result<(), IngestError> {
    match refresh_inner(db, client).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = put_state(
                db,
                None,
                "error",
                Some(truncate(&err.to_string(), 500)),
                0,
                0,
                None,
                Some(Utc::now()),
            )
            .await;
            Err(err)
        }
    }
}

async fn refresh_inner(db: &DatabaseConnection, client: &Client) -> Result<(), IngestError> {
    let entry = client::bulk_data(client)
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
        && state.status == "complete"
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
        Some(entry.updated_at.clone()),
        "running",
        Some("importing sets".into()),
        0,
        0,
        Some(started),
        None,
    )
    .await?;

    // Sets first, so cards can reference stored sets.
    let sets = client::all_sets(client).await?;
    let paper_codes: HashSet<String> = sets
        .iter()
        .filter(|s| !s.digital.unwrap_or(false))
        .map(|s| s.code.to_lowercase())
        .collect();
    let sets_imported = import_sets(db, &sets).await?;
    put_state(
        db,
        Some(entry.updated_at.clone()),
        "running",
        Some("importing cards".into()),
        sets_imported,
        0,
        Some(started),
        None,
    )
    .await?;

    // Switch the bar to its determinate phase now that the byte length is known.
    progress.begin_cards(entry.size);
    let cards_imported = import_cards(
        db,
        client,
        &entry.download_uri,
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
        Some(entry.updated_at.clone()),
        "complete",
        None,
        sets_imported,
        cards_imported,
        Some(started),
        Some(Utc::now()),
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
                    .update_columns([
                        card_set::Column::Name,
                        card_set::Column::SetType,
                        card_set::Column::ReleasedAt,
                        card_set::Column::CardCount,
                        card_set::Column::Digital,
                        card_set::Column::IconSvgUri,
                        card_set::Column::ParentSetCode,
                        card_set::Column::ExternalId,
                        card_set::Column::UpdatedAt,
                    ])
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
                    Some(updated_at.to_string()),
                    "running",
                    Some(format!("imported {total} cards")),
                    sets_imported,
                    total,
                    Some(started),
                    None,
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
                .update_columns([
                    card::Column::OracleId,
                    card::Column::Name,
                    card::Column::SetCode,
                    card::Column::SetName,
                    card::Column::CollectorNumber,
                    card::Column::CollectorNumberInt,
                    card::Column::Rarity,
                    card::Column::Lang,
                    card::Column::ReleasedAt,
                    card::Column::ManaCost,
                    card::Column::Cmc,
                    card::Column::TypeLine,
                    card::Column::ColorIdentity,
                    card::Column::Colors,
                    card::Column::Layout,
                    card::Column::OracleText,
                    card::Column::Power,
                    card::Column::Toughness,
                    card::Column::Loyalty,
                    card::Column::ImageSmall,
                    card::Column::ImageNormal,
                    card::Column::ImageLarge,
                    card::Column::ImageArtCrop,
                    card::Column::ImagePng,
                    card::Column::CardFaces,
                    card::Column::PriceUsd,
                    card::Column::PriceUsdFoil,
                    card::Column::PriceEur,
                    card::Column::PriceTix,
                    card::Column::Digital,
                    card::Column::UpdatedAt,
                ])
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

/// Upsert the single `ingest_state` row for `(GAME, DATASET)`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn put_state(
    db: &DatabaseConnection,
    source_updated_at: Option<String>,
    status: &str,
    detail: Option<String>,
    sets_imported: i32,
    cards_imported: i32,
    started_at: Option<DateTimeUtc>,
    finished_at: Option<DateTimeUtc>,
) -> Result<(), IngestError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(source_updated_at),
        status: Set(status.to_string()),
        detail: Set(detail),
        sets_imported: Set(sets_imported),
        cards_imported: Set(cards_imported),
        started_at: Set(started_at),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns([
                    ingest_state::Column::SourceUpdatedAt,
                    ingest_state::Column::Status,
                    ingest_state::Column::Detail,
                    ingest_state::Column::SetsImported,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5).chars().count(), 6); // 5 chars + ellipsis
    }
}
