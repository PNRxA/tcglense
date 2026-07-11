use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

use crate::scryfall::subtypes::HAS_SUBTYPE_SQL_ARMS;

/// Replaces `m..033`'s covering index with a **partial** index for the by-treatment set gate.
///
/// `scryfall::subtypes::sets_with_subtypes` runs `SELECT DISTINCT set_code FROM cards WHERE
/// game = ? AND (<has_subtype_condition>)`, where the predicate is an OR of a leading-wildcard
/// `frame_effects LIKE '%,showcase,%'`, the `extendedart` twin, `border_color = 'borderless'`,
/// and `full_art = true`. Only `game = ?` is sargable; every OR arm is unseekable.
///
/// `m..033` added a covering index `(game, set_code, frame_effects, border_color, full_art)` on
/// the theory it yields a heap-free index-only scan. It doesn't fix the latency, for two reasons
/// its comment missed:
///   1. **It never narrows the rows examined.** The OR-of-unseekables can only be a post-read
///      `Filter`, so even at its best the plan reads and filters *every* tuple of the `mtg`
///      partition — effectively the whole catalog — though only ~2% of cards are special. Cost
///      scales with total card count, not special-card count.
///   2. **The "no heap access" claim is contingent on a populated visibility map.** `cards` is
///      re-ingested and price-updated on an ~hourly cadence and the schema never `VACUUM`s, so
///      the VM's all-visible bits are chronically cleared; the "index-only" scan degrades into a
///      per-tuple heap fetch, and on a freshly re-synced table the planner drops it for a seq /
///      bitmap-heap scan (measured *slower*). Net: still a ~1.4 s full-partition scan in prod.
///
/// A **partial** index whose `WHERE` is exactly `has_subtype_condition()` stores only the special
/// rows. The query predicate provably implies the index predicate, so the planner serves it as an
/// Index Only Scan bounded by the special-card count, streaming the `DISTINCT set_code` in
/// `(game, set_code)` order with no sort — and, being tiny, it stays cheap regardless of VM state
/// (measured on 400k representative rows: ~1 ms / 72 KB index vs ~127 ms / 3 MB for `m..033`).
///
/// **Lock-step:** the predicate is rendered from [`HAS_SUBTYPE_SQL_ARMS`], the same constant
/// `has_subtype_condition()` builds the query filter from, so the two are byte-identical here. A
/// later edit to that constant needs a *new* migration to rebuild this index; if they drift, the
/// planner silently falls back to the full scan (guarded by the `subtypes` drift-canary test).
///
/// sea-query's `IndexCreateStatement` has no partial-`WHERE` builder, so — like `m..027`'s
/// expression indexes — this goes through raw `execute_unprepared`. Unlike `m..027` no
/// `db::Dialect` gate is needed: partial indexes and every arm (`||`, `LOWER`, `COALESCE`,
/// `LIKE`, `=`, `full_art = true`) render identically on SQLite and Postgres. Plain (non-
/// `CONCURRENTLY`) `CREATE INDEX` takes a brief `SHARE` lock on `cards`; card writes happen only
/// during the periodic sync, so it is normally uncontended (same caveat as `m..027`).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop the superseded covering index. Its only other would-be consumer, the per-set gate
        // `set_has_subtypes`, is a `(game, set_code)` point lookup already served by
        // `idx_cards_game_set_code_collector_number`'s prefix — so nothing else needs it.
        manager
            .drop_index(
                Index::drop()
                    .if_exists()
                    .name("idx_cards_game_subtype_facet")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await?;

        let predicate = HAS_SUBTYPE_SQL_ARMS.join(" OR ");
        manager
            .get_connection()
            .execute_unprepared(&format!(
                "CREATE INDEX IF NOT EXISTS \"idx_cards_subtype\" \
                 ON \"cards\" (\"game\", \"set_code\") WHERE {predicate}"
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS \"idx_cards_subtype\"")
            .await?;
        // Restore `m..033`'s covering index so `down()` is a faithful inverse.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_cards_game_subtype_facet")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .col(Cards::FrameEffects)
                    .col(Cards::BorderColor)
                    .col(Cards::FullArt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    SetCode,
    FrameEffects,
    BorderColor,
    FullArt,
}
