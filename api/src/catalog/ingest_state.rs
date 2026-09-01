//! Shared `ingest_state` bookkeeping for the version-gated provider syncs.
//!
//! Three provider ingest paths (TCGCSV products [`crate::tcgcsv::ingest`], the TCGCSV
//! historic price backfill [`crate::tcgcsv::backfill`], and MTGJSON sealed contents
//! [`crate::mtgjson::ingest`]) each track their progress in one `(game, dataset)` row of
//! the shared `ingest_state` table. The load / upsert / mark-error mechanics were
//! byte-for-byte identical across them, so they live here once; each provider passes its
//! own `dataset` key and semantics (Scryfall's own path stays in [`crate::scryfall::ingest`]
//! because it additionally redacts secrets via `IngestError::public_detail` and is reused
//! by the dummy seeder).
//!
//! The version gate reads [`Model::source_updated_at`]; a run importing zero rows is
//! recorded as `error` (via [`mark_error`]) rather than version-locked as `complete`, so it
//! retries on the next boot.

use std::time::Duration;

use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, Iterable, QueryFilter,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use crate::entities::ingest_state;
use crate::entities::prelude::IngestState;

/// The delay before a periodic loop's **first** run: `ZERO` (run now) when the interval is
/// startup-only, nothing has ever run, or the last run is at least an interval old; otherwise
/// the remaining time until the interval elapses since that last run. Shared by the Secret
/// Lair drop sync ([`crate::scryfall::sld_tasks`]) and the card-sync tick
/// ([`crate::tasks`] + [`super::sync_state`]), which each persist their last completed run in
/// an `ingest_state` row precisely so a restart soon after a success defers instead of
/// re-running on every boot. Kept pure (takes `now`) so the "skip a too-soon re-run across
/// restarts" policy is unit-testable without a clock or a DB.
pub(crate) fn initial_delay(
    last_run: Option<DateTimeUtc>,
    interval_hours: u64,
    now: DateTimeUtc,
) -> Duration {
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

/// Load the `(game, dataset)` `ingest_state` row, if it exists.
pub async fn load(
    db: &DatabaseConnection,
    game: &str,
    dataset: &str,
) -> Result<Option<ingest_state::Model>, DbErr> {
    IngestState::find()
        .filter(ingest_state::Column::Game.eq(game))
        .filter(ingest_state::Column::Dataset.eq(dataset))
        .one(db)
        .await
}

/// Named fields for an [`ingest_state`] upsert — the borrowed counterpart of
/// `scryfall::ingest`'s `IngestStateUpdate`. Every field is set explicitly (no `Default`)
/// so a caller can't silently drop one; each provider fills `sets_imported` /
/// `cards_imported` with its own meaning (groups + products, days + rows, …).
pub struct StateFields<'a> {
    pub game: &'a str,
    pub dataset: &'a str,
    pub status: &'a str,
    pub source_updated_at: Option<&'a str>,
    pub detail: &'a str,
    pub sets_imported: i32,
    pub cards_imported: i32,
    pub started_at: DateTimeUtc,
    pub finished_at: Option<DateTimeUtc>,
}

/// Upsert the `(game, dataset)` `ingest_state` row, updating every column but the
/// identity/conflict keys (id/game/dataset).
pub async fn put(db: &DatabaseConnection, fields: StateFields<'_>) -> Result<(), DbErr> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(fields.game.to_string()),
        dataset: Set(fields.dataset.to_string()),
        source_updated_at: Set(fields.source_updated_at.map(str::to_string)),
        status: Set(fields.status.to_string()),
        detail: Set(Some(fields.detail.to_string())),
        sets_imported: Set(fields.sets_imported),
        cards_imported: Set(fields.cards_imported),
        started_at: Set(Some(fields.started_at)),
        finished_at: Set(fields.finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns(ingest_state::Column::iter().filter(|c| {
                    !matches!(
                        c,
                        ingest_state::Column::Id
                            | ingest_state::Column::Game
                            | ingest_state::Column::Dataset
                    )
                }))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

/// Best-effort mark the `(game, dataset)` row `error`, preserving the last-known
/// `source_updated_at` (so a transient failure doesn't force a full re-fetch unless the
/// upstream version also changed) and the run's `started_at`. The message is truncated to
/// 500 chars.
pub async fn mark_error(
    db: &DatabaseConnection,
    game: &str,
    dataset: &str,
    message: &str,
) -> Result<(), DbErr> {
    let existing = load(db, game, dataset).await?;
    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    let last = existing.and_then(|s| s.source_updated_at);
    let detail: String = message.chars().take(500).collect();
    put(
        db,
        StateFields {
            game,
            dataset,
            status: "error",
            source_updated_at: last.as_deref(),
            detail: &detail,
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await
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
