//! Streaming import of Scryfall's `art_tags` bulk file into `art_tags` + `card_art_tags`.
//!
//! Art tags are community Tagger labels describing what a card's artwork depicts
//! (issue #140) — the data behind the `art:` / `arttag:` / `atag:` search filters. They
//! key on `illustration_id` (the artwork identity shared by reprints of the same
//! painting), which `cards.illustration_id` stores. Like the rulings file, the bulk file
//! is a single JSON array with one object per line, so it streams line-by-line; we keep
//! only taggings whose artwork belongs to a card we actually store, expand the tag
//! hierarchy at ingest ([`expand`]) so a parent tag like `animal` matches its
//! descendants' artworks without a query-time tree walk, then swap both tables in one
//! transaction so a refresh is atomic. Version-gated via `ingest_state` `(mtg,
//! art_tags)` on the bulk file's `updated_at` **combined with** the card dataset's
//! imported version (the mapping derives from both inputs), so an unchanged pair is
//! skipped on the next tick and a card re-import rebuilds the mapping.
//!
//! Known limitation: `cards.illustration_id` is the catalog's *flattened* artwork id
//! (the first face carrying one — see `map::map_card`), so a tagging that applies only
//! to a non-first face's artwork (e.g. a transform card's back face) is dropped here
//! and `art:` won't match that card. Fixing that means per-face artwork identity on
//! `cards` (it equally affects `unique:art` grouping) — deferred with that work.

mod expand;

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
use super::model::ScryfallArtTag;
use super::{DATASET, DATASET_ART_TAGS, GAME};
use crate::catalog::ingest_state::{self, StateFields};
use crate::datasets::SyncSource;
use crate::entities::prelude::{ArtTag, Card, CardArtTag};
use crate::entities::{art_tag, card, card_art_tag};

use expand::{Expanded, TagInput};

/// Rows per `art_tags` insert batch: 7 bound columns, so 1000 × 7 = 7k parameters —
/// comfortably under SQLite's 32 766-parameter limit.
const TAG_BATCH: usize = 1000;
/// Rows per `card_art_tags` insert batch: 3 bound columns, so 4000 × 3 = 12k parameters.
/// The mapping table is the big one (~1M rows), so batches run larger to cut round trips.
const MAPPING_BATCH: usize = 4000;

/// Refresh MTG art tags from Scryfall, recording status in `ingest_state`.
///
/// On error the `(mtg, art_tags)` state row is best-effort marked `"error"` so the next
/// boot retries, and the error is returned for logging by the caller.
pub async fn refresh(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
) -> Result<(), IngestError> {
    match refresh_inner(db, client, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ =
                ingest_state::mark_error(db, GAME, DATASET_ART_TAGS, &err.public_detail()).await;
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
        .find(|b| b.kind == DATASET_ART_TAGS)
        .ok_or_else(|| {
            IngestError::Other(format!(
                "scryfall bulk dataset '{DATASET_ART_TAGS}' not found"
            ))
        })?;

    // The mapping derives from BOTH inputs — the art-tags file *and* the card
    // catalog's illustration ids it's scoped to — so the version gate folds the card
    // dataset's imported version into its value: a card re-import (new sets → new
    // artworks) rebuilds the mapping even when the tag file itself is unchanged.
    let cards_version = ingest_state::load(db, GAME, DATASET)
        .await?
        .and_then(|s| s.source_updated_at)
        .unwrap_or_default();
    let source_version = format!("{} (cards {cards_version})", entry.updated_at);

    // Skip if we already imported this exact input pair.
    if let Some(state) = ingest_state::load(db, GAME, DATASET_ART_TAGS).await?
        && state.status == "complete"
        && state.source_updated_at.as_deref() == Some(source_version.as_str())
    {
        tracing::info!(updated_at = %entry.updated_at, "scryfall {DATASET_ART_TAGS} already up to date");
        return Ok(());
    }

    // Taggings key on `illustration_id`; keep only those for artworks we actually store,
    // so the table stays scoped to the (paper) catalog — same posture as rulings. No
    // stored artworks (fresh DB, or the card import failed this tick) means building
    // now would swap in empty tables and stamp them complete — defer instead, leaving
    // the previous state so the next tick retries once cards exist.
    let known = known_illustration_ids(db).await?;
    if known.is_empty() {
        tracing::info!("no stored artworks yet; deferring scryfall {DATASET_ART_TAGS} import");
        return Ok(());
    }

    let started = Utc::now();
    tracing::info!(
        updated_at = %entry.updated_at,
        size_mb = entry.size.unwrap_or(0) / 1_000_000,
        "importing scryfall {DATASET_ART_TAGS}"
    );
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET_ART_TAGS,
            status: "running",
            source_updated_at: Some(&source_version),
            detail: "importing art tags",
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: None,
        },
    )
    .await?;

    // In mirror mode the file streams from the mirror (overriding the catalog's embedded
    // upstream `download_uri`); upstream mode follows that `download_uri` directly.
    let download_url = source
        .scryfall_file_url(DATASET_ART_TAGS)
        .unwrap_or_else(|| entry.download_uri.clone());
    let inputs = collect_tags(client, &download_url).await?;
    let expanded = expand::expand(inputs, &known);

    // A run that fetched everything but matched nothing — while we DO hold artworks
    // (guaranteed by the empty-`known` deferral above) — means the download was bad
    // (empty/garbage body, or a format drift). Fail it — before touching the tables —
    // rather than wiping good tags and version-locking the empty state.
    if expanded.rows == 0 {
        return Err(IngestError::Other(
            "art-tag import produced 0 rows despite stored artworks; treating as failure to retry"
                .to_string(),
        ));
    }

    let tag_count = expanded.tags.len() as i32;
    let row_count = expanded.rows as i32;
    replace_art_tags(db, expanded).await?;

    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET_ART_TAGS,
            status: "complete",
            source_updated_at: Some(&source_version),
            detail: &format!("imported {tag_count} art tags ({row_count} artwork mappings)"),
            sets_imported: tag_count,
            cards_imported: row_count,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(
        tags = tag_count,
        mappings = row_count,
        "scryfall art tags import complete"
    );
    Ok(())
}

