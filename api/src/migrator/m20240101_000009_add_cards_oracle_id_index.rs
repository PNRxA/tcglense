use sea_orm_migration::prelude::*;

/// Adds a composite index on `cards (game, oracle_id)` so the card-detail "other
/// printings" lookup (`handlers::catalog::prints_query`, which filters on
/// `game` + `oracle_id`) is an index seek rather than a full scan of the cards
/// table — mirroring the existing `idx_cards_game_set_code` / `idx_cards_game_name`
/// access-pattern indexes. Real catalogs are single-game, so a `game`-only index
/// would prune nothing.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_oracle_id")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::OracleId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_oracle_id")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    OracleId,
}
