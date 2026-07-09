use sea_orm_migration::prelude::*;

/// Adds the nullable `msrp` column to `products` — the manufacturer's suggested retail
/// price (USD, decimal string), populated from the curated `tcgcsv::msrp` map during
/// ingest. Nullable with no default (valid on both SQLite and Postgres), matching the
/// existing optional `price_usd` columns; products not in the curated map stay NULL.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Products::Table)
                    .add_column(ColumnDef::new(Products::Msrp).string().null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Products::Table)
                    .drop_column(Products::Msrp)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Msrp,
}