/// The game's distinct, non-null card `illustration_id`s, for scoping taggings to
/// stored artworks.
async fn known_illustration_ids(db: &DatabaseConnection) -> Result<HashSet<String>, IngestError> {
    let ids = Card::find()
        .select_only()
        .column(card::Column::IllustrationId)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::IllustrationId.is_not_null())
        .distinct()
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .collect();
    Ok(ids)
}

/// Stream the bulk art-tags file and parse each tag line. Each element sits on its own
/// line, comma-terminated except the last, with the array brackets on their own lines —
/// same shape as the card and rulings files (a single tag line can run to ~1MB; the
/// reader's line buffer grows as needed).
async fn collect_tags(client: &Client, url: &str) -> Result<Vec<TagInput>, IngestError> {
    let stream = client::download_stream(client, url).await?;
    let mut lines = BufReader::with_capacity(64 * 1024, StreamReader::new(stream)).lines();

    let mut inputs: Vec<TagInput> = Vec::new();
    let mut seen_slugs: HashSet<String> = HashSet::new();
    let mut skipped_parse: u64 = 0;
    while let Some(raw) = lines.next_line().await? {
        match parse_tag_line(&raw, &mut seen_slugs) {
            Ok(Some(input)) => inputs.push(input),
            Ok(None) => {}
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable art-tag line");
                }
            }
        }
    }
    if skipped_parse > 0 {
        tracing::warn!(count = skipped_parse, "skipped unparseable art-tag lines");
    }
    Ok(inputs)
}

/// Parse one bulk-file line into a [`TagInput`]. `Ok(None)` = structurally fine but
/// nothing to keep (array bracket, blank, a foreign tag type, a slugless or duplicate
/// tag); `Err` = a malformed JSON line, counted and skipped by the caller.
fn parse_tag_line(
    raw: &str,
    seen_slugs: &mut HashSet<String>,
) -> Result<Option<TagInput>, serde_json::Error> {
    let line = raw.trim();
    let line = line.strip_suffix(',').unwrap_or(line).trim();
    if line.is_empty() || line == "[" || line == "]" {
        return Ok(None);
    }
    let tag: ScryfallArtTag = serde_json::from_str(line)?;
    // Keep only complete illustration tags (a `slug`, and no foreign tag type — oracle
    // tags ship in their own file). The file's slugs are canonical-unique; a duplicate
    // would break the swap's unique index, so drop repeats defensively.
    let Some(slug) = tag.slug.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if tag.kind.as_deref().is_some_and(|k| k != "illustration") {
        return Ok(None);
    }
    if !seen_slugs.insert(slug.clone()) {
        tracing::warn!(slug = %slug, "skipping duplicate art-tag slug");
        return Ok(None);
    }
    Ok(Some(TagInput {
        scryfall_id: tag.id,
        label: tag.label.unwrap_or_else(|| slug.clone()),
        slug,
        description: tag.description.filter(|d| !d.is_empty()),
        child_ids: tag.child_ids.unwrap_or_default(),
        taggings: tag
            .taggings
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| t.illustration_id)
            .collect(),
    }))
}

