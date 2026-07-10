//! Distributing the visual-scanner fingerprint index between TCGLense instances.
//!
//! The fingerprint index is built exactly once — by the operator's index-building
//! instance (`FINGERPRINT_BUILD_ENABLED`; a hash-and-discard walk of the catalogue, see
//! [`crate::catalog::fingerprints`] + [`crate::tasks`]). Every ordinary self-host instead
//! **imports** the finished index over the dataset mirror and fetches zero card images:
//!
//! * The origin serves its in-memory index at `GET /api/mirror/fingerprints/{game}`
//!   (see [`crate::handlers::mirror`]), gated on `MIRROR_ENABLED` like the other mirror
//!   endpoints, as a compact binary payload with a content [`etag`].
//! * A consumer pulls that payload ([`import_from_mirror`]), version-gates it against its
//!   own `FINGERPRINT_ALGO_VERSION` (the pHash the browser computes must match the hashes
//!   in the index), and replaces its local `card_fingerprint` rows in one transaction
//!   ([`crate::catalog::fingerprints::replace_all`]). The stored `ETag` makes an
//!   unchanged index a cheap conditional `304`.
//!
//! **Wire format** (little-endian, self-describing, versioned by its magic):
//!
//! ```text
//! magic         8 bytes   b"TCGLFP01"
//! algo_version  i32       the FINGERPRINT_ALGO_VERSION the index was built at
//! count         u32       number of records that follow
//! record × count:
//!   id_len      u16       external-id length in bytes
//!   external_id id_len    UTF-8 (a Scryfall UUID)
//!   face_index  i32       0 for single-faced / the front face
//!   hash        32 bytes  the 256-bit pHash (PHASH_BYTES)
//! ```
//!
//! Roughly `(44 + id_len) × count` bytes — about 3–4 MB for the ~106k-card MTG catalogue,
//! which is why the index ships whole rather than being diffed.

use chrono::Utc;
use reqwest::{
    StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter,
};
use sha2::{Digest, Sha256};

use crate::catalog::fingerprints::{self, ExportEntry, ImportRow};
use crate::entities::ingest_state;
use crate::entities::prelude::IngestState;
use crate::phash::PHASH_BYTES;

/// Path prefix the export endpoint is served under + the import path targets on the
/// mirror. Must match the literal route registered in [`crate::router`].
pub const MIRROR_PREFIX: &str = "/api/mirror/fingerprints";

/// Magic header identifying the payload + its format version (bump the trailing digits
/// on an incompatible layout change). A consumer that reads a different magic refuses the
/// payload rather than mis-parsing it.
const MAGIC: &[u8; 8] = b"TCGLFP01";

/// `source_size` recorded on imported rows. The origin fingerprints the `small` image, so
/// this labels the imported rows the same — purely descriptive (an importer never rebuilds).
const IMPORTED_SOURCE_SIZE: &str = "small";

/// `ingest_state.dataset` key the import bookkeeping is stored under (per game), reusing
/// the shared ingest-state table so the last-imported `ETag` survives restarts and shows
/// up alongside the other datasets' status.
const IMPORT_DATASET: &str = "card_fingerprints";

/// A malformed fingerprint payload (wrong magic, truncated, or a non-UTF-8 id). The
/// consumer only fetches from the mirror it was configured to trust, so this is
/// corruption / a version skew, not adversarial input — but parsing is still total.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("bad magic header (not a TCGLense fingerprint payload)")]
    BadMagic,
    #[error("payload is truncated")]
    Truncated,
    #[error("external id is not valid UTF-8")]
    BadExternalId,
}

