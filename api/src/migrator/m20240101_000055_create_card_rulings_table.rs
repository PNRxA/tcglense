use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CardRulings::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CardRulings::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CardRulings::Game).string().not_null())
                    // Gameplay identity the ruling applies to (Scryfall `oracle_id`); the
                    // join key onto `cards.oracle_id`. Not a foreign key — rulings are a
                    // separately-refreshed, wholesale-rebuilt table and a card row may be
                    // absent momentarily during a card re-import.
                    .col(ColumnDef::new(CardRulings::OracleId).string().not_null())
                    .col(ColumnDef::new(CardRulings::Source).string().not_null())
                    .col(ColumnDef::new(CardRulings::PublishedAt).string().not_null())
                    // Ruling text can be long, so `text` rather than `string` (VARCHAR).
                    .col(ColumnDef::new(CardRulings::Comment).text().not_null())
                    .col(
                        ColumnDef::new(CardRulings::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // The one read shape: a card's rulings by `(game, oracle_id)`.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_rulings_game_oracle_id")
                    .table(CardRulings::Table)
                    .col(CardRulings::Game)
                    .col(CardRulings::OracleId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CardRulings::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardRulings {
    Table,
    Id,
    Game,
    OracleId,
    Source,
    PublishedAt,
    Comment,
    CreatedAt,
}
