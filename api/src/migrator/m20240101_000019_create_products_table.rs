use sea_orm_migration::prelude::*;

/// Creates the `products` table: sealed TCGplayer products (booster boxes, bundles,
/// decks, …) sourced from TCGCSV, joined to `card_sets` by a lowercased group
/// abbreviation stored in `set_code`. One row per `(game, external_id)`; the sync
/// upserts on that key.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Products::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Products::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Products::Game).string().not_null())
                    .col(ColumnDef::new(Products::ExternalId).string().not_null())
                    .col(ColumnDef::new(Products::Name).string().not_null())
                    .col(ColumnDef::new(Products::CleanName).string().null())
                    .col(ColumnDef::new(Products::SetCode).string().not_null())
                    .col(ColumnDef::new(Products::ProductType).string().not_null())
                    .col(ColumnDef::new(Products::Url).string().null())
                    .col(ColumnDef::new(Products::ImageUrl).string().null())
                    .col(ColumnDef::new(Products::PriceUsd).string().null())
                    .col(ColumnDef::new(Products::PriceUsdFoil).string().null())
                    .col(ColumnDef::new(Products::ReleasedAt).string().null())
                    .col(
                        ColumnDef::new(Products::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Products::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Provider id is unique within a game; the sync upserts on this key.
        manager
            .create_index(
                Index::create()
                    .name("idx_products_game_external_id")
                    .table(Products::Table)
                    .col(Products::Game)
                    .col(Products::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports browsing / filtering products by set.
        manager
            .create_index(
                Index::create()
                    .name("idx_products_game_set_code")
                    .table(Products::Table)
                    .col(Products::Game)
                    .col(Products::SetCode)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Products::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Id,
    Game,
    ExternalId,
    Name,
    CleanName,
    SetCode,
    ProductType,
    Url,
    ImageUrl,
    PriceUsd,
    PriceUsdFoil,
    ReleasedAt,
    CreatedAt,
    UpdatedAt,
}
