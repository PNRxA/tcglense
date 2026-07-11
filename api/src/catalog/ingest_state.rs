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

use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, Iterable, QueryFilter,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use crate::entities::ingest_state;
use crate::entities::prelude::IngestState;

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
