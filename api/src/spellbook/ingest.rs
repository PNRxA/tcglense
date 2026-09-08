//! The combo sync: fetch the dataset (upstream export or mirror snapshot), reduce it to
//! [`ComboRecord`]s, and swap the `combos` + `combo_pieces` tables wholesale.
//!
//! **Version-gated on the document's `ETag`** through `ingest_state` `(mtg, combos)`,
//! but unlike the Scryfall datasets the gate is the *server's*: there is no catalog
//! document advertising a version, so the fetch itself is conditional
//! (`If-None-Match`) and an unchanged day is a bodyless `304` — one request, no parse.
//! The stored tag is only sent back after a **complete** run; an errored run re-fetches.
//! A document served without an `ETag` is versioned by a **content hash** of what it
//! reduced to instead, so the mirror re-serve still has a version to gate on (it can't be
//! sent back as `If-None-Match`, so such a document is fetched every tick and says so).
//! The `running` state keeps the *previous* completed tag: the mirror serves the tables it
//! still holds under that tag until the swap lands, rather than blanking for the import
//! window (or for as long as a run stays errored — `mark_error` preserves it too).
//!
//! **Two documents, one table shape.** In upstream mode the ~28 MB gzipped export is
//! inflated and split element by element ([`super::stream`]); in mirror mode the origin's
//! compact JSONL re-serve is read line by line through the same gzip-sniffing seam every
//! bulk consumer uses. Both produce the same records and land in [`replace_combos`].
//! Records are collected before the (short, DB-only) transaction, like the rulings —
//! ~110k compact records is tens of MB, well under the inflated document.
//!
//! **A run that imports zero combos is a failure**, recorded as `error` so it retries,
//! never version-locked as an empty `complete` — an empty table would make every deck
//! confidently report "no combos".

use std::collections::HashMap;

use chrono::Utc;
use futures_util::TryStreamExt;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, ETAG, IF_NONE_MATCH},
};
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, TransactionTrait,
};
use sha2::{Digest, Sha256};

use super::model::{ComboRecord, Variant};
use super::stream::VariantSplitter;
use super::{DATASET, GAME};
use crate::catalog::ingest_state::{self, StateFields};
use crate::datasets::SyncSource;
use crate::entities::prelude::{Combo, ComboPiece};
use crate::entities::{combo, combo_piece};
use crate::scryfall::client;
use crate::scryfall::ingest::IngestError;

/// Combos per insert batch: 16 columns × 400 = 6.4k bound parameters, under SQLite's
/// 32 766 limit with room for the pieces batch below.
const COMBO_BATCH: usize = 400;
/// Pieces per insert batch: 9 columns × 1000 = 9k parameters.
const PIECE_BATCH: usize = 1000;

