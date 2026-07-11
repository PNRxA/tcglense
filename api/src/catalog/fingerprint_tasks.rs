//! The opt-in visual-scanner fingerprint subsystem's background tasks: the operator
//! build (the ONE sanctioned bulk image fetch — hash-and-discard behind
//! `FINGERPRINT_BUILD_ENABLED`) and the self-host import that pulls the prebuilt index
//! from the dataset mirror instead. Split out of `tasks.rs` so the fingerprint machinery
//! lives next to its data ops under `catalog/`; the generic maintenance loop stays in
//! `tasks.rs`.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::Client;
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};

use crate::catalog::fingerprints::FingerprintIndex;
use crate::catalog::images::ImageCache;
use crate::state::AppState;

/// Image size the fingerprint build fetches: the smallest useful crop (146×204), so a
/// full-catalogue pass moves the least bytes and keeps the whole disambiguating frame.
const FINGERPRINT_SOURCE_SIZE: &str = "small";

/// Cards read per DB batch while walking for un-fingerprinted cards (bounded + resumable).
const FINGERPRINT_BATCH: u64 = 200;

/// Republish the in-memory match index after this many new fingerprints during a build,
/// so the scanner starts recognising cards while the (long) first pass is still running
/// rather than only once the whole catalogue is done.
const FINGERPRINT_INDEX_REFRESH_EVERY: u64 = 500;

/// Everything the opt-in fingerprint build needs, threaded from [`AppState`] once the
/// operator has enabled it. Cheaply cloneable (all `Arc`/`Copy`).
#[derive(Clone)]
pub(crate) struct FingerprintBuild {
    /// The polite image downloader (shared 8-way cap + host allow-list); the build
    /// fetches bytes through it and discards them after hashing (never persists to disk).
    images: Arc<ImageCache>,
    /// The live match index to refresh after a build pass.
    index_slot: Arc<RwLock<Arc<FingerprintIndex>>>,
    /// Version stamped on built rows and used to load the index.
    algo_version: i32,
    /// Hours between re-scans (to fingerprint newly-synced cards); `0` = a single pass.
    rebuild_interval_hours: u64,
}

/// Assemble the build config from state (only meaningful when the build is enabled).
pub(crate) fn fingerprint_build(state: &AppState) -> FingerprintBuild {
    FingerprintBuild {
        images: state.images.clone(),
        index_slot: state.fingerprint_index.clone(),
        algo_version: state.config.fingerprint_algo_version,
        rebuild_interval_hours: state.config.sync_interval_hours,
    }
}

/// Load the current fingerprints from the table and swap them into the live match index.
/// Shared by the build and import paths (each holds the same index slot + algo version).
async fn reload_fingerprint_index(
    db: &DatabaseConnection,
    algo_version: i32,
    index_slot: &Arc<RwLock<Arc<FingerprintIndex>>>,
) {
    match crate::catalog::fingerprints::load_index(db, algo_version).await {
        Ok(index) => {
            tracing::info!(count = index.len(), "loaded card-fingerprint match index");
            *index_slot.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(index);
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to load card-fingerprint match index")
        }
    }
}

