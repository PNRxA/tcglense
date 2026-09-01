//! Persisted card-sync tick bookkeeping (the 2026-08 price-snapshot outage fix).
//!
//! The periodic card sync used to be a boot-anchored `tokio::time::interval`: its schedule
//! died with the process, and a tick lost to a held leader lock — or to a killed/hung
//! leader — was not retried for a whole `SYNC_INTERVAL_HOURS`. A restart landing near the
//! tick therefore pushed the day's sync (and with it the daily price snapshot) into the
//! same failure window night after night, with no self-healing. The fix mirrors the Secret
//! Lair scrape's design ([`crate::scryfall::sld_tasks`]): persist the last *completed* tick
//! in the shared `ingest_state` table, defer the first attempt at boot by the remainder of
//! the interval since that completion, and retry an incomplete tick after a short delay
//! instead of a full interval (see [`crate::tasks`]).
//!
//! The row is `(game = "all", dataset = "sync_tick")` — the tick spans every game in
//! [`super::GAMES`], so it isn't any one game's row. The status route pins to the card-data
//! dataset, so this extra row never surfaces there (the same contract as the
//! `(mtg, sld_drops)` row).

use chrono::Utc;
use sea_orm::DatabaseConnection;
use sea_orm::prelude::DateTimeUtc;

use super::ingest_state::{self, StateFields};

/// `ingest_state.game` for the tick bookkeeping row: the tick covers every game, so a
/// synthetic key rather than any real game id.
const GAME: &str = "all";

/// `ingest_state.dataset` for the tick bookkeeping row.
const DATASET: &str = "sync_tick";

/// When the last card-sync tick fully completed (refresh + snapshot), or `None` if no
/// completion is recorded. A read error is treated as "never completed" (the loop just runs
/// now) — the worst case is one redundant, version-gated-cheap sync, never a refused one.
pub(crate) async fn last_completed(db: &DatabaseConnection) -> Option<DateTimeUtc> {
    match ingest_state::load(db, GAME, DATASET).await {
        Ok(row) => row.and_then(|r| r.finished_at),
        Err(err) => {
            tracing::warn!(error = %err, "failed to read the card-sync tick state");
            None
        }
    }
}

/// Record a fully-completed tick: stamps `finished_at = now`, the value the boot deferral
/// reads. Only called after refresh + snapshot both returned — a skipped, timed-out, or
/// killed tick leaves the prior row untouched, so the next attempt stays soon. Best-effort:
/// a write failure costs at most one redundant sync after the next restart.
pub(crate) async fn record_completed(db: &DatabaseConnection) {
    let now = Utc::now();
    if let Err(err) = ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET,
            status: "complete",
            source_updated_at: None,
            detail: "card-sync tick completed",
            sets_imported: 0,
            cards_imported: 0,
            started_at: now,
            finished_at: Some(now),
        },
    )
    .await
    {
        tracing::warn!(error = %err, "failed to record the card-sync tick state");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boot deferral reads what a completed tick recorded: a fresh DB reads as "never
    /// completed", and a recorded completion round-trips through the shared `ingest_state`
    /// row with a current `finished_at`.
    #[tokio::test]
    async fn roundtrips_the_last_completed_tick() {
        let db = crate::test_support::migrated_memory_db().await;
        assert_eq!(last_completed(&db).await, None);

        record_completed(&db).await;
        let recorded = last_completed(&db)
            .await
            .expect("a completed tick must be readable back");
        assert!((Utc::now() - recorded).num_seconds() < 60);
    }
}
