//! Background maintenance tasks spawned at startup: refresh-token pruning and,
//! depending on config, either an offline dummy-catalog seed or the periodic
//! card-data sync. Split out of `main` so the orchestration reads at a glance.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::{
    catalog,
    entities::{prelude::User, user},
    state::AppState,
};

/// Publicly-known credentials for the offline dev/e2e account seeded in dummy
/// mode. Registration requires an emailed verification link, which an offline
/// dev/CI run can never receive, so `SEED_DUMMY_DATA` provides one
/// already-verified account to sign in with (the Playwright e2e suite uses it).
/// Only ever seeded behind that flag — never enable it in production.
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
        password_hash: Set(password_hash),
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

/// Periodically prune expired refresh + email tokens so those tables can't grow
/// unbounded. The first tick fires immediately, then every 6 hours.
fn spawn_token_pruner(db: DatabaseConnection) {
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
        }
    });
}

/// Import card data from each provider in the background so the server is available
/// immediately (the SPA shows import progress via the status route), then re-import
/// on a fixed interval to pick up Scryfall's newer prices/sets. The import is
/// idempotent and version-gated, so a tick with no upstream change is cheap (a small
/// bulk-data catalog check, no ~500 MB download).
fn spawn_card_sync(db: DatabaseConnection, http: Client, sync_interval_hours: u64) {
    tokio::spawn(async move {
        if sync_interval_hours == 0 {
            // Periodic refresh disabled: import once on startup only.
            catalog::refresh_all(&db, &http).await;
            // Capture today's snapshot from the freshly-imported cards.
            catalog::snapshot_all(&db).await;
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
            catalog::refresh_all(&db, &http).await;
            // Always capture a daily snapshot, even when the import above was
            // version-gated and skipped — keeps the price series continuous.
            catalog::snapshot_all(&db).await;
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
    spawn_token_pruner(state.db.clone());

    if state.config.seed_dummy_data {
        tracing::warn!(
            "SEED_DUMMY_DATA enabled: seeding a dummy offline catalog and skipping all \
             network card-data sync. Never enable this in production."
        );
        catalog::seed_all(&state.db).await;
        seed_dev_user(&state.db).await;
    } else if state.config.sync_on_startup {
        spawn_card_sync(
            state.db.clone(),
            http.clone(),
            state.config.sync_interval_hours,
        );
    } else {
        tracing::info!("SYNC_ON_STARTUP disabled; skipping card-data import");
    }
}
