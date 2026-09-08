//! The mirror origin's **compact re-serve** of the combo database: every stored combo as
//! one gzipped JSONL line of [`ComboRecord`], streamed out of the tables (see
//! [`crate::handlers::mirror::spellbook_combos`] for the route and its `ETag`).
//!
//! Streamed the way the card export is ([`crate::handlers::shared::card_export`]), and for
//! the same two reasons: the body is never assembled in memory (~110k combos is tens of
//! MB plain), and **no database connection is held while awaiting the client** — the ids
//! are resolved in one query, then each chunk re-acquires a connection, renders, and
//! releases it before the (client-paced) send. Each chunk is its own gzip **member**;
//! concatenated members are one valid gzip stream, and the consumer's inflate seam reads
//! them as such (`multiple_members`), so the whole body is compressed without buffering it.
//!
//! A mid-stream failure ends the transfer with an error rather than a short-but-valid
//! body — a consumer that read a truncated snapshot would import half a database and
//! version-lock it, and the ingest's zero-row guard can't see "half". That includes the
//! drain's own race: a rebuild committing under it re-mints every id, so a chunk that
//! resolves fewer rows than it asked for is that rebuild, and the transfer is errored
//! rather than padded with the empty chunks `load_combos` would otherwise hand back.

use std::io;

use axum::body::{Body, Bytes};
use futures_util::stream;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use super::GAME;
use super::model::ComboRecord;
use crate::entities::combo;
use crate::entities::prelude::Combo;
use crate::handlers::shared::combos::load_combos;

/// Combos per rendered chunk (~150 KB plain, a gzip member each).
const SNAPSHOT_CHUNK: usize = 500;
/// Rendered chunks in flight before the drain waits on the client.
const SNAPSHOT_CHANNEL_CHUNKS: usize = 4;

/// A streaming body of the whole combo database, gzipped JSONL.
pub fn stream_snapshot(db: DatabaseConnection) -> Body {
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(SNAPSHOT_CHANNEL_CHUNKS);
    tokio::spawn(async move { drain(db, tx).await });
    Body::from_stream(stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    }))
}

async fn drain(db: DatabaseConnection, tx: mpsc::Sender<Result<Bytes, io::Error>>) {
    // Phase 1: which combos, in a stable order. Ids only.
    let ids: Vec<i32> = match Combo::find()
        .select_only()
        .column(combo::Column::Id)
        .filter(combo::Column::Game.eq(GAME))
        .order_by_asc(combo::Column::Id)
        .into_tuple()
        .all(&db)
        .await
    {
        Ok(ids) => ids,
        Err(error) => {
            tracing::error!(%error, "combo snapshot query failed to start");
            let _ = tx
                .send(Err(io::Error::other("combo snapshot query failed")))
                .await;
            return;
        }
    };

    // Phase 2: a chunk at a time, connection released before each send.
    for chunk in ids.chunks(SNAPSHOT_CHUNK) {
        let rows = match load_combos(&db, GAME, chunk).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "combo snapshot failed part-way");
                let _ = tx
                    .send(Err(io::Error::other("combo snapshot failed part-way")))
                    .await;
                return;
            }
        };
        // `load_combos` only ever drops ids that no longer resolve, and ids are never
        // reused, so a short chunk means exactly one thing: the tables were rebuilt while
        // this drain was in flight. End the transfer with an error — a consumer must never
        // import the first half of one version as the whole of it.
        if rows.len() != chunk.len() {
            tracing::warn!(
                expected = chunk.len(),
                got = rows.len(),
                "combo snapshot rows vanished mid-drain (table rebuilt); ending the transfer"
            );
            let _ = tx
                .send(Err(io::Error::other(
                    "combo snapshot rebuilt mid-stream; retry",
                )))
                .await;
            return;
        }
        let mut plain: Vec<u8> = Vec::new();
        for (model, pieces) in &rows {
            let record = ComboRecord::from_models(model, pieces);
            match serde_json::to_writer(&mut plain, &record) {
                Ok(()) => plain.push(b'\n'),
                Err(error) => {
                    tracing::error!(%error, combo = %model.external_id, "combo snapshot serialise failed");
                    let _ = tx
                        .send(Err(io::Error::other("combo snapshot serialise failed")))
                        .await;
                    return;
                }
            }
        }
        let member = match gzip_member(&plain).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::error!(%error, "combo snapshot compression failed");
                let _ = tx.send(Err(error)).await;
                return;
            }
        };
        if tx.send(Ok(Bytes::from(member))).await.is_err() {
            return; // the client hung up — a normal end
        }
    }
}

/// Compress `plain` as one complete gzip member.
async fn gzip_member(plain: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = async_compression::tokio::write::GzipEncoder::new(Vec::new());
    encoder.write_all(plain).await?;
    encoder.shutdown().await?;
    Ok(encoder.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spellbook::ingest::replace_combos;
    use crate::spellbook::model::PieceRecord;
    use crate::test_support::migrated_memory_db;
    use axum::body::to_bytes;

    fn record(id: &str) -> ComboRecord {
        ComboRecord {
            id: id.to_string(),
            identity: vec!["R".into()],
            mana_needed: String::new(),
            mana_value_needed: None,
            prerequisites: String::new(),
            description: "Go.".into(),
            notes: String::new(),
            popularity: 1,
            bracket_tag: String::new(),
            templates: Vec::new(),
            produces: Vec::new(),
            commander_legal: true,
            pieces: vec![PieceRecord {
                oracle_id: format!("o-{id}"),
                name: format!("Card {id}"),
                quantity: 1,
                must_be_commander: false,
                zones: vec!["B".into()],
            }],
        }
    }

    /// The snapshot round-trips through the very reader a consumer imports it with: gzip
    /// members concatenated, one record per line, in id order.
    #[tokio::test]
    async fn the_snapshot_reads_back_through_the_consumers_seam() {
        let db = migrated_memory_db().await;
        let records: Vec<ComboRecord> = (0..1203).map(|i| record(&format!("c{i:04}"))).collect();
        replace_combos(&db, records.clone()).await.expect("seed");

        let body = stream_snapshot(db);
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        assert_eq!(bytes[0], 0x1f, "gzipped");

        let stream = futures_util::stream::iter(vec![Ok::<_, io::Error>(bytes)]);
        let mut lines = crate::scryfall::client::json_lines(stream)
            .await
            .expect("reader");
        let mut back: Vec<ComboRecord> = Vec::new();
        while let Some(line) = lines.next_line().await.expect("line") {
            back.push(serde_json::from_str(&line).expect("record"));
        }
        assert_eq!(back, records, "three members, every record, in order");
    }
}