/// Walk the catalogue once, fetching + hashing the `small` image of every card that
/// still lacks a current-version front-face fingerprint, and upserting the result. The
/// image bytes are dropped immediately after hashing — nothing is written to the image
/// cache. Resumable by the `cards.id` cursor; per-card failures are logged and skipped.
/// Publishes the match index every [`FINGERPRINT_INDEX_REFRESH_EVERY`] new fingerprints
/// so the scanner comes online progressively. Returns how many fingerprints were built.
async fn run_fingerprint_pass(
    db: &DatabaseConnection,
    cfg: &FingerprintBuild,
) -> Result<u64, sea_orm::DbErr> {
    use crate::catalog::fingerprints;

    let game = crate::scryfall::GAME;
    let mut after_id = 0i32;
    let mut built = 0u64;
    let mut refreshed_at = 0u64;
    loop {
        let batch =
            fingerprints::pending_batch(db, game, cfg.algo_version, after_id, FINGERPRINT_BATCH)
                .await?;
        let Some(last_id) = batch.last_id else {
            break; // no more candidate cards — the walk is done
        };
        // Fetch + hash this batch's images CONCURRENTLY: the shared 8-way cap inside
        // `fetch_bytes` bounds real parallelism (no artificial delay — the image CDN
        // isn't rate-limited), and the network round-trip dominates, so this is ~8× the
        // serial throughput. Decode+hash is cheap and done inline; the bytes are dropped
        // as soon as the hash is computed.
        let hashed = futures_util::future::join_all(batch.cards.into_iter().map(|pending| {
            let images = cfg.images.clone();
            async move {
                match images.fetch_bytes(&pending.image_url).await {
                    Ok(bytes) => match fingerprints::hash_image_bytes(&bytes) {
                        Some(hash) => {
                            Some((pending.external_id, hash, hex::encode(Sha256::digest(&bytes))))
                        }
                        None => {
                            tracing::debug!(id = %pending.external_id, "fetched image did not decode; skipping");
                            None
                        }
                    },
                    Err(err) => {
                        tracing::debug!(id = %pending.external_id, error = %err, "fingerprint image fetch failed; skipping");
                        None
                    }
                }
            }
        }))
        .await;
        // Upsert serially — the SQLite pool is single-writer anyway, and the writes are
        // fast relative to the network fetches above.
        for (external_id, hash, source_hash) in hashed.into_iter().flatten() {
            match fingerprints::upsert(
                db,
                game,
                &external_id,
                0,
                cfg.algo_version,
                &hash,
                FINGERPRINT_SOURCE_SIZE,
                &source_hash,
            )
            .await
            {
                Ok(()) => built += 1,
                Err(err) => {
                    tracing::warn!(id = %external_id, error = %err, "failed to store card fingerprint")
                }
            }
        }
        after_id = last_id;
        // Publish partial progress so the scanner recognises the cards done so far while
        // the rest of the (long) first pass keeps running.
        if built - refreshed_at >= FINGERPRINT_INDEX_REFRESH_EVERY {
            reload_fingerprint_index(db, cfg.algo_version, &cfg.index_slot).await;
            refreshed_at = built;
        }
    }
    Ok(built)
}

/// Spawn the detached, opt-in fingerprint build (only when `FINGERPRINT_BUILD_ENABLED`).
/// Mirrors [`spawn_price_backfill`]: a long, polite background walk kept off the sync
/// ticker, internally incremental (skips already-fingerprinted cards) and resumable, so
/// re-running after completion is cheap. After each pass it reloads the in-memory match
/// index. With a periodic sync it re-scans every `rebuild_interval_hours` to pick up
/// newly-synced cards; with periodic sync disabled it runs a single pass. Errors are
/// logged, never fatal.
///
/// [`spawn_price_backfill`]: crate::tasks
pub(crate) fn spawn_fingerprint_build(db: DatabaseConnection, cfg: FingerprintBuild) {
    tokio::spawn(async move {
        loop {
            match run_fingerprint_pass(&db, &cfg).await {
                Ok(built) if built > 0 => {
                    tracing::info!(built, "card-fingerprint build pass complete")
                }
                Ok(_) => tracing::debug!("card-fingerprint build pass: nothing to do"),
                Err(err) => tracing::error!(error = %err, "card-fingerprint build pass failed"),
            }
            // Final refresh off the freshly-built table (partial refreshes happened
            // during the pass; this catches the tail).
            reload_fingerprint_index(&db, cfg.algo_version, &cfg.index_slot).await;
            if cfg.rebuild_interval_hours == 0 {
                break; // startup-only posture: one pass, no periodic re-scan
            }
            tokio::time::sleep(Duration::from_secs(
                cfg.rebuild_interval_hours.saturating_mul(60 * 60),
            ))
            .await;
        }
    });
}

