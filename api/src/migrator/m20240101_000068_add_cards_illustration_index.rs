use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// Index `cards` on `(game, illustration_id)` — the *card* side of the `art:`/`arttag:`/
/// `atag:` search filter's semi-join. `m..066` indexed the mapping table for the card
/// page's "which tags are on this artwork" read; this is the opposite direction, and the
/// one the catalog **search** rides.
///
/// The filter compiles to a correlated `EXISTS (… card_art_tags.illustration_id =
/// cards.illustration_id)` (`scryfall::search::compile::tags`), and `cards` carried no
/// index on `illustration_id` at all — nine `idx_cards_*` indexes, none of them this one.
/// So Postgres had exactly one way to answer an art-tag search: drive from `cards`.
/// Combined with the listing's `ORDER BY name, set_code, collector_number_int, id` +
/// `LIMIT`, the planner walked `idx_cards_game_name` in sort order probing the mapping
/// per row, betting it would fill the page early. For any tag that isn't ubiquitous that
/// bet loses, and it walks the whole game partition.
///
/// Measured on Postgres 16, 800k `cards` / 1.2M `card_art_tags` (warm cache; a
/// production-sized cold catalog is far worse — the page fetch that prompted this took
/// **86 s** in production):
///
/// | query                     | before                          | after      |
/// |---------------------------|---------------------------------|------------|
/// | page fetch, selective tag | 1 282 ms (walks `game_name`)    | **1.0 ms** |
/// | count half, selective tag | 93 ms (parallel seq scan)       | **0.4 ms** |
/// | page fetch, broad tag     | 6 ms                            | 6 ms       |
/// | count half, broad tag     | 90 ms                           | 90 ms      |
///
/// With the index the planner drives from the tag instead: index-only scan of `m..063`'s
/// `(game, tag_slug, illustration_id)` for the matching artworks, then a lookup per
/// artwork here. A broad tag keeps its old plan (walking the sorted index *does* fill the
/// page early when most cards match), so this only adds a path — it removes none.
///
/// Notes:
/// - **Two columns, not three.** Unlike `m..066` this need not cover its query: the
///   semi-join's inner side is `cards`' own row, so the index is a lookup, not a
///   projection. SQLite's planner (no `ANALYZE`, so every index gets the same row
///   estimate) still prefers driving from the ordering index there; SQLite instances are
///   self-hosts serving one household, where the whole partition fits in page cache and
///   the query is milliseconds either way. This migration is for the Postgres arm.
/// - Not unique: many printings share one artwork — that's the point of tagging by
///   `illustration_id`.
/// - Plain (non-`CONCURRENTLY`) `CREATE INDEX`, with `SET LOCAL statement_timeout = 0` on
///   Postgres first, exactly as `m..066`/`m..050`/`m..031` do: the pending batch runs in
///   one transaction on boot, so a server/role-default `statement_timeout` killing the
///   build would roll the whole batch back and fail startup. The build measured 726 ms
///   for an 11 MB index over 800k rows, behind the startup gate.
const INDEX: &str = "idx_cards_game_illustration";

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
                    .name(INDEX)
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::IllustrationId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name(INDEX).table(Cards::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    IllustrationId,
}
