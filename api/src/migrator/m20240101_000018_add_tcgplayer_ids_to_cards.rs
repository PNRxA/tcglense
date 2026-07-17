use sea_orm_migration::prelude::*;

/// Adds the TCGplayer product ids to `cards`: `tcgplayer_id` (the regular/foil
/// printing) and `tcgplayer_etched_id` (the etched printing, when distinct). Both
/// nullable (default NULL) so the ADD COLUMNs are valid on SQLite and the next card
/// sync backfills them from Scryfall. They give the historic price backfill
/// (`crate::tcgcsv`) its join key onto TCGplayer's `productId`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite allows only one ALTER option per statement, so add each column
        // in its own `ALTER TABLE`.
        for column in [
            ColumnDef::new(Cards::TcgplayerId)
                .integer()
                .null()
                .to_owned(),
            ColumnDef::new(Cards::TcgplayerEtchedId)
                .integer()
                .null()
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }

        // The backfill builds an in-memory tcgplayer_id -> card_id map by scanning
        // every card with a non-null id for the game; this partial-friendly index
        // keeps that scan cheap.
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_tcgplayer_id")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::TcgplayerId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_tcgplayer_id")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await?;
        for column in [Cards::TcgplayerId, Cards::TcgplayerEtchedId] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    TcgplayerId,
    TcgplayerEtchedId,
}