/// Refresh the combo database, recording status in `ingest_state`.
///
/// Skips silently (no bookkeeping write) when the dataset is switched off. Every other
/// failure marks the row `error` and is returned, so the sync loop's provider flag sees it.
pub async fn refresh(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
) -> Result<(), IngestError> {
    let Some(url) = source.spellbook_combos_url() else {
        tracing::debug!("combo sync disabled (COMBOS_SYNC_ENABLED=false); skipping");
        return Ok(());
    };
    match refresh_inner(db, client, source, &url).await {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::error!(error = %err, "combo import failed");
            let _ = ingest_state::mark_error(db, GAME, DATASET, &err.public_detail()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    client: &Client,
    source: &SyncSource,
    url: &str,
) -> Result<(), IngestError> {
    // The last tag the tables were filled under (kept through `running` and `error`
    // states, see the module doc), and whether it was a *completed* import's — only then
    // is it sent back, so an errored or zero-row run re-fetches the document rather than
    // 304-ing onto a table it never filled.
    let state = ingest_state::load(db, GAME, DATASET).await?;
    let held_tag = state.as_ref().and_then(|s| s.source_updated_at.clone());
    let prev_etag = state
        .as_ref()
        .filter(|s| s.status == "complete")
        .and_then(|s| s.source_updated_at.as_deref())
        // A content-hash version is ours, not the server's: never sent back.
        .filter(|tag| !tag.starts_with(CONTENT_VERSION_PREFIX));

    let mut request = client.get(url).header(ACCEPT, "application/json");
    if let Some(tag) = prev_etag {
        request = request.header(IF_NONE_MATCH, tag);
    }
    let response = request.send().await?;
    if response.status() == StatusCode::NOT_MODIFIED {
        tracing::info!("combo database unchanged (304); already up to date");
        return Ok(());
    }
    // The mirror answers 404 until its origin has completed an import (an origin on a
    // pre-#683 build, or one that opted out): name that plainly, since it is a steady state
    // the consumer can't do anything about, not a transport blip.
    if response.status() == StatusCode::NOT_FOUND && !source.from_upstream() {
        return Err(IngestError::Other(
            "the mirror does not offer a combo snapshot yet (its origin has not completed an import); will retry next tick"
                .to_string(),
        ));
    }
    let response = response.error_for_status()?;
    // The version this document is: its `ETag` (both the upstream CDN and the mirror send
    // a strong one). Absent, the reduced records are hashed below instead.
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if etag.is_none() {
        tracing::warn!("combo document carries no ETag; it will be re-fetched every tick");
    }

    let started = Utc::now();
    tracing::info!(url, etag = ?etag, "importing combo database");
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET,
            status: "running",
            source_updated_at: held_tag.as_deref(),
            detail: "importing combos",
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: None,
        },
    )
    .await?;

    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let records = if source.from_upstream() {
        collect_upstream(stream).await?
    } else {
        collect_mirror(stream).await?
    };
    let count = records.len();
    if count == 0 {
        return Err(IngestError::Other(
            "combo import produced 0 combos; treating as failure to retry".to_string(),
        ));
    }
    let version = etag.unwrap_or_else(|| content_version(&records));

    let pieces = replace_combos(db, records).await?;

    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET,
            status: "complete",
            source_updated_at: Some(&version),
            detail: &format!("imported {count} combos ({pieces} pieces)"),
            sets_imported: 0,
            cards_imported: i32::try_from(count).unwrap_or(i32::MAX),
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(combos = count, pieces, "combo database import complete");
    Ok(())
}

/// Marks a version the ingest minted itself (no `ETag` on the document) — never sent back
/// as `If-None-Match`, but a stable tag for the mirror re-serve to gate on.
const CONTENT_VERSION_PREFIX: &str = "content-";

/// A version for a document that carried no `ETag`: a hash over what it reduced to — every
/// combo's id and popularity and its pieces' identities, in order — so two fetches of the
/// same data agree and any change to a combo moves it.
fn content_version(records: &[ComboRecord]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        hasher.update(record.id.as_bytes());
        hasher.update(record.popularity.to_le_bytes());
        for piece in &record.pieces {
            hasher.update(piece.oracle_id.as_bytes());
            hasher.update([u8::from(piece.must_be_commander)]);
        }
        hasher.update(b"\n");
    }
    format!(
        "{CONTENT_VERSION_PREFIX}{}",
        hex::encode(&hasher.finalize()[..16])
    )
}

/// Reduce the upstream export — one gzipped JSON document — element by element.
async fn collect_upstream<S>(stream: S) -> Result<Vec<ComboRecord>, IngestError>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin,
{
    let reader = client::inflated(stream).await?;
    let mut splitter = VariantSplitter::new(reader);
    let mut records = Vec::new();
    let mut skipped_parse: u64 = 0;
    let mut skipped_shape: u64 = 0;
    while let Some(bytes) = splitter.next_object().await? {
        let variant: Variant = match serde_json::from_slice(&bytes) {
            Ok(variant) => variant,
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable combo variant");
                }
                continue;
            }
        };
        match ComboRecord::from_variant(variant) {
            Some(record) => records.push(record),
            None => skipped_shape += 1,
        }
    }
    if skipped_parse > 0 || skipped_shape > 0 {
        tracing::warn!(
            unparseable = skipped_parse,
            unmatchable = skipped_shape,
            "skipped combo variants"
        );
    }
    Ok(records)
}