/// A failure importing the index from the mirror. Non-fatal at the call site (logged;
/// the scanner just keeps serving whatever index it already had).
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("fingerprint mirror request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("fingerprint payload is malformed: {0}")]
    Parse(#[from] ParseError),
    #[error("storing imported fingerprints failed: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// The result of one import attempt.
pub enum ImportOutcome {
    /// The origin's index is unchanged since our last import (a `304`, or the served
    /// `ETag` still matches ours) — nothing was written.
    Unchanged,
    /// The origin serves an index built at a different `algo_version` than this instance
    /// expects. Skipped: the browser here would compute hashes the imported index can't
    /// match. The operator must align `FINGERPRINT_ALGO_VERSION` (and the web bundle).
    AlgoMismatch { served: i32, expected: i32 },
    /// `count` fingerprints imported; carry the new `ETag` to store for next time.
    Imported { count: usize, etag: Option<String> },
}

/// The parsed contents of a fingerprint payload.
pub struct Parsed {
    pub algo_version: i32,
    pub rows: Vec<ImportRow>,
}

/// Serialize `entries` (already game-scoped + stably ordered by
/// [`fingerprints::FingerprintIndex::export_entries`]) into the wire format above.
pub fn serialize(algo_version: i32, entries: &[ExportEntry<'_>]) -> Vec<u8> {
    // Header + a per-record estimate (2 len + ~36 id + 4 face + 32 hash) to avoid regrowth.
    let mut buf = Vec::with_capacity(16 + entries.len() * (2 + 36 + 4 + PHASH_BYTES));
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&algo_version.to_le_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        let id = entry.external_id.as_bytes();
        // Card provider ids are short UUIDs, never near u16::MAX; truncation can't occur.
        buf.extend_from_slice(&(id.len() as u16).to_le_bytes());
        buf.extend_from_slice(id);
        buf.extend_from_slice(&entry.face_index.to_le_bytes());
        buf.extend_from_slice(entry.hash);
    }
    buf
}

/// A forward-only reader over the payload that bounds-checks every read, so any
/// truncation surfaces as [`ParseError::Truncated`] rather than a panic.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(ParseError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u16_le(&mut self) -> Result<u16, ParseError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32_le(&mut self) -> Result<i32, ParseError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u32_le(&mut self) -> Result<u32, ParseError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Parse a payload produced by [`serialize`]. Total: a wrong magic, a short buffer, a
/// count that overruns the data, or a non-UTF-8 id all return an `Err`, never panic.
pub fn parse(bytes: &[u8]) -> Result<Parsed, ParseError> {
    let mut reader = Reader {
        data: bytes,
        pos: 0,
    };
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(ParseError::BadMagic);
    }
    let algo_version = reader.i32_le()?;
    let count = reader.u32_le()? as usize;
    // Cap only the *pre-allocation* against a bogus huge count; the loop itself is bounded
    // by `count` and each read is bounds-checked, so a lie about the count fails fast at
    // `Truncated` without ever allocating for it.
    let mut rows = Vec::with_capacity(count.min(200_000));
    for _ in 0..count {
        let id_len = reader.u16_le()? as usize;
        let id = reader.take(id_len)?;
        let external_id = std::str::from_utf8(id)
            .map_err(|_| ParseError::BadExternalId)?
            .to_string();
        let face_index = reader.i32_le()?;
        let hash: [u8; PHASH_BYTES] = reader.take(PHASH_BYTES)?.try_into().unwrap();
        rows.push(ImportRow {
            external_id,
            face_index,
            hash,
        });
    }
    Ok(Parsed { algo_version, rows })
}

/// A strong content `ETag` for a serialized payload: the first 16 bytes of its SHA-256,
/// hex, quoted. Two identical indexes serialize to identical bytes (export order is
/// stable), so the tag only changes when the index's contents actually change.
pub fn etag(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    format!("\"fp-{}\"", hex::encode(&digest[..16]))
}

/// Pull the current fingerprint index for `game` from the mirror at `mirror_base` and,
/// when it differs from `prev_etag`, replace the local rows with it. Conditional on
/// `prev_etag` (an `If-None-Match`), so an unchanged index costs a single cheap `304`.
/// Version-gates the payload against `expected_algo_version` before touching the DB.
pub async fn import_from_mirror(
    db: &DatabaseConnection,
    http: &reqwest::Client,
    mirror_base: &str,
    game: &str,
    expected_algo_version: i32,
    prev_etag: Option<&str>,
) -> Result<ImportOutcome, ImportError> {
    let url = format!(
        "{}{MIRROR_PREFIX}/{game}",
        mirror_base.trim_end_matches('/')
    );
    let mut request = http.get(&url);
    if let Some(tag) = prev_etag {
        request = request.header(IF_NONE_MATCH, tag);
    }
    let response = request.send().await?;
    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(ImportOutcome::Unchanged);
    }
    let response = response.error_for_status()?;
    let served_etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    // Belt-and-braces: some caches answer a conditional GET with a full 200 that still
    // carries the same ETag — treat that as unchanged too, so we never re-import needlessly.
    if let (Some(prev), Some(served)) = (prev_etag, served_etag.as_deref()) {
        if prev == served {
            return Ok(ImportOutcome::Unchanged);
        }
    }
    let bytes = response.bytes().await?;
    let parsed = parse(&bytes)?;
    if parsed.algo_version != expected_algo_version {
        return Ok(ImportOutcome::AlgoMismatch {
            served: parsed.algo_version,
            expected: expected_algo_version,
        });
    }
    let count = fingerprints::replace_all(
        db,
        game,
        expected_algo_version,
        IMPORTED_SOURCE_SIZE,
        parsed.rows,
    )
    .await?;
    Ok(ImportOutcome::Imported {
        count,
        etag: served_etag,
    })
}

