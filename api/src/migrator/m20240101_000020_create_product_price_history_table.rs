use sea_orm_migration::prelude::*;

/// Creates the `product_price_history` table: one daily market-price snapshot per
/// sealed product, the sealed-product mirror of `card_price_history`. USD-only (no
/// eur/tix — TCGCSV carries neither). `product_id` FKs `products` with ON DELETE
/// CASCADE so a product's history is cleaned up if the product row is ever removed.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProductPriceHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProductPriceHistory::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ProductPriceHistory::Game)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProductPriceHistory::ProductId)
                            .integer()
                            .not_null(),
                    )
                    // Stored as a "YYYY-MM-DD" string (mirroring `products.released_at`).
                    .col(
                        ColumnDef::new(ProductPriceHistory::AsOfDate)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProductPriceHistory::PriceUsd)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ProductPriceHistory::PriceUsdFoil)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ProductPriceHistory::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_product_price_history_product")
                            .from(
                                ProductPriceHistory::Table,
                                ProductPriceHistory::ProductId,
                            )
                            .to(Products::Table, Products::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One price row per (game, product, day): same-day re-runs upsert on this key
        // rather than inserting duplicates.
        manager
            .create_index(
                Index::create()
                    .name("idx_product_price_history_game_product_date")
                    .table(ProductPriceHistory::Table)
                    .col(ProductPriceHistory::Game)
                    .col(ProductPriceHistory::ProductId)
                    .col(ProductPriceHistory::AsOfDate)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ProductPriceHistory::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ProductPriceHistory {
    Table,
    Id,
    Game,
    ProductId,
    AsOfDate,
    PriceUsd,
    PriceUsdFoil,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Id,
}
