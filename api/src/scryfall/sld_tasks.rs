//! Background tasks keeping the Secret Lair drop snapshot fresh.
//!
//! Secret Lair drop titles aren't in the bulk card API, so they're scraped from Scryfall's
//! gallery. The **mirror origin** (`MIRROR_ENABLED`) re-scrapes daily ([`super::sld_scrape`]) and
//! installs the fresh snapshot, so its `/api/mirror/scryfall/sld-drops` endpoint serves current
//! titles; **every other instance** imports that snapshot from the mirror on the same interval
//! ([`super::sld_sync`]) rather than scraping Scryfall itself. Both fall back to the committed
//! `sld_drops.json` until their first successful fetch, and every failure is logged, never fatal.
//!
//! Both loops run the refresh immediately at startup **unless it already ran within the interval**:
//! the last-run time and the import `ETag` are persisted in one `ingest_state` row
//! (`(mtg, sld_drops)`), so a restart soon after a refresh defers its first run to when the
//! interval elapses (rather than re-scraping/re-importing on every boot), while a restart after a
//! long downtime runs immediately. The persisted `ETag` also lets a consumer's first post-restart
//! import be a cheap conditional `304`.
//!
//! **The drop store is reseeded from the DB-persisted snapshot at boot** ([`super::sld_persist`]),
//! before that deferral: the in-memory store otherwise reseeds from the committed `sld_drops.json`
//! on every boot, so honouring the last-run deferral would serve that stale seed for up to an
//! interval after a restart. Reseeding from the last-good persisted snapshot first means the
//! deferral serves *fresh* drops, the mirror origin serves the same ETag it served before the
//! restart, and a consumer's conditional import `304`s onto the persisted snapshot rather than the
//! seed. Each successful scrape/import is persisted so the next boot has it; the committed file stays
//! the offline / first-boot fallback.
//!
//! Split out of `tasks.rs` so the Secret Lair machinery lives beside its data ops under
//! `scryfall/`; the generic maintenance/sync orchestration stays in `tasks.rs`.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::DatabaseConnection;
use sea_orm::prelude::DateTimeUtc;

use crate::catalog::ingest_state::{self, StateFields};
use crate::scryfall::{drops, sld_persist, sld_scrape, sld_sync};

/// `ingest_state.dataset` key the drop-sync bookkeeping is stored under (per the `mtg` game),
/// reusing the shared ingest-state table so the last-run time + import `ETag` survive restarts.
/// The status route pins to the card-data dataset, so this extra row never surfaces there.
const SLD_DATASET: &str = "sld_drops";

/// The game the Secret Lair set belongs to (the bookkeeping row's `game`).
const GAME: &str = super::GAME;

/// The delay before a loop's **first** refresh: `0` (run now) when the interval is startup-only,
/// nothing has ever run, or the last run is at least an interval old; otherwise the remaining time
/// until the interval elapses since that last run. Kept pure (takes `now`) so the "skip a too-soon
/// re-run across restarts" policy is unit-testable without a clock or a DB.
fn initial_delay(last_run: Option<DateTimeUtc>, interval_hours: u64, now: DateTimeUtc) -> Duration {
    if interval_hours == 0 {
        // Startup-only posture: always run the single pass now.
        return Duration::ZERO;
    }
    let interval = Duration::from_secs(interval_hours.saturating_mul(60 * 60));
    match last_run {
        None => Duration::ZERO, // never ran: run now
        Some(last) => {
            // Negative (a future timestamp from clock skew) -> ZERO elapsed -> wait ~an interval.
            let elapsed = (now - last).to_std().unwrap_or(Duration::ZERO);
            interval.saturating_sub(elapsed) // 0 once at least an interval has passed
        }
    }
}

/// Load the persisted drop-sync state: `(last import ETag, last run time)`. A read error is
/// treated as "never ran" (the loop just runs now), never fatal.
async fn load_state(db: &DatabaseConnection) -> (Option<String>, Option<DateTimeUtc>) {
    match ingest_state::load(db, GAME, SLD_DATASET).await {
        Ok(Some(row)) => (row.source_updated_at, row.finished_at),
        Ok(None) => (None, None),
        Err(err) => {
            tracing::warn!(error = %err, "failed to read Secret Lair drop sync state");
            (None, None)
        }
    }
}

/// Record a completed run in the shared `ingest_state` row: stamps `finished_at = now` (so the next
/// startup can defer a too-soon re-run) and stores `etag` (the import's `ETag`; `None` on the scrape
/// path, which has no upstream validator). Only called after a successful run — a failed
/// scrape/import leaves the prior row untouched so it retries on the next tick/boot. Best-effort.
async fn record_run(db: &DatabaseConnection, etag: Option<&str>, status: &str, detail: &str) {
    let now = Utc::now();
    if let Err(err) = ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: SLD_DATASET,
            status,
            source_updated_at: etag,
            detail,
            sets_imported: 0,
            cards_imported: 0,
            started_at: now,
            finished_at: Some(now),
        },
    )
    .await
    {
        tracing::warn!(error = %err, "failed to record Secret Lair drop sync state");
    }
}

