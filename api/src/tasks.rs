//! Background maintenance tasks spawned at startup: refresh-token pruning and,
//! depending on config, either an offline dummy-catalog seed or the periodic
//! card-data sync. Split out of `main` so the orchestration reads at a glance.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::{
    catalog,
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
        spawn_card_sync(
            state.db.clone(),
            http.clone(),
            state.config.sync_interval_hours,
            state.config.tcgcsv_user_agent.clone(),
            backfill,
            SyncSource::from_config(&state.config),
        );
    } else {
        tracing::info!("SYNC_ON_STARTUP disabled; skipping card-data import");
        // No sync will run enrich_foil_variant_prices per tick, so do it once here against
        // the existing catalog — otherwise a foil-★ holding folded by the m..023 migration
        // values at $0 (issue #209).
        spawn_foil_price_enrichment(state.db.clone());
    }
}
