use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CardSets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CardSets::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CardSets::Game).string().not_null())
                    .col(ColumnDef::new(CardSets::Code).string().not_null())
                    .col(ColumnDef::new(CardSets::Name).string().not_null())
                    .col(ColumnDef::new(CardSets::SetType).string().null())
                    .col(ColumnDef::new(CardSets::ReleasedAt).string().null())
                    .col(
                        ColumnDef::new(CardSets::CardCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CardSets::Digital)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(CardSets::IconSvgUri).string().null())
                    .col(ColumnDef::new(CardSets::ParentSetCode).string().null())
                    .col(ColumnDef::new(CardSets::ExternalId).string().null())
                    .col(
                        ColumnDef::new(CardSets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CardSets::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // A set code is unique within a game (other TCGs can reuse a code).
        manager
            .create_index(
                Index::create()
                    .name("idx_card_sets_game_code")
                    .table(CardSets::Table)
                    .col(CardSets::Game)
                    .col(CardSets::Code)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports listing a game's sets ordered by release date.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_sets_game_released_at")
                    .table(CardSets::Table)
                    .col(CardSets::Game)
                    .col(CardSets::ReleasedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CardSets::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardSets {
    Table,
    Id,
    Game,
    Code,
    Name,
    SetType,
    ReleasedAt,
    CardCount,
    Digital,
    IconSvgUri,
    ParentSetCode,
    ExternalId,
    CreatedAt,
    UpdatedAt,
}