/// Reseed the in-memory drop store from the DB-persisted snapshot ([`sld_persist::load`]) if one
/// exists, so a restart serves the last-good scraped/imported drops immediately instead of the
/// committed seed. Best-effort: a missing row keeps the committed seed, and a corrupt/rejected
/// snapshot is logged and ignored — [`drops::install_snapshot`] validates before swapping, so it can
/// never wipe the seed. Run once at boot, before the first-refresh deferral.
async fn reseed_from_persisted(db: &DatabaseConnection) {
    match sld_persist::load(db).await {
        Ok(Some(json)) => match drops::install_snapshot(&json) {
            Ok(count) => {
                tracing::info!(
                    count,
                    "reseeded Secret Lair drop store from the persisted snapshot"
                )
            }
            Err(err) => tracing::warn!(
                error = %err,
                "persisted Secret Lair snapshot rejected; keeping the committed seed"
            ),
        },
        Ok(None) => {} // nothing persisted yet — keep the committed seed
        Err(err) => {
            tracing::warn!(error = %err, "failed to load the persisted Secret Lair snapshot")
        }
    }
}

/// Persist the snapshot the store now holds — its canonical JSON + content version, read together via
/// [`drops::current_snapshot`] — so the next boot reseeds from it. Called after a successful install;
/// best-effort (a write failure is logged, the live store still serves the fresh drops).
async fn persist_current(db: &DatabaseConnection) {
    let (json, version) = drops::current_snapshot();
    if let Err(err) = sld_persist::save(db, &json, &version).await {
        tracing::warn!(error = %err, "failed to persist the Secret Lair drop snapshot");
    }
}

/// Spawn the mirror origin's daily Secret Lair scrape: fetch Scryfall's gallery and install the
/// fresh snapshot into the drop store. A broken scrape (markup change → no drops) or an invalid
/// snapshot is rejected by [`drops::install_snapshot`], so the store keeps its last-good table. At
/// boot the store is reseeded from the persisted snapshot so the deferral serves the last-good drops
/// (and the mirror serves their ETag) instead of the committed seed; the first scrape then runs
/// unless one ran within the interval (persisted last-run defers it), and each success is persisted.
/// Then every `interval_hours` (a single pass when it's `0`).
pub(crate) fn spawn_sld_scrape(
    db: DatabaseConnection,
    http: Client,
    user_agent: String,
    interval_hours: u64,
) {
    tokio::spawn(async move {
        reseed_from_persisted(&db).await;
        let (_, last_run) = load_state(&db).await;
        let delay = initial_delay(last_run, interval_hours, Utc::now());
        if !delay.is_zero() {
            tracing::info!(
                defer_secs = delay.as_secs(),
                "Secret Lair scrape ran recently; deferring the first scrape"
            );
            tokio::time::sleep(delay).await;
        }
        loop {
            match sld_scrape::fetch_snapshot_json(&http, &user_agent).await {
                Ok(json) => match drops::install_snapshot(&json) {
                    Ok(count) => {
                        tracing::info!(count, "refreshed Secret Lair drop snapshot from Scryfall");
                        // Persist the freshly-installed snapshot *before* recording the run — the two
                        // writes hit different tables and aren't transactional, so on a crash between
                        // them we want the snapshot fresh and the run-time stale (the next boot then
                        // reseeds the fresh drops and re-scrapes immediately), not the reverse (which
                        // would defer while serving the old snapshot).
                        persist_current(&db).await;
                        // No upstream ETag on the gallery scrape — record only the run time.
                        record_run(&db, None, "complete", "scraped from Scryfall").await;
                    }
                    Err(err) => tracing::warn!(
                        error = %err,
                        "scraped Secret Lair snapshot rejected; keeping the current drop table"
                    ),
                },
                Err(err) => tracing::warn!(
                    error = %err,
                    "Secret Lair gallery scrape failed; keeping the current drop table"
                ),
            }
            if interval_hours == 0 {
                break; // startup-only posture: one scrape, no periodic refresh
            }
            tokio::time::sleep(Duration::from_secs(interval_hours.saturating_mul(60 * 60))).await;
        }
    });
}

