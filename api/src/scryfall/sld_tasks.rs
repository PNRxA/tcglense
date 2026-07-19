//! Background tasks keeping the Secret Lair drop snapshot fresh.
//!
//! Secret Lair drop titles aren't in the bulk card API, so they're scraped from Scryfall's
//! gallery. The **mirror origin** (`MIRROR_ENABLED`) re-scrapes daily ([`super::sld_scrape`]) and
//! installs the fresh snapshot, so its `/api/mirror/scryfall/sld-drops` endpoint serves current
//! titles; **every other instance** imports that snapshot from the mirror on the same interval
//! ([`super::sld_sync`]) rather than scraping Scryfall itself. Both fall back to the committed
//! `sld_drops.json` until their first successful fetch, and every failure is logged, never fatal.
//!
//! **Both loops refresh at startup whenever the drop store is still the committed seed**
//! ([`drops::store_is_seed`]) — which, because the in-memory store reseeds from that snapshot on
//! every boot, is the case right after every restart. So a restart never keeps serving the stale
//! committed fallback for up to an interval: it re-scrapes / re-imports promptly. The consumer forces
//! that first post-restart import *unconditional* (skipping the persisted `ETag`) so it installs the
//! mirror's current snapshot rather than `304`-ing back onto the seed.
//!
//! Only when a *fresher* snapshot is already loaded at startup (not the seed) does the persisted
//! last-run defer a too-soon re-run to when the interval elapses ([`startup_delay`] →
//! [`initial_delay`]); the last-run time lives in one `ingest_state` row (`(mtg, sld_drops)`). With
//! today's in-memory-only seeding the store always boots on the seed, so that deferral is the
//! insurance path, not the common one. The trade-off: a crash-looping instance re-scrapes /
//! re-imports once per boot (a single gallery GET, or a CDN-absorbed mirror GET) instead of being
//! rate-limited across restarts — accepted, because serving the stale seed is the worse failure.
//!
//! Split out of `tasks.rs` so the Secret Lair machinery lives beside its data ops under
//! `scryfall/`; the generic maintenance/sync orchestration stays in `tasks.rs`.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::DatabaseConnection;
use sea_orm::prelude::DateTimeUtc;

use crate::catalog::ingest_state::{self, StateFields};
use crate::scryfall::{drops, sld_scrape, sld_sync};

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

/// The delay before a loop's **first** refresh. Runs **now** (`ZERO`) whenever the drop store is
/// still the committed seed (`store_is_seed`): the in-memory store reseeds from the committed
/// snapshot on every boot, so a restart that honoured a not-yet-elapsed [`initial_delay`] would serve
/// that stale seed for up to an interval even though scraping/importing works. Only when a *fresher*
/// snapshot is already loaded (not the seed) does it defer per the persisted last-run
/// ([`initial_delay`]) — the rate-limit that stops a too-soon re-run. Kept pure (takes the flag +
/// `now`) so both branches are unit-testable without the global store or a clock.
fn startup_delay(
    store_is_seed: bool,
    last_run: Option<DateTimeUtc>,
    interval_hours: u64,
    now: DateTimeUtc,
) -> Duration {
    if store_is_seed {
        Duration::ZERO
    } else {
        initial_delay(last_run, interval_hours, now)
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

/// Spawn the mirror origin's daily Secret Lair scrape: fetch Scryfall's gallery and install the
/// fresh snapshot into the drop store. A broken scrape (markup change → no drops) or an invalid
/// snapshot is rejected by [`drops::install_snapshot`], so the store keeps its last-good table. The
/// first scrape runs at startup whenever the store is still the committed seed (always, right after a
/// restart — see [`startup_delay`]), so the origin never serves the stale fallback after a boot; only
/// an already-fresh loaded snapshot defers per the persisted last-run. Then every `interval_hours`
/// (a single pass when it's `0`).
pub(crate) fn spawn_sld_scrape(
    db: DatabaseConnection,
    http: Client,
    user_agent: String,
    interval_hours: u64,
) {
    tokio::spawn(async move {
        let (_, last_run) = load_state(&db).await;
        let delay = startup_delay(drops::store_is_seed(), last_run, interval_hours, Utc::now());
        if !delay.is_zero() {
            tracing::info!(
                defer_secs = delay.as_secs(),
                "Secret Lair scrape ran recently and a fresh snapshot is loaded; deferring the first scrape"
            );
            tokio::time::sleep(delay).await;
        }
        loop {
            match sld_scrape::fetch_snapshot_json(&http, &user_agent).await {
                Ok(json) => match drops::install_snapshot(&json) {
                    Ok(count) => {
                        tracing::info!(count, "refreshed Secret Lair drop snapshot from Scryfall");
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

/// Spawn a consumer's daily Secret Lair drop import: pull the snapshot from the mirror and install
/// it. The first import runs at startup whenever the store is still the committed seed (always, right
/// after a restart — see [`startup_delay`]), and is forced *unconditional* then so it installs the
/// mirror's current snapshot rather than `304`-ing back onto the stale seed. Later ticks re-condition
/// on the served `ETag` (an unchanged snapshot is then a cheap `304`), every `interval_hours` (a
/// single import when it's `0`). Every failure is logged, never fatal — the instance keeps serving
/// whatever snapshot it already had loaded.
pub(crate) fn spawn_sld_import(
    db: DatabaseConnection,
    http: Client,
    mirror_base: String,
    interval_hours: u64,
) {
    use crate::scryfall::sld_sync::ImportOutcome;
    tokio::spawn(async move {
        // Restore the last import ETag (to condition later fetches) and last-run time.
        let (mut prev_etag, last_run) = load_state(&db).await;
        let on_seed = drops::store_is_seed();
        let delay = startup_delay(on_seed, last_run, interval_hours, Utc::now());
        if on_seed {
            // The store booted on the committed seed, so a conditional first import could `304` and
            // leave that stale seed in place. Force the first fetch unconditional to actually install
            // the mirror's current snapshot; the loop re-conditions on the served ETag afterwards.
            prev_etag = None;
        }
        if !delay.is_zero() {
            tracing::info!(
                defer_secs = delay.as_secs(),
                "Secret Lair drop import ran recently and a fresh snapshot is loaded; deferring the first import"
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

    #[test]
    fn startup_delay_runs_now_when_store_is_seed_regardless_of_last_run() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // On the seed: refresh immediately even though a run finished a moment ago (which would
        // otherwise defer ~a full interval) — a restart must not keep serving the stale seed.
        assert_eq!(
            startup_delay(true, Some(at(1, now)), 24, now),
            Duration::ZERO
        );
        assert_eq!(startup_delay(true, None, 24, now), Duration::ZERO);
    }

    #[test]
    fn startup_delay_defers_per_last_run_when_a_fresher_snapshot_is_loaded() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // Not the seed (a fresher snapshot already loaded): fall back to the last-run deferral...
        assert_eq!(
            startup_delay(false, Some(at(3600, now)), 24, now),
            Duration::from_secs(23 * 3600)
        );
        // ...and still run now once that deferral has elapsed.
        assert_eq!(
            startup_delay(false, Some(at(24 * 3600, now)), 24, now),
            Duration::ZERO
        );
    }
}
