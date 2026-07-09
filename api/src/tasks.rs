//! Background maintenance tasks spawned at startup: refresh-token pruning and,
//! depending on config, either an offline dummy-catalog seed or the periodic
//! card-data sync. Split out of `main` so the orchestration reads at a glance.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};

use crate::{
    catalog,
    catalog::fingerprints::FingerprintIndex,
    catalog::images::ImageCache,
    datasets::SyncSource,
    entities::{prelude::User, user},
    ratelimit::{AuthRateLimiter, UserRateLimiter},
    state::AppState,
};

/// Publicly-known credentials for the offline dev/e2e account seeded in dummy
/// mode: a ready-made, already-verified account to sign in with, so the
/// Playwright e2e suite's login/session journeys don't have to register first
/// (offline registration works too — with no email provider the register
/// response carries the completion token — but a stable seeded account keeps
/// those tests independent of the registration flow). Only ever seeded behind
/// `SEED_DUMMY_DATA` — never enable it in production.
const DEV_USER_EMAIL: &str = "e2e@tcglense.test";
const DEV_USER_PASSWORD: &str = "password123";

/// Seed the verified offline dev/e2e user (dummy mode only). Idempotent like
/// the catalog seed: an existing row is left untouched. Errors are logged, not
/// fatal — the catalog is still usable without the account.
async fn seed_dev_user(db: &DatabaseConnection) {
    match User::find()
        .filter(user::Column::Email.eq(DEV_USER_EMAIL))
        .one(db)
        .await
    {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(error = %err, "failed to look up the dummy dev user");
            return;
        }
    }

    let password_hash = match crate::auth::password::hash_password(DEV_USER_PASSWORD) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::warn!(error = %err, "failed to hash the dummy dev user password");
            return;
        }
    };
    let now = Utc::now();
    let result = user::ActiveModel {
        email: Set(DEV_USER_EMAIL.to_string()),
        password_hash: Set(Some(password_hash)),
        display_name: Set(Some("Dev Tester".to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        email_verified_at: Set(Some(now)),
        ..Default::default()
    }
    .insert(db)
    .await;
    match result {
        Ok(_) => tracing::info!("seeded the verified offline dev/e2e user {DEV_USER_EMAIL}"),
        Err(err) => tracing::warn!(error = %err, "failed to seed the dummy dev user"),
    }
}

/// Periodic maintenance: prune expired refresh + email tokens and dead (expired or
/// revoked) API keys so those tables can't grow unbounded, and drop replenished
/// rate-limiter keys (both the per-IP
/// and the per-user sets) so those keyspaces can't either. When a limiter is
/// Redis-backed its keys self-evict via `PEXPIRE`, so `retain_recent` there only
/// sweeps the in-memory fail-open fallback (cheap and harmless). The first tick
/// fires immediately, then every 6 hours.
fn spawn_maintenance(
    db: DatabaseConnection,
    rate_limiters: Arc<AuthRateLimiter>,
    user_rate_limiters: Arc<UserRateLimiter>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
        loop {
            ticker.tick().await;
            match crate::auth::refresh::prune_expired(&db).await {
                Ok(n) if n > 0 => tracing::info!("pruned {n} expired refresh tokens"),
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "failed to prune expired refresh tokens")
                }
            }
            match crate::auth::email_token::prune_expired(&db).await {
                Ok(n) if n > 0 => tracing::info!("pruned {n} expired email tokens"),
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "failed to prune expired email tokens")
                }
            }
            match crate::auth::api_key::prune_dead(&db).await {
                Ok(n) if n > 0 => tracing::info!("pruned {n} dead api keys"),
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "failed to prune dead api keys")
                }
            }
            rate_limiters.retain_recent();
            user_rate_limiters.retain_recent();
        }
    });
}