/// Spawn a consumer's daily Secret Lair drop import: pull the snapshot from the mirror (conditional
/// on the last `ETag`, so an unchanged snapshot is a cheap `304`) and install it. At boot the store
/// is reseeded from the persisted snapshot, so the restored `ETag` matches what's loaded — a first
/// post-restart `304` then keeps the *persisted* fresh snapshot (not the committed seed), and the
/// last-run deferral serves it too. The first import runs at startup (unless deferred), then every
/// `interval_hours` (a single import when it's `0`); each successful import is persisted. Every
/// failure is logged, never fatal — the instance keeps serving whatever snapshot it already had.
pub(crate) fn spawn_sld_import(
    db: DatabaseConnection,
    http: Client,
    mirror_base: String,
    interval_hours: u64,
) {
    use crate::scryfall::sld_sync::ImportOutcome;
    tokio::spawn(async move {
        reseed_from_persisted(&db).await;
        // Restore the last import ETag (so the first fetch can be conditional) and last-run time.
        let (mut prev_etag, last_run) = load_state(&db).await;
        let delay = initial_delay(last_run, interval_hours, Utc::now());
        if !delay.is_zero() {
            tracing::info!(
                defer_secs = delay.as_secs(),
                "Secret Lair drop import ran recently; deferring the first import"
            );
            tokio::time::sleep(delay).await;
        }
        loop {
            match sld_sync::import_from_mirror(&http, &mirror_base, prev_etag.as_deref()).await {
                Ok(ImportOutcome::Imported { count, etag }) => {
                    tracing::info!(count, "imported Secret Lair drop snapshot from the mirror");
                    // Keep the served ETag; if the response carried none, retain the prior one.
                    if let Some(tag) = etag {
                        prev_etag = Some(tag);
                    }
                    // Persist the freshly-imported snapshot *before* recording the ETag. The store
                    // now holds exactly this import, so its canonical JSON is `current_snapshot`.
                    // Order matters: the two writes aren't transactional, and a crash between them
                    // must leave the snapshot fresh with the ETag one behind — the next boot reseeds
                    // the fresh drops and the next conditional import `200`s and repairs the ETag. The
                    // reverse (ETag new, snapshot old) would strand the store on the old snapshot
                    // behind a matching ETag (a `304`) until the mirror next advances.
                    persist_current(&db).await;
                    record_run(
                        &db,
                        prev_etag.as_deref(),
                        "complete",
                        "imported from mirror",
                    )
                    .await;
                }
                Ok(ImportOutcome::Unchanged) => {
                    tracing::debug!("Secret Lair drop snapshot unchanged on the mirror");
                    // Touch the run time (keeping the ETag) so a soon restart defers correctly.
                    record_run(&db, prev_etag.as_deref(), "complete", "unchanged on mirror").await;
                }
                Err(err) => tracing::warn!(
                    error = %err,
                    "Secret Lair drop import from the mirror failed"
                ),
            }
            if interval_hours == 0 {
                break; // startup-only posture: one import, no periodic re-check
            }
            tokio::time::sleep(Duration::from_secs(interval_hours.saturating_mul(60 * 60))).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs_ago: i64, now: DateTimeUtc) -> DateTimeUtc {
        now - chrono::Duration::seconds(secs_ago)
    }

    #[test]
    fn initial_delay_runs_now_when_never_run_or_overdue() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // Never ran -> run now.
        assert_eq!(initial_delay(None, 24, now), Duration::ZERO);
        // Ran exactly an interval ago -> run now.
        assert_eq!(
            initial_delay(Some(at(24 * 3600, now)), 24, now),
            Duration::ZERO
        );
        // Ran well over an interval ago (the "down for > a day" case) -> run now.
        assert_eq!(
            initial_delay(Some(at(72 * 3600, now)), 24, now),
            Duration::ZERO
        );
    }

    #[test]
    fn initial_delay_defers_a_too_soon_run_by_the_remainder() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // Ran 1h ago on a 24h interval -> wait the remaining 23h.
        assert_eq!(
            initial_delay(Some(at(3600, now)), 24, now),
            Duration::from_secs(23 * 3600)
        );
    }

    #[test]
    fn initial_delay_is_zero_for_the_startup_only_interval() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // interval 0 = single startup pass: always run now, even if it ran a moment ago.
        assert_eq!(initial_delay(Some(at(1, now)), 0, now), Duration::ZERO);
        assert_eq!(initial_delay(None, 0, now), Duration::ZERO);
    }

    #[test]
    fn initial_delay_treats_a_future_timestamp_as_not_overdue() {
        // A last-run in the future (clock moved back) mustn't read as "overdue -> run now" with a
        // huge negative elapsed; it defers up to an interval rather than hammering on every boot.
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let future = now + chrono::Duration::seconds(3600);
        assert_eq!(
            initial_delay(Some(future), 24, now),
            Duration::from_secs(24 * 3600)
        );
    }
}
