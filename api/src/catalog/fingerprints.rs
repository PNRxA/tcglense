//! Card image fingerprints for the visual scanner.
//!
//! Builds and queries a perceptual-hash index over card images so a photographed
//! card can be identified without OCR. The heavy lifting is split cleanly:
//!
//! * **Data ops** here — enumerate cards still needing a hash ([`pending_batch`]),
//!   [`upsert`] a row, [`load_index`] the current hashes into memory, and
//!   [`hash_image_bytes`] (decode + pHash a fetched image).
//! * **Matching** here too — [`FingerprintIndex`] is a read-only in-memory structure
//!   held in [`crate::state::AppState`] and rebuilt after each build/sync;
//!   [`FingerprintIndex::nearest`] is a brute-force Hamming scan (a few milliseconds
//!   over ~90k entries), so there is no vector extension, no ANN, and no
//!   dialect-specific SQL — the fingerprint column is an opaque BLOB on both backends.
//! * **Orchestration** (the polite, incremental build loop) lives in
//!   [`crate::tasks`], which drives these ops behind the opt-in build flag.
//!
//! A normal self-host never runs the build: it imports a prebuilt index (distributed
//! via the dataset mirror) and fetches zero images.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use crate::entities::prelude::{Card, CardFingerprint};
use crate::entities::{card, card_fingerprint};
use crate::phash::{PHASH_BYTES, hamming, phash_from_rgba};
use crate::scryfall::model::StoredFace;

/// One entry in the in-memory match index: the hash plus the card it identifies.
struct IndexEntry {
    game: String,
    external_id: String,
    face_index: i32,
    hash: [u8; PHASH_BYTES],
}

/// A ranked match returned by [`FingerprintIndex::nearest`].
pub struct ScanHit {
    /// External (Scryfall) id of the matched printing.
    pub external_id: String,
    /// Which face matched (`0` for single-faced / front).
    pub face_index: i32,
    /// Hamming distance to the query hash (0 = identical; smaller is closer).
    pub distance: u32,
}

/// Read-only nearest-neighbour index over every current-version fingerprint.
///
/// Rebuilt from the table at startup and after each build/sync pass, then swapped
/// into `AppState` behind an `Arc` (see [`crate::state::AppState::set_fingerprint_index`]).
/// Matching is a linear Hamming scan — trivially fast at this scale and identical on
/// SQLite and Postgres because the DB only ever does insert / select-all.
#[derive(Default)]
pub struct FingerprintIndex {
    entries: Vec<IndexEntry>,
}

impl FingerprintIndex {
    /// Number of fingerprints in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index holds no fingerprints (nothing to match against).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The `top_k` printings nearest to the query for `game`, nearest first. `queries` is
    /// one or more variant hashes of the same scanned card (the crop plus small geometric
    /// corrections the client tries); each card's distance is the **minimum** over all
    /// variants, so a residually-rotated / loosely-cropped scan still matches its tight,
    /// upright reference. At most one hit per printing (the best-matching face). Returns
    /// empty for no queries, `top_k == 0`, or an empty index. Ties break by external id.
    pub fn nearest(&self, game: &str, queries: &[[u8; PHASH_BYTES]], top_k: usize) -> Vec<ScanHit> {
        if queries.is_empty() || top_k == 0 || self.entries.is_empty() {
            return Vec::new();
        }
        let mut best: HashMap<&str, (u32, i32)> = HashMap::new();
        for entry in &self.entries {
            if entry.game != game {
                continue;
            }
            // Best (min) distance across the query's geometric variants.
            let dist = queries
                .iter()
                .map(|q| hamming(&entry.hash, q))
                .min()
                .unwrap_or(u32::MAX);
            best.entry(entry.external_id.as_str())
                .and_modify(|slot| {
                    if dist < slot.0 {
                        *slot = (dist, entry.face_index);
                    }
                })
                .or_insert((dist, entry.face_index));
        }
        let mut hits: Vec<ScanHit> = best
            .into_iter()
            .map(|(external_id, (distance, face_index))| ScanHit {
                external_id: external_id.to_string(),
                face_index,
                distance,
            })
            .collect();
        hits.sort_by(|a, b| {
            a.distance
                .cmp(&b.distance)
                .then_with(|| a.external_id.cmp(&b.external_id))
        });
        hits.truncate(top_k);
        hits
    }
}