/// Swap the whole game's art tags atomically: delete every row of both tables +
/// re-insert the fresh set in one transaction, so a concurrent reader sees either the
/// old tag set or the new one, never a half-rebuilt one (same contract as rulings; there
/// is no natural per-row id to upsert on once the hierarchy is expanded). The network
/// download already finished, so the transaction is DB-only.
async fn replace_art_tags(db: &DatabaseConnection, expanded: Expanded) -> Result<(), IngestError> {
    let now = Utc::now();
    let txn = db.begin().await?;
    CardArtTag::delete_many()
        .filter(card_art_tag::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    ArtTag::delete_many()
        .filter(art_tag::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;

    let Expanded {
        illustrations,
        tags,
        ..
    } = expanded;

    // Tag metadata first (small). Built fully up front, then inserted from an owned
    // iterator — no borrowing iterator/closure is held across an await, which trips
    // the compiler's Send inference inside the spawned sync task.
    let tag_models: Vec<art_tag::ActiveModel> = tags
        .iter()
        .map(|t| art_tag::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            scryfall_id: Set(t.scryfall_id.clone()),
            slug: Set(t.slug.clone()),
            label: Set(t.label.clone()),
            description: Set(t.description.clone()),
            taggings_count: Set(t.illustrations.len() as i32),
            created_at: Set(now),
        })
        .collect();
    let mut tag_iter = tag_models.into_iter();
    loop {
        let chunk: Vec<art_tag::ActiveModel> = tag_iter.by_ref().take(TAG_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        ArtTag::insert_many(chunk)
            .exec_without_returning(&txn)
            .await?;
    }

    // The ~1M mapping rows are materialized one batch at a time, so the peak footprint
    // stays at the interner + one batch of ActiveModels (not a million owned rows).
    let mut batch: Vec<card_art_tag::ActiveModel> = Vec::with_capacity(MAPPING_BATCH);
    for tag in tags {
        for &ill in &tag.illustrations {
            batch.push(card_art_tag::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                tag_slug: Set(tag.slug.clone()),
                illustration_id: Set(illustrations[ill as usize].clone()),
            });
            if batch.len() >= MAPPING_BATCH {
                CardArtTag::insert_many(std::mem::take(&mut batch))
                    .exec_without_returning(&txn)
                    .await?;
            }
        }
    }
    if !batch.is_empty() {
        CardArtTag::insert_many(batch)
            .exec_without_returning(&txn)
            .await?;
    }

    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A verbatim line from the real `art_tags` bulk file (2026-07-23) — pins the wire
    /// contract: comma-terminated one-object-per-line, `type: "illustration"`, taggings
    /// carrying `illustration_id` + fields we ignore (`weight`, `uri`, `aliases`, …).
    const REAL_LINE: &str = r#"{"object":"tag","id":"00b65114-abe0-4b1d-a8b5-f03a43d8f0ad","label":"uthgardt","slug":"uthgardt","type":"illustration","uri":"https://tagger.scryfall.com/tags/artwork/uthgardt","description":null,"parent_ids":["30aa8b34-2a80-4cfb-8dfb-8ea5757c3fa6"],"child_ids":["3ef6e704-f8f7-485b-b3ff-9555103704a1"],"aliases":[],"taggings":[{"illustration_id":"d8f6c122-c474-44b5-8df7-31798c6476ea","weight":"median"}]},"#;

    #[test]
    fn parses_a_real_bulk_line() {
        let mut seen = HashSet::new();
        let input = parse_tag_line(REAL_LINE, &mut seen)
            .expect("valid json")
            .expect("kept");
        assert_eq!(input.scryfall_id, "00b65114-abe0-4b1d-a8b5-f03a43d8f0ad");
        assert_eq!(input.slug, "uthgardt");
        assert_eq!(input.label, "uthgardt");
        assert_eq!(input.description, None);
        assert_eq!(input.child_ids, ["3ef6e704-f8f7-485b-b3ff-9555103704a1"]);
        assert_eq!(input.taggings, ["d8f6c122-c474-44b5-8df7-31798c6476ea"]);

        // The same slug again is a defensive drop, not an error.
        assert!(
            parse_tag_line(REAL_LINE, &mut seen)
                .expect("valid")
                .is_none()
        );
    }

    #[test]
    fn skips_brackets_foreign_types_and_garbage() {
        let mut seen = HashSet::new();
        assert!(parse_tag_line("[", &mut seen).expect("bracket").is_none());
        assert!(parse_tag_line("]", &mut seen).expect("bracket").is_none());
        assert!(parse_tag_line("  ", &mut seen).expect("blank").is_none());
        // An oracle tag sneaking into the file is ignored, not ingested.
        let oracle = r#"{"id":"x","slug":"ramp","label":"Ramp","type":"oracle"},"#;
        assert!(parse_tag_line(oracle, &mut seen).expect("valid").is_none());
        // A slugless tag can't be searched for; dropped.
        let slugless = r#"{"id":"y","label":"Nameless","type":"illustration"}"#;
        assert!(
            parse_tag_line(slugless, &mut seen)
                .expect("valid")
                .is_none()
        );
        // Garbage is an error the caller counts.
        assert!(parse_tag_line("{not json", &mut seen).is_err());
    }
}
