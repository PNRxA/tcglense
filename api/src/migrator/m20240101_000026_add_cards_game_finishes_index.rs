use sea_orm_migration::prelude::*;

/// Adds a composite `(game, finishes)` index on `cards`.
///
/// The foil-variant consolidation (`collection_import::consolidate::load_foil_variant_pairs`,
/// run before every collection import) resolves the purely-foil `…★` star cards with
/// `WHERE game = ? AND finishes = 'foil' AND collector_number LIKE '%★'`. The leading-`★`
/// `LIKE` is unindexable, so without this index Postgres full-scanned the whole (wide) `cards`
/// table on every import; on the weak prod instance that was seconds. This index serves the two
/// equality predicates so the planner scans only the small `finishes = 'foil'` subset and
/// applies the trailing-`★` match as a residual filter over those few rows.
///
/// Pure query-builder, so it renders identically on SQLite and Postgres (no `db::Dialect`
/// needed). `finishes` is nullable — `finishes = 'foil'` excludes NULLs and a b-tree indexes
/// NULL entries fine.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_cards_game_finishes")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::Finishes)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_finishes")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    Finishes,
}