/// Load every current-version fingerprint for all games into an in-memory index.
/// Selects only the columns the index needs (no timestamps / source hashes), so even
/// a full catalogue is a small, quick load at startup / after a build.
pub async fn load_index(
    db: &DatabaseConnection,
    algo_version: i32,
) -> Result<FingerprintIndex, DbErr> {
    let rows: Vec<(String, String, i32, Vec<u8>)> = CardFingerprint::find()
        .select_only()
        .column(card_fingerprint::Column::Game)
        .column(card_fingerprint::Column::ExternalId)
        .column(card_fingerprint::Column::FaceIndex)
        .column(card_fingerprint::Column::Fingerprint)
        .filter(card_fingerprint::Column::AlgoVersion.eq(algo_version))
        .into_tuple()
        .all(db)
        .await?;
    let entries = rows
        .into_iter()
        .filter_map(|(game, external_id, face_index, fp)| {
            // A row whose BLOB isn't exactly 32 bytes is corrupt (wrong algo width); skip
            // it rather than let a mis-sized hash produce a `u32::MAX` "never matches" row.
            let hash: [u8; PHASH_BYTES] = fp.as_slice().try_into().ok()?;
            Some(IndexEntry {
                game,
                external_id,
                face_index,
                hash,
            })
        })
        .collect();
    Ok(FingerprintIndex { entries })
}

/// One card that still needs a fingerprint, with its front-face `small` image URL.
pub struct PendingCard {
    pub external_id: String,
    pub image_url: String,
}

/// A window of the "cards needing a fingerprint" walk.
pub struct PendingBatch {
    /// Cards in this window that lack a current front-face fingerprint and have an image.
    pub cards: Vec<PendingCard>,
    /// The largest `cards.id` in the scanned window — advance the resume cursor to this
    /// even when every card was skipped. `None` when no candidate cards remained (done).
    pub last_id: Option<i32>,
}

/// The next `batch_size` cards after `after_id` (id-ordered, resumable) that still lack
/// a current-version front-face fingerprint. Reads only the id / external id / image
/// columns — never the wide card row — and checks existing fingerprints for just this
/// window through the unique index, so it stays off a full scan of the fingerprint
/// table. Cards with no usable `small` image are omitted from `cards` (nothing to
/// hash) but still advance `last_id`, so the walk always makes progress.
pub async fn pending_batch(
    db: &DatabaseConnection,
    game: &str,
    algo_version: i32,
    after_id: i32,
    batch_size: u64,
) -> Result<PendingBatch, DbErr> {
    let rows: Vec<(i32, String, Option<String>, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::ExternalId)
        .column(card::Column::ImageSmall)
        .column(card::Column::CardFaces)
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::Id.gt(after_id))
        .order_by_asc(card::Column::Id)
        .limit(batch_size)
        .into_tuple()
        .all(db)
        .await?;

    let last_id = rows.iter().map(|row| row.0).max();
    if last_id.is_none() {
        return Ok(PendingBatch {
            cards: Vec::new(),
            last_id: None,
        });
    }

    let external_ids: Vec<String> = rows.iter().map(|row| row.1.clone()).collect();
    let existing: HashSet<String> = CardFingerprint::find()
        .select_only()
        .column(card_fingerprint::Column::ExternalId)
        .filter(card_fingerprint::Column::Game.eq(game))
        .filter(card_fingerprint::Column::AlgoVersion.eq(algo_version))
        .filter(card_fingerprint::Column::FaceIndex.eq(0))
        .filter(card_fingerprint::Column::ExternalId.is_in(external_ids))
        .into_tuple::<String>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    let cards = rows
        .into_iter()
        .filter_map(|(_, external_id, image_small, card_faces)| {
            if existing.contains(&external_id) {
                return None;
            }
            let image_url = front_small_url(image_small, card_faces)?;
            Some(PendingCard {
                external_id,
                image_url,
            })
        })
        .collect();

    Ok(PendingBatch { cards, last_id })
}