/// Enrich the foil-variant base prices once, off the startup path. Normally
/// [`catalog::refresh_all`] does this every sync tick, but when the periodic sync is
/// disabled (`SYNC_ON_STARTUP=false`) it never runs — yet the `m..023` migration may have
/// folded legacy foil-★ holdings onto their nonfoil base, whose foil price would then stay
/// empty and value those copies at $0 (issue #209). This closes that gap against the
/// already-synced catalog. A no-op on a fresh/dummy catalog with no such pairs.
fn spawn_foil_price_enrichment(db: DatabaseConnection) {
    tokio::spawn(async move {
        match crate::scryfall::enrich_foil_variant_prices(&db).await {
            Ok(rows) if rows > 0 => tracing::info!(rows, "enriched foil-variant base prices"),
            Ok(_) => {}
            Err(err) => tracing::error!(error = %err, "foil-variant price enrichment failed"),
        }
    });
}

/// Parameters for the one-time TCGCSV historic price backfill, when enabled. `None`
/// disables it. Passed into the card-sync task so the backfill can start once the
/// first card sync has populated `cards.tcgplayer_id` (its join key).
struct BackfillConfig {
    user_agent: String,
    days: u32,
}

/// Kick off the one-time historic price backfill (once cards exist) as its own
/// detached task, so a long walk over TCGCSV's daily archives never blocks the
/// periodic card-sync ticker. The backfill is internally gated (an `ingest_state`
/// row), so re-invoking it after it has completed is a cheap no-op. Errors are
/// logged, never fatal.
fn spawn_price_backfill(
    db: DatabaseConnection,
    http: Client,
    cfg: BackfillConfig,
    source: SyncSource,
) {
    tokio::spawn(async move {
        if let Err(err) =
            crate::tcgcsv::backfill::run(&db, &http, &cfg.user_agent, cfg.days, &source).await
        {
            tracing::error!(error = %err, "tcgcsv price backfill failed");
        }
    });
}

/// Image size the fingerprint build fetches: the smallest useful crop (146×204), so a
/// full-catalogue pass moves the least bytes and keeps the whole disambiguating frame.
const FINGERPRINT_SOURCE_SIZE: &str = "small";

/// Cards read per DB batch while walking for un-fingerprinted cards (bounded + resumable).
const FINGERPRINT_BATCH: u64 = 200;

