use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Cards::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Cards::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Cards::Game).string().not_null())
                    .col(ColumnDef::new(Cards::ExternalId).string().not_null())
                    .col(ColumnDef::new(Cards::OracleId).string().null())
                    .col(ColumnDef::new(Cards::Name).string().not_null())
                    .col(ColumnDef::new(Cards::SetCode).string().not_null())
                    .col(ColumnDef::new(Cards::SetName).string().not_null())
                    .col(ColumnDef::new(Cards::CollectorNumber).string().not_null())
                    .col(ColumnDef::new(Cards::CollectorNumberInt).integer().null())
                    .col(ColumnDef::new(Cards::Rarity).string().null())
                    .col(ColumnDef::new(Cards::Lang).string().not_null())
                    .col(ColumnDef::new(Cards::ReleasedAt).string().null())
                    .col(ColumnDef::new(Cards::ManaCost).string().null())
                    .col(ColumnDef::new(Cards::Cmc).double().null())
                    .col(ColumnDef::new(Cards::TypeLine).string().null())
                    .col(ColumnDef::new(Cards::ColorIdentity).string().null())
                    .col(ColumnDef::new(Cards::Colors).string().null())
                    .col(ColumnDef::new(Cards::Layout).string().null())
                    .col(ColumnDef::new(Cards::ImageSmall).string().null())
                    .col(ColumnDef::new(Cards::ImageNormal).string().null())
                    .col(ColumnDef::new(Cards::ImageLarge).string().null())
                    .col(ColumnDef::new(Cards::ImageArtCrop).string().null())
                    .col(ColumnDef::new(Cards::ImagePng).string().null())
                    .col(ColumnDef::new(Cards::CardFaces).text().null())
                    .col(ColumnDef::new(Cards::PriceUsd).string().null())
                    .col(ColumnDef::new(Cards::PriceUsdFoil).string().null())
                    .col(ColumnDef::new(Cards::PriceEur).string().null())
                    .col(ColumnDef::new(Cards::PriceTix).string().null())
                    .col(
                        ColumnDef::new(Cards::Digital)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Cards::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Cards::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Provider id is unique within a game; the import upserts on this key.
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_external_id")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports browsing the cards of a single set.
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_set_code")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .to_owned(),
            )
            .await?;

        // Supports the "all cards" listing, ordered by name.
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_name")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::Name)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Cards::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Id,
    Game,
    ExternalId,
    OracleId,
    Name,
    SetCode,
    SetName,
    CollectorNumber,
    CollectorNumberInt,
    Rarity,
    Lang,
    ReleasedAt,
    ManaCost,
    Cmc,
    TypeLine,
    ColorIdentity,
    Colors,
    Layout,
    ImageSmall,
    ImageNormal,
    ImageLarge,
    ImageArtCrop,
    ImagePng,
    CardFaces,
    PriceUsd,
    PriceUsdFoil,
    PriceEur,
    PriceTix,
    Digital,
    CreatedAt,
    UpdatedAt,
}