/// The front-face `small` image URL: the top-level one if present, else the first
/// stored face's (multi-faced cards carry no top-level image). `None` when neither has
/// one — that card simply isn't fingerprinted.
fn front_small_url(image_small: Option<String>, card_faces: Option<String>) -> Option<String> {
    if let Some(url) = image_small.filter(|s| !s.trim().is_empty()) {
        return Some(url);
    }
    let faces: Vec<StoredFace> = card_faces
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())?;
    faces
        .into_iter()
        .next()
        .and_then(|face| face.image_small)
        .filter(|s| !s.trim().is_empty())
}

/// Insert or update the fingerprint row for `(game, external_id, face_index)`, keyed by
/// the table's unique index so a rebuild replaces rather than duplicates.
#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    db: &DatabaseConnection,
    game: &str,
    external_id: &str,
    face_index: i32,
    algo_version: i32,
    fingerprint: &[u8],
    source_size: &str,
    source_image_hash: &str,
) -> Result<(), DbErr> {
    let existing = CardFingerprint::find()
        .filter(card_fingerprint::Column::Game.eq(game))
        .filter(card_fingerprint::Column::ExternalId.eq(external_id))
        .filter(card_fingerprint::Column::FaceIndex.eq(face_index))
        .one(db)
        .await?;
    let now = Utc::now();
    match existing {
        Some(model) => {
            let mut active: card_fingerprint::ActiveModel = model.into();
            active.algo_version = Set(algo_version);
            active.fingerprint = Set(fingerprint.to_vec());
            active.source_size = Set(source_size.to_string());
            active.source_image_hash = Set(source_image_hash.to_string());
            active.updated_at = Set(now);
            active.update(db).await?;
        }
        None => {
            card_fingerprint::ActiveModel {
                game: Set(game.to_string()),
                external_id: Set(external_id.to_string()),
                face_index: Set(face_index),
                algo_version: Set(algo_version),
                fingerprint: Set(fingerprint.to_vec()),
                source_size: Set(source_size.to_string()),
                source_image_hash: Set(source_image_hash.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// Load the full card rows for a set of external ids in one query, keyed by external id
/// — used by the scan endpoint to dress ranked hits with card detail.
pub async fn cards_by_external_id(
    db: &DatabaseConnection,
    game: &str,
    external_ids: Vec<String>,
) -> Result<HashMap<String, card::Model>, DbErr> {
    let rows = Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.is_in(external_ids))
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|card| (card.external_id.clone(), card))
        .collect())
}

/// Decode an image (JPEG/PNG) and compute its 256-bit pHash. `None` if the bytes don't
/// decode. Uses the same [`phash_from_rgba`] pipeline the browser runs on the camera
/// crop, so the reference hash and the query hash are directly comparable. The decoded
/// RGBA buffer is dropped as soon as the hash is computed — nothing is persisted.
pub fn hash_image_bytes(bytes: &[u8]) -> Option<[u8; PHASH_BYTES]> {
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    Some(phash_from_rgba(rgba.as_raw(), width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    fn entry(game: &str, external_id: &str, hash: [u8; PHASH_BYTES]) -> IndexEntry {
        IndexEntry {
            game: game.to_string(),
            external_id: external_id.to_string(),
            face_index: 0,
            hash,
        }
    }

    #[test]
    fn nearest_ranks_by_hamming_and_filters_game() {
        let a = [0u8; PHASH_BYTES];
        let mut b = [0u8; PHASH_BYTES];
        let mut c = [0u8; PHASH_BYTES];
        b[0] = 0b0000_0001; // 1 bit from a
        c[0] = 0b0000_1111; // 4 bits from a
        let index = FingerprintIndex {
            entries: vec![
                entry("mtg", "a", a),
                entry("mtg", "b", b),
                entry("mtg", "c", c),
                entry("other", "z", a), // must be filtered out by game
            ],
        };

        let query = [0u8; PHASH_BYTES];
        let hits = index.nearest("mtg", &[query], 3);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].external_id, "a");
        assert_eq!(hits[0].distance, 0);
        assert_eq!(hits[1].external_id, "b");
        assert_eq!(hits[1].distance, 1);
        assert_eq!(hits[2].external_id, "c");
        assert_eq!(hits[2].distance, 4);
        // The other-game entry never appears.
        assert!(index.nearest("mtg", &[query], 10).iter().all(|h| h.external_id != "z"));
    }

    #[test]
    fn nearest_takes_the_min_distance_across_query_variants() {
        let mut b = [0u8; PHASH_BYTES];
        b[0] = 0b0000_1111; // 4 bits from all-zero
        let index = FingerprintIndex {
            entries: vec![entry("mtg", "a", [0u8; PHASH_BYTES]), entry("mtg", "b", b)],
        };
        // Two variants: one far from "a", one exact — the min wins, so "a" is distance 0.
        let far = {
            let mut q = [0u8; PHASH_BYTES];
            q[5] = 0xff;
            q
        };
        let hits = index.nearest("mtg", &[far, [0u8; PHASH_BYTES]], 1);
        assert_eq!(hits[0].external_id, "a");
        assert_eq!(hits[0].distance, 0);
    }

    #[test]
    fn nearest_dedupes_to_the_best_face_per_printing() {
        let mut near = [0u8; PHASH_BYTES];
        let mut far = [0u8; PHASH_BYTES];
        near[0] = 0b0000_0001;
        far[0] = 0b1111_1111;
        let index = FingerprintIndex {
            entries: vec![
                IndexEntry { game: "mtg".into(), external_id: "dfc".into(), face_index: 0, hash: far },
                IndexEntry { game: "mtg".into(), external_id: "dfc".into(), face_index: 1, hash: near },
            ],
        };
        let hits = index.nearest("mtg", &[[0u8; PHASH_BYTES]], 5);
        assert_eq!(hits.len(), 1, "one hit per printing");
        assert_eq!(hits[0].face_index, 1, "keeps the closer face");
        assert_eq!(hits[0].distance, 1);
    }

    #[test]
    fn nearest_handles_empty_queries_and_zero_k() {
        let index = FingerprintIndex {
            entries: vec![entry("mtg", "a", [0u8; PHASH_BYTES])],
        };
        assert!(index.nearest("mtg", &[], 3).is_empty());
        assert!(index.nearest("mtg", &[[0u8; PHASH_BYTES]], 0).is_empty());
    }

    #[test]
    fn front_small_url_prefers_top_level_then_face() {
        assert_eq!(
            front_small_url(Some("https://img/top.jpg".into()), None),
            Some("https://img/top.jpg".into())
        );
        // Blank top-level falls through to the first face.
        let faces = r#"[{"image_small":"https://img/face0.jpg"},{"image_small":"https://img/face1.jpg"}]"#;
        assert_eq!(
            front_small_url(Some("  ".into()), Some(faces.into())),
            Some("https://img/face0.jpg".into())
        );
        // Nothing usable.
        assert_eq!(front_small_url(None, None), None);
        assert_eq!(front_small_url(None, Some("[]".into())), None);
    }

    #[test]
    fn hash_image_bytes_matches_the_raw_rgba_pipeline() {
        // Encode a small deterministic image to PNG, then confirm decoding + hashing it
        // yields the same hash as running phash_from_rgba on the raw pixels directly.
        let (w, h) = (48u32, 64u32);
        let mut buf = image::RgbaImage::new(w, h);
        for (x, y, px) in buf.enumerate_pixels_mut() {
            let v = ((x * 5 + y * 3) % 256) as u8;
            *px = image::Rgba([v, v, v, 255]);
        }
        let raw = buf.as_raw().clone();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&raw, w, h, ExtendedColorType::Rgba8)
            .expect("encode png");

        let via_bytes = hash_image_bytes(&png).expect("decode + hash");
        let direct = phash_from_rgba(&raw, w as usize, h as usize);
        assert_eq!(via_bytes, direct);
    }

    #[test]
    fn hash_image_bytes_rejects_non_image() {
        assert!(hash_image_bytes(b"not an image").is_none());
    }
}