/// The `ETag` of the fingerprint index we last successfully imported for `game`, if any.
/// Stored in `ingest_state` so a restart still short-circuits an unchanged index to a
/// `304` rather than re-downloading + re-replacing the whole table.
pub async fn last_import_etag(
    db: &DatabaseConnection,
    game: &str,
) -> Result<Option<String>, DbErr> {
    Ok(IngestState::find()
        .filter(ingest_state::Column::Game.eq(game))
        .filter(ingest_state::Column::Dataset.eq(IMPORT_DATASET))
        .one(db)
        .await?
        .and_then(|row| row.source_updated_at))
}

/// Record a completed fingerprint import in `ingest_state` (upsert on `(game, dataset)`):
/// the served `ETag` gates the next conditional fetch, and `status`/`detail`/`count`
/// surface progress the same way the provider imports do. Only called after a successful
/// import — a skip/mismatch/error leaves the prior row (and its ETag) untouched.
pub async fn record_import(
    db: &DatabaseConnection,
    game: &str,
    etag: Option<&str>,
    status: &str,
    detail: &str,
    imported: i32,
) -> Result<(), DbErr> {
    let now = Utc::now();
    let existing = IngestState::find()
        .filter(ingest_state::Column::Game.eq(game))
        .filter(ingest_state::Column::Dataset.eq(IMPORT_DATASET))
        .one(db)
        .await?;
    match existing {
        Some(model) => {
            let mut active: ingest_state::ActiveModel = model.into();
            // Keep the previous ETag if this import didn't carry one (e.g. a 200 without
            // the header) so a later conditional request still has something to send.
            if let Some(tag) = etag {
                active.source_updated_at = Set(Some(tag.to_string()));
            }
            active.status = Set(status.to_string());
            active.detail = Set(Some(detail.to_string()));
            active.cards_imported = Set(imported);
            active.finished_at = Set(Some(now));
            active.update(db).await?;
        }
        None => {
            ingest_state::ActiveModel {
                game: Set(game.to_string()),
                dataset: Set(IMPORT_DATASET.to_string()),
                source_updated_at: Set(etag.map(str::to_string)),
                status: Set(status.to_string()),
                detail: Set(Some(detail.to_string())),
                sets_imported: Set(0),
                cards_imported: Set(imported),
                started_at: Set(Some(now)),
                finished_at: Set(Some(now)),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(algo: i32) -> Vec<u8> {
        let h1 = [1u8; PHASH_BYTES];
        let h2 = [2u8; PHASH_BYTES];
        let entries = vec![
            ExportEntry {
                external_id: "aaaa-1111",
                face_index: 0,
                hash: &h1,
            },
            ExportEntry {
                external_id: "bbbb-2222",
                face_index: 1,
                hash: &h2,
            },
        ];
        serialize(algo, &entries)
    }

    #[test]
    fn round_trips_serialize_then_parse() {
        let parsed = parse(&sample(7)).expect("parse");
        assert_eq!(parsed.algo_version, 7);
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(parsed.rows[0].external_id, "aaaa-1111");
        assert_eq!(parsed.rows[0].face_index, 0);
        assert_eq!(parsed.rows[0].hash, [1u8; PHASH_BYTES]);
        assert_eq!(parsed.rows[1].external_id, "bbbb-2222");
        assert_eq!(parsed.rows[1].face_index, 1);
        assert_eq!(parsed.rows[1].hash, [2u8; PHASH_BYTES]);
    }

    #[test]
    fn serializes_an_empty_index() {
        let payload = serialize(1, &[]);
        let parsed = parse(&payload).expect("parse empty");
        assert_eq!(parsed.algo_version, 1);
        assert!(parsed.rows.is_empty());
    }

    #[test]
    fn parse_rejects_a_bad_magic() {
        let mut bytes = sample(1);
        bytes[0] = b'X';
        assert!(matches!(parse(&bytes), Err(ParseError::BadMagic)));
    }

    #[test]
    fn parse_rejects_a_truncated_payload() {
        let bytes = sample(1);
        // Chop the final hash short: the header still promises 2 records.
        assert!(matches!(
            parse(&bytes[..bytes.len() - 4]),
            Err(ParseError::Truncated)
        ));
        // Even the fixed header being incomplete is Truncated, not a panic.
        assert!(matches!(parse(&[0u8; 3]), Err(ParseError::Truncated)));
    }

    #[test]
    fn parse_rejects_a_count_that_overruns_the_buffer() {
        // A header claiming a record that isn't there fails fast, allocating nothing huge.
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&5_000_000u32.to_le_bytes()); // wildly overstated count
        assert!(matches!(parse(&bytes), Err(ParseError::Truncated)));
    }

    #[test]
    fn etag_is_stable_for_equal_and_differs_for_changed() {
        assert_eq!(etag(&sample(1)), etag(&sample(1)));
        assert_ne!(etag(&sample(1)), etag(&sample(2)));
    }
}