/// Everything the fingerprint **import** needs on a self-host that pulls the prebuilt
/// index from the mirror instead of building it. Cheaply cloneable (an `Arc`, a `Client`,
/// a short `String`).
#[derive(Clone)]
pub(crate) struct FingerprintImport {
    /// Shared HTTP client used for the conditional mirror fetch.
    http: Client,
    /// Mirror origin to pull the index from (`DATASET_MIRROR_URL`), trailing slash trimmed.
    mirror_base: String,
    /// The live match index to republish after an import.
    index_slot: Arc<RwLock<Arc<FingerprintIndex>>>,
    /// The algo version this instance expects; a mirror index built at another is skipped.
    algo_version: i32,
    /// Hours between re-checks of the mirror (`0` = a single import at startup).
    interval_hours: u64,
}

/// Assemble the import config from state (only meaningful when import is enabled).
pub(crate) fn fingerprint_import(state: &AppState, http: &Client) -> FingerprintImport {
    FingerprintImport {
        http: http.clone(),
        mirror_base: state.config.dataset_mirror_url.clone(),
        index_slot: state.fingerprint_index.clone(),
        algo_version: state.config.fingerprint_algo_version,
        interval_hours: state.config.sync_interval_hours,
    }
}

/// Spawn the detached fingerprint **import** (a self-host that isn't the builder): pull
/// the prebuilt index from the mirror, replace the local table when it changed, and
/// republish the match index. Conditional on the ETag stored in `ingest_state`, so an
/// unchanged index costs a single cheap `304`. Loops on the sync interval (or runs once
/// when it's `0`). Every failure is logged, never fatal — the scanner keeps serving
/// whatever index it already had loaded at startup.
pub(crate) fn spawn_fingerprint_import(db: DatabaseConnection, cfg: FingerprintImport) {
    use crate::catalog::fingerprint_sync::{self, ImportOutcome};
    let game = crate::scryfall::GAME;
    tokio::spawn(async move {
        loop {
            let prev_etag = match fingerprint_sync::last_import_etag(&db, game).await {
                Ok(etag) => etag,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to read fingerprint import state");
                    None
                }
            };
            match fingerprint_sync::import_from_mirror(
                &db,
                &cfg.http,
                &cfg.mirror_base,
                game,
                cfg.algo_version,
                prev_etag.as_deref(),
            )
            .await
            {
                Ok(ImportOutcome::Imported { count, etag }) => {
                    tracing::info!(count, "imported card-fingerprint index from the mirror");
                    if let Err(err) = fingerprint_sync::record_import(
                        &db,
                        game,
                        etag.as_deref(),
                        "complete",
                        "imported from mirror",
                        count as i32,
                    )
                    .await
                    {
                        tracing::warn!(error = %err, "failed to record fingerprint import state");
                    }
                    reload_fingerprint_index(&db, cfg.algo_version, &cfg.index_slot).await;
                }
                Ok(ImportOutcome::Unchanged) => {
                    tracing::debug!("card-fingerprint index unchanged on the mirror")
                }
                Ok(ImportOutcome::AlgoMismatch { served, expected }) => tracing::warn!(
                    served,
                    expected,
                    "mirror fingerprint index was built at a different algo version; skipping \
                     import (align FINGERPRINT_ALGO_VERSION with the mirror + the web bundle)"
                ),
                Err(err) => {
                    tracing::warn!(error = %err, "card-fingerprint import from the mirror failed")
                }
            }
            if cfg.interval_hours == 0 {
                break; // startup-only posture: one import, no periodic re-check
            }
            tokio::time::sleep(Duration::from_secs(
                cfg.interval_hours.saturating_mul(60 * 60),
            ))
            .await;
        }
    });
}
