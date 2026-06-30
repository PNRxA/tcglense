use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CardPriceHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CardPriceHistory::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CardPriceHistory::Game).string().not_null())
                    .col(
                        ColumnDef::new(CardPriceHistory::CardId)
                            .integer()
                            .not_null(),
                    )
                    // Stored as a "YYYY-MM-DD" string (mirroring `cards.released_at`),
                    // not a native DATE type.
                    .col(
                        ColumnDef::new(CardPriceHistory::AsOfDate)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CardPriceHistory::PriceUsd).string().null())
                    .col(
                        ColumnDef::new(CardPriceHistory::PriceUsdFoil)
                            .string()
                            .null(),
                    )
                    .col(ColumnDef::new(CardPriceHistory::PriceEur).string().null())
                    .col(ColumnDef::new(CardPriceHistory::PriceTix).string().null())
                    .col(
                        ColumnDef::new(CardPriceHistory::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // One price row per (game, card, day): same-day re-runs upsert on this key
        // rather than inserting duplicates.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_price_history_game_card_date")
                    .table(CardPriceHistory::Table)
                    .col(CardPriceHistory::Game)
                    .col(CardPriceHistory::CardId)
                    .col(CardPriceHistory::AsOfDate)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CardPriceHistory::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardPriceHistory {
    Table,
    Id,
    Game,
    CardId,
    AsOfDate,
    PriceUsd,
    PriceUsdFoil,
    PriceEur,
    PriceTix,
    CreatedAt,
}
