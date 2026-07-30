use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

/// Index the `card_art_tags` mapping for the card page's "Artwork tags" read —
/// "every tag on THIS artwork" (`/api/games/{game}/cards/{id}/art-tags`), which filters
/// on `(game, illustration_id)` and selects only `tag_slug`.
///
/// Notes:
/// - **Three columns, not two.** `(game, illustration_id)` alone satisfies the WHERE but
///   not the projection, and on SQLite — the default backend, whose planner stats are a
///   deliberate non-goal, so `ANALYZE` never runs — a non-covering index loses to
///   `m..063`'s *covering* `(game, tag_slug, illustration_id)`: with no `sqlite_stat1`,
///   every index gets the same fixed row estimate, so the planner can't tell that
///   `game = 'mtg'` is the whole table and happily index-only-scans the entire ~1M-row
///   partition. Measured on a ~1M-row fixture: 76 ms per request with the two-column
///   form (planner picks `m..063`, exactly the scan this migration exists to remove)
///   versus 0.01 ms once `tag_slug` is appended and the index covers the query. Adding
///   the column is what makes the index get used at all.
/// - **Not unique**: one artwork legitimately carries many tags (that's the point);
///   `m..063`'s `(game, tag_slug, illustration_id)` still enforces pair uniqueness.
/// - `m..063`'s index is deliberately left alone. Reordering *it* to serve both reads
///   would avoid a second B-tree entirely, but it is the index the shipped `art:` search
///   probe rides, and re-ordering measurably slowed that probe in testing — not a trade
///   worth making to save write time on a once-daily background rebuild.
/// - Plain (non-`CONCURRENTLY`) `CREATE INDEX`, and `up()` issues `SET LOCAL
///   statement_timeout = 0` on Postgres first (mirrors `m..050`/`m..031`): the whole
///   pending batch runs in one transaction, so a server/role-default `statement_timeout`
///   killing a slow build over this large table would roll the entire batch back and
///   fail boot.
const ILLUSTRATION_INDEX: &str = "idx_card_art_tags_game_illustration";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction for the whole batch, so a slow build must not hit a
        // server/role-default statement_timeout and roll everything back.
        if manager.get_database_backend() == DatabaseBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared("SET LOCAL statement_timeout = 0")
                .await?;
        }

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name(ILLUSTRATION_INDEX)
                    .table(CardArtTags::Table)
                    .col(CardArtTags::Game)
                    .col(CardArtTags::IllustrationId)
                    // Covering: without it SQLite ignores this index entirely (see above).
                    .col(CardArtTags::TagSlug)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(ILLUSTRATION_INDEX)
                    .table(CardArtTags::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardArtTags {
    Table,
    Game,
    TagSlug,
    IllustrationId,
}