/// Read the mirror's compact snapshot: one [`ComboRecord`] per line, possibly gzipped.
async fn collect_mirror<S>(stream: S) -> Result<Vec<ComboRecord>, IngestError>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin,
{
    let mut lines = client::json_lines(stream).await?;
    let mut records = Vec::new();
    let mut skipped_parse: u64 = 0;
    while let Some(raw) = lines.next_line().await? {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<ComboRecord>(line) {
            Ok(record) if !record.pieces.is_empty() => records.push(record),
            Ok(_) => {}
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable combo snapshot line");
                }
            }
        }
    }
    if skipped_parse > 0 {
        tracing::warn!(
            count = skipped_parse,
            "skipped unparseable combo snapshot lines"
        );
    }
    Ok(records)
}

/// Swap the game's whole combo database atomically: delete every row + re-insert the
/// fresh set in one transaction, so a reader sees the old table or the new one, never a
/// half-rebuilt one. Returns how many pieces were written. Shared with the dummy seeder,
/// so the offline catalog's combos are shaped exactly like imported ones.
pub async fn replace_combos(
    db: &DatabaseConnection,
    records: Vec<ComboRecord>,
) -> Result<usize, IngestError> {
    let txn = db.begin().await?;
    // Children first: SQLite doesn't enforce the cascade unless foreign keys are on.
    ComboPiece::delete_many()
        .filter(combo_piece::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    Combo::delete_many()
        .filter(combo::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;

    let mut pieces_written = 0usize;
    for batch in records.chunks(COMBO_BATCH) {
        let rows: Vec<combo::ActiveModel> = batch
            .iter()
            .map(|r| combo::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                external_id: Set(r.id.clone()),
                color_identity: Set(r.identity.join(",")),
                mana_needed: Set(r.mana_needed.clone()),
                mana_value_needed: Set(r.mana_value_needed),
                prerequisites: Set(r.prerequisites.clone()),
                description: Set(r.description.clone()),
                notes: Set(r.notes.clone()),
                popularity: Set(r.popularity),
                bracket_tag: Set(r.bracket_tag.clone()),
                piece_count: Set(i32::try_from(r.pieces.len()).unwrap_or(i32::MAX)),
                template_count: Set(i32::try_from(r.templates.len()).unwrap_or(i32::MAX)),
                templates: Set(serde_json::to_string(&r.templates).unwrap_or_else(|_| "[]".into())),
                produces: Set(serde_json::to_string(&r.produces).unwrap_or_else(|_| "[]".into())),
                commander_legal: Set(r.commander_legal),
            })
            .collect();
        Combo::insert_many(rows)
            .exec_without_returning(&txn)
            .await?;

        // The ids the insert minted, by external id — one select per batch rather than a
        // `RETURNING` the two backends spell differently.
        let ids: HashMap<String, i32> = Combo::find()
            .select_only()
            .column(combo::Column::Id)
            .column(combo::Column::ExternalId)
            .filter(combo::Column::Game.eq(GAME))
            .filter(combo::Column::ExternalId.is_in(batch.iter().map(|r| r.id.as_str())))
            .into_tuple::<(i32, String)>()
            .all(&txn)
            .await?
            .into_iter()
            .map(|(id, external)| (external, id))
            .collect();

        let mut pieces: Vec<combo_piece::ActiveModel> = Vec::new();
        for record in batch {
            let Some(&combo_id) = ids.get(&record.id) else {
                return Err(IngestError::Other(format!(
                    "combo {} vanished between insert and lookup",
                    record.id
                )));
            };
            for (position, piece) in record.pieces.iter().enumerate() {
                pieces.push(combo_piece::ActiveModel {
                    id: NotSet,
                    game: Set(GAME.to_string()),
                    combo_id: Set(combo_id),
                    oracle_id: Set(piece.oracle_id.clone()),
                    name: Set(piece.name.clone()),
                    quantity: Set(piece.quantity),
                    must_be_commander: Set(piece.must_be_commander),
                    zone_locations: Set(piece.zones.join(",")),
                    position: Set(i32::try_from(position).unwrap_or(i32::MAX)),
                });
            }
        }
        pieces_written += pieces.len();
        let mut iter = pieces.into_iter();
        loop {
            let chunk: Vec<combo_piece::ActiveModel> = iter.by_ref().take(PIECE_BATCH).collect();
            if chunk.is_empty() {
                break;
            }
            ComboPiece::insert_many(chunk)
                .exec_without_returning(&txn)
                .await?;
        }
    }
    txn.commit().await?;
    Ok(pieces_written)
}

/// Load every stored combo of `game` with its pieces, ordered by combo id — the mirror
/// re-serve and the dummy-seed round-trip tests read through this.
#[cfg(test)]
pub(crate) async fn load_all<C: sea_orm::ConnectionTrait>(
    db: &C,
) -> Result<Vec<ComboRecord>, sea_orm::DbErr> {
    use sea_orm::QueryOrder;
    let combos = Combo::find()
        .filter(combo::Column::Game.eq(GAME))
        .order_by_asc(combo::Column::Id)
        .all(db)
        .await?;
    let pieces = ComboPiece::find()
        .filter(combo_piece::Column::Game.eq(GAME))
        .order_by_asc(combo_piece::Column::ComboId)
        .order_by_asc(combo_piece::Column::Position)
        .all(db)
        .await?;
    let mut by_combo: HashMap<i32, Vec<combo_piece::Model>> = HashMap::new();
    for piece in pieces {
        by_combo.entry(piece.combo_id).or_default().push(piece);
    }
    Ok(combos
        .iter()
        .map(|c| ComboRecord::from_models(c, by_combo.get(&c.id).map_or(&[][..], Vec::as_slice)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spellbook::model::{PieceRecord, ProducedFeature};
    use crate::test_support::migrated_memory_db;
    use sea_orm::PaginatorTrait;

    fn record(id: &str, pieces: &[(&str, &str)], templates: &[&str]) -> ComboRecord {
        ComboRecord {
            id: id.to_string(),
            identity: vec!["U".into(), "B".into()],
            mana_needed: "{2}".into(),
            mana_value_needed: Some(2),
            prerequisites: "".into(),
            description: format!("How {id} works."),
            notes: "".into(),
            popularity: 7,
            bracket_tag: "R".into(),
            templates: templates.iter().map(|t| (*t).to_string()).collect(),
            produces: vec![ProducedFeature {
                name: "Infinite mana".into(),
                status: "S".into(),
            }],
            commander_legal: true,
            pieces: pieces
                .iter()
                .enumerate()
                .map(|(i, (oracle, name))| PieceRecord {
                    oracle_id: (*oracle).to_string(),
                    name: (*name).to_string(),
                    quantity: 1,
                    must_be_commander: i == 0,
                    zones: vec!["B".into()],
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn replace_writes_rows_that_round_trip_with_the_parent_counts() {
        let db = migrated_memory_db().await;
        let records = vec![
            record("1-2", &[("o1", "One"), ("o2", "Two")], &[]),
            record(
                "1-2-3",
                &[("o1", "One"), ("o2", "Two"), ("o3", "Three")],
                &["Any outlet"],
            ),
        ];
        let pieces = replace_combos(&db, records.clone()).await.expect("replace");
        assert_eq!(pieces, 5);
        assert_eq!(load_all(&db).await.expect("load"), records);

        // The parent carries the counts the deck read judges a combo by.
        let three = ComboPiece::find()
            .find_also_related(Combo)
            .filter(combo_piece::Column::OracleId.eq("o3"))
            .one(&db)
            .await
            .expect("query")
            .expect("row");
        let parent = three.1.expect("parent");
        assert_eq!(parent.piece_count, 3);
        assert_eq!(parent.template_count, 1);
        assert_eq!(parent.popularity, 7);
        assert_eq!(parent.templates, "[\"Any outlet\"]");
        assert_eq!(three.0.position, 2);
        assert!(!three.0.must_be_commander);
    }

    #[tokio::test]
    async fn replace_swaps_the_whole_table_not_a_merge() {
        let db = migrated_memory_db().await;
        replace_combos(&db, vec![record("old", &[("o1", "One")], &[])])
            .await
            .expect("first");
        replace_combos(&db, vec![record("new", &[("o9", "Nine")], &[])])
            .await
            .expect("second");
        let ids: Vec<String> = load_all(&db)
            .await
            .expect("load")
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["new"]);
        let orphaned = ComboPiece::find()
            .filter(combo_piece::Column::OracleId.eq("o1"))
            .count(&db)
            .await
            .expect("count");
        assert_eq!(orphaned, 0, "the old combo's pieces went with it");
    }

    #[test]
    fn a_content_version_is_stable_and_moves_with_the_data() {
        let a = vec![
            record("1", &[("o1", "One")], &[]),
            record("2", &[("o2", "Two")], &[]),
        ];
        let same = a.clone();
        assert_eq!(content_version(&a), content_version(&same));
        assert!(content_version(&a).starts_with(CONTENT_VERSION_PREFIX));
        let mut moved = a.clone();
        moved[1].popularity += 1;
        assert_ne!(content_version(&a), content_version(&moved));
        let mut repieced = a;
        repieced[0].pieces[0].oracle_id = "o9".into();
        assert_ne!(content_version(&same), content_version(&repieced));
        let mut recommandered = same.clone();
        recommandered[0].pieces[0].must_be_commander =
            !recommandered[0].pieces[0].must_be_commander;
        assert_ne!(content_version(&same), content_version(&recommandered));
    }

    #[tokio::test]
    async fn the_mirror_snapshot_reader_skips_blank_and_bad_lines() {
        let good = serde_json::to_string(&record("a", &[("o1", "One")], &[])).expect("json");
        let body = format!("{good}\n\nnot json\n{{\"id\":\"empty\",\"pieces\":[]}}\n");
        let stream = futures_util::stream::iter(vec![Ok(bytes::Bytes::from(body))]);
        let records = collect_mirror(stream).await.expect("collect");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "a");
    }

    /// Live contract canary — **not run by CI** (network), like `scryfall::client`'s:
    /// `cargo test -- --ignored live_spellbook_export`. Streams the real ~28 MB export through
    /// the splitter + reducer end to end, so a field rename upstream (or a document reshaped
    /// away from `{"variants": [...]}`) fails here with an obvious message instead of as an
    /// every-tick "0 combos" error on the mirror origin.
    #[tokio::test]
    #[ignore = "hits json.commanderspellbook.com; run manually"]
    async fn live_spellbook_export_parses_through_the_splitter() {
        let client = Client::builder()
            .user_agent("TCGLense/contract-canary")
            .build()
            .expect("client");
        let response = client
            .get(crate::spellbook::VARIANTS_URL)
            .send()
            .await
            .expect("download opens")
            .error_for_status()
            .expect("2xx");
        assert!(
            response.headers().get(ETAG).is_some(),
            "the export carries an ETag"
        );
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        let records = collect_upstream(stream).await.expect("the document parses");
        assert!(
            records.len() > 50_000,
            "the live export holds ~110k combos; got {}",
            records.len()
        );
        // Every record is matchable by construction, and a known combo is present.
        assert!(records.iter().all(|r| !r.pieces.is_empty()));
        assert!(
            records
                .iter()
                .any(|r| r.pieces.len() == 2 && r.commander_legal)
        );
    }

    #[tokio::test]
    async fn the_upstream_reader_reduces_the_document_through_the_splitter() {
        let doc = r#"{"version": "6.4.0", "variants": [
            {"id": "1", "uses": [{"card": {"name": "A", "oracleId": "oa"}}], "identity": "W"},
            {"id": "2", "uses": []},
            {"id": "3", "uses": [{"card": {"name": "B", "oracleId": "ob"}, "quantity": 2}]}
        ]}"#;
        let stream = futures_util::stream::iter(vec![Ok(bytes::Bytes::from(doc))]);
        let records = collect_upstream(stream).await.expect("collect");
        let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "3"], "the piece-less variant is dropped");
        assert_eq!(records[1].pieces[0].quantity, 2);
    }
}
