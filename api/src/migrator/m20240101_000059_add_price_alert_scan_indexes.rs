use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The evaluator (issue #525) keyset-paginates the armed alerts (`WHERE is_active AND
        // id > :after ORDER BY id`), so make the scan a covering seek on `(is_active, id)`
        // rather than a filter over the id PK — this keeps a large inactive/paused population
        // out of the walk. Replaces the plain `is_active` index from m..056.
        manager
            .drop_index(
                Index::drop()
                    .name("idx_price_alerts_active")
                    .table(PriceAlerts::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_price_alerts_active_id")
                    .table(PriceAlerts::Table)
                    .col(PriceAlerts::IsActive)
                    .col(PriceAlerts::Id)
                    .to_owned(),
            )
            .await?;

        // Back the target-side of the evaluator's change-narrowing join (`... = price_alerts
        // .card_id / .product_id`) and any future "which alerts watch this changed target"
        // lookup, so neither seq-scans the alerts table.
        manager
            .create_index(
                Index::create()
                    .name("idx_price_alerts_card_id")
                    .table(PriceAlerts::Table)
                    .col(PriceAlerts::CardId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_price_alerts_product_id")
                    .table(PriceAlerts::Table)
                    .col(PriceAlerts::ProductId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_price_alerts_product_id")
                    .table(PriceAlerts::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_price_alerts_card_id")
                    .table(PriceAlerts::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_price_alerts_active_id")
                    .table(PriceAlerts::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_price_alerts_active")
                    .table(PriceAlerts::Table)
                    .col(PriceAlerts::IsActive)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PriceAlerts {
    Table,
    Id,
    CardId,
    ProductId,
    IsActive,
}