/// Everything the opt-in fingerprint build needs, threaded from [`AppState`] once the
/// operator has enabled it. Cheaply cloneable (all `Arc`/`Copy`).
#[derive(Clone)]
struct FingerprintBuild {
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
fn fingerprint_build(state: &AppState) -> FingerprintBuild {
    FingerprintBuild {
        images: state.images.clone(),
        index_slot: state.fingerprint_index.clone(),
        algo_version: state.config.fingerprint_algo_version,
        rebuild_interval_hours: state.config.sync_interval_hours,
    }
}

/// Walk the catalogue once, fetching + hashing the `small` image of every card that
/// still lacks a current-version front-face fingerprint, and upserting the result. The
/// image bytes are dropped immediately after hashing — nothing is written to the image
/// cache. Resumable by the `cards.id` cursor; per-card failures are logged and skipped.
/// Returns how many fingerprints were built this pass.
async fn run_fingerprint_pass(
    db: &DatabaseConnection,
    cfg: &FingerprintBuild,
) -> Result<u64, sea_orm::DbErr> {
    use crate::catalog::fingerprints;

    let game = crate::scryfall::GAME;
    let mut after_id = 0i32;
    let mut built = 0u64;
    loop {
        let batch =
            fingerprints::pending_batch(db, game, cfg.algo_version, after_id, FINGERPRINT_BATCH)
                .await?;
        let Some(last_id) = batch.last_id else {
            break; // no more candidate cards — the walk is done
        };
        for pending in batch.cards {
            match cfg.images.fetch_bytes(&pending.image_url).await {
                Ok(bytes) => match fingerprints::hash_image_bytes(&bytes) {
                    Some(hash) => {
                        let source_hash = hex::encode(Sha256::digest(&bytes));
                        match fingerprints::upsert(
                            db,
                            game,
                            &pending.external_id,
                            0,
                            cfg.algo_version,
                            &hash,
                            FINGERPRINT_SOURCE_SIZE,
                            &source_hash,
                        )
                        .await
                        {
                            Ok(()) => built += 1,
                            Err(err) => tracing::warn!(
                                id = %pending.external_id,
                                error = %err,
                                "failed to store card fingerprint"
                            ),
                        }
                    }
                    None => tracing::debug!(
                        id = %pending.external_id,
                        "fetched image did not decode; skipping"
                    ),
                },
                Err(err) => tracing::debug!(
                    id = %pending.external_id,
                    error = %err,
                    "fingerprint image fetch failed; skipping"
                ),
            }
            // No artificial per-request delay: the image CDN isn't rate-limited, and the
            // shared 8-way concurrency cap in `fetch_bytes` is the politeness bound.
        }
        after_id = last_id;
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
fn spawn_fingerprint_build(db: DatabaseConnection, cfg: FingerprintBuild) {
    tokio::spawn(async move {
        loop {
            match run_fingerprint_pass(&db, &cfg).await {
                Ok(built) if built > 0 => {
                    tracing::info!(built, "card-fingerprint build pass complete")
                }
                Ok(_) => tracing::debug!("card-fingerprint build pass: nothing to do"),
                Err(err) => tracing::error!(error = %err, "card-fingerprint build pass failed"),
            }
            // Refresh the live match index off the freshly-built table.
            match crate::catalog::fingerprints::load_index(&db, cfg.algo_version).await {
                Ok(index) => {
                    tracing::info!(count = index.len(), "loaded card-fingerprint match index");
                    *cfg.index_slot.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(index);
                }
                Err(err) => {
                    tracing::error!(error = %err, "failed to load card-fingerprint match index")
                }
            }
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

/// Import card data from each provider in the background so the server is available
/// immediately (the SPA shows import progress via the status route), then re-import
/// on a fixed interval to pick up Scryfall's newer prices/sets. The import is
/// idempotent and version-gated, so a tick with no upstream change is cheap (a small
/// bulk-data catalog check, no ~500 MB download).
///
/// After the first sync completes (so `cards.tcgplayer_id` is populated), the
/// one-time TCGCSV historic price backfill is spawned if `backfill` is `Some`.
fn spawn_card_sync(
    db: DatabaseConnection,
    http: Client,
    sync_interval_hours: u64,
    tcgcsv_user_agent: String,
    mut backfill: Option<BackfillConfig>,
    mut fingerprint: Option<FingerprintBuild>,
    source: SyncSource,
) {
    tokio::spawn(async move {
        if sync_interval_hours == 0 {
            // Periodic refresh disabled: import once on startup only.
            catalog::refresh_all(&db, &http, &tcgcsv_user_agent, &source).await;
            // Capture today's snapshot from the freshly-imported cards + products.
            catalog::snapshot_all(&db).await;
            if let Some(cfg) = backfill.take() {
                spawn_price_backfill(db.clone(), http.clone(), cfg, source.clone());
            }
            // Cards now exist, so the opt-in fingerprint build can walk them.
            if let Some(cfg) = fingerprint.take() {
                spawn_fingerprint_build(db.clone(), cfg);
            }
            return;
        }
        // saturating_mul so an absurd SYNC_INTERVAL_HOURS can't overflow the
        // u64: an overflow panics in debug and, worse, can wrap to a zero period
        // in release — which tokio::time::interval itself panics on, slipping
        // past the `== 0` guard above.
        let period = Duration::from_secs(sync_interval_hours.saturating_mul(60 * 60));
        let mut ticker = tokio::time::interval(period);
        // If a refresh ever runs long, skip the ticks it overran rather than
        // firing them back-to-back (the default Burst behaviour would).
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            // The first tick fires immediately (the startup import), then every
            // `sync_interval_hours` thereafter.
            ticker.tick().await;
            catalog::refresh_all(&db, &http, &tcgcsv_user_agent, &source).await;
            // Always capture a daily snapshot, even when the import above was
            // version-gated and skipped — keeps the price series continuous.
            catalog::snapshot_all(&db).await;
            // After the first successful sync, cards carry their tcgplayer_id, so
            // the one-time historic backfill can join against them. `take` ensures
            // it's only spawned once.
            if let Some(cfg) = backfill.take() {
                spawn_price_backfill(db.clone(), http.clone(), cfg, source.clone());
            }
            // Same one-time spawn for the opt-in fingerprint build (cards now exist to
            // walk); its own loop then re-scans on the sync interval for new cards.
            if let Some(cfg) = fingerprint.take() {
                spawn_fingerprint_build(db.clone(), cfg);
            }
        }
    });
}

/// Spawn all background tasks: the refresh-token pruner (always) plus, depending on
/// config, either the offline dummy-catalog seed or the periodic card-data sync.
///
/// SEED_DUMMY_DATA takes precedence over SYNC_ON_STARTUP / SYNC_INTERVAL_HOURS:
/// seed a small offline dummy catalog and perform NO network sync (no startup
/// import, no periodic refresh). We await it here rather than spawning — unlike
/// the ~500 MB real import it's a handful of local inserts, so the catalog is
/// present before the first request (handy for CI/e2e). A seed error is logged but
/// does not abort startup. Never enable this outside dev/CI/test.
pub async fn start(state: &AppState, http: &Client) {
    spawn_maintenance(
        state.db.clone(),
        state.rate_limiters.clone(),
        state.user_rate_limiters.clone(),
    );

    // Load any already-built / imported fingerprint index into memory before serving, so
    // the visual scanner works immediately on an instance that imported a prebuilt index
    // (the common self-host case — it never runs the build itself). Empty on a fresh DB;
    // the build task (if enabled) refreshes it after each pass. Non-fatal on error.
    match catalog::fingerprints::load_index(&state.db, state.config.fingerprint_algo_version).await
    {
        Ok(index) => {
            if !index.is_empty() {
                tracing::info!(count = index.len(), "loaded card-fingerprint match index");
            }
            state.set_fingerprint_index(index);
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to load card-fingerprint match index at startup")
        }
    }

    if state.config.seed_dummy_data {
        tracing::warn!(
            "SEED_DUMMY_DATA enabled: seeding a dummy offline catalog and skipping all \
             network card-data sync. Never enable this in production."
        );
        catalog::seed_all(&state.db).await;
        seed_dev_user(&state.db).await;
    } else if state.config.sync_on_startup {
        // The historic price backfill runs only outside dummy mode (real cards must
        // exist to join against) and when enabled.
        let backfill = state.config.price_backfill_enabled.then(|| BackfillConfig {
            user_agent: state.config.tcgcsv_user_agent.clone(),
            days: state.config.price_backfill_days,
        });
        // The opt-in visual-scanner fingerprint build (off by default); spawned after the
        // first sync populates cards to walk (see `spawn_card_sync`).
        let fingerprint = state
            .config
            .fingerprint_build_enabled
            .then(|| fingerprint_build(state));
        spawn_card_sync(
            state.db.clone(),
            http.clone(),
            state.config.sync_interval_hours,
            state.config.tcgcsv_user_agent.clone(),
            backfill,
            fingerprint,
            SyncSource::from_config(&state.config),
        );
    } else {
        tracing::info!("SYNC_ON_STARTUP disabled; skipping card-data import");
        // No sync will run enrich_foil_variant_prices per tick, so do it once here against
        // the existing catalog — otherwise a foil-★ holding folded by the m..023 migration
        // values at $0 (issue #209).
        spawn_foil_price_enrichment(state.db.clone());
        // Cards already exist from a prior run (no sync this boot); if the operator opted
        // into the fingerprint build, run it against the existing catalogue.
        if state.config.fingerprint_build_enabled {
            spawn_fingerprint_build(state.db.clone(), fingerprint_build(state));
        }
    }
}
