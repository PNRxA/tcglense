use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PriceAlerts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PriceAlerts::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PriceAlerts::UserId).integer().not_null())
                    .col(ColumnDef::new(PriceAlerts::Game).string().not_null())
                    .col(ColumnDef::new(PriceAlerts::TargetKind).string().not_null())
                    // Exactly one of card_id / product_id is set (per target_kind).
                    // Deliberately orphan-tolerant like collection_items.card_id: a
                    // catalog re-import may remove the target, and evaluation/reads skip
                    // a missing joined row rather than dangling on an FK.
                    .col(ColumnDef::new(PriceAlerts::CardId).integer().null())
                    .col(ColumnDef::new(PriceAlerts::ProductId).integer().null())
                    .col(ColumnDef::new(PriceAlerts::Finish).string().not_null())
                    .col(ColumnDef::new(PriceAlerts::Direction).string().not_null())
                    .col(ColumnDef::new(PriceAlerts::Threshold).string().not_null())
                    .col(
                        ColumnDef::new(PriceAlerts::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(PriceAlerts::Triggered)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(PriceAlerts::LastTriggeredAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new(PriceAlerts::LastPrice).string().null())
                    .col(
                        ColumnDef::new(PriceAlerts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PriceAlerts::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their alerts.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_price_alerts_user_id")
                            .from(PriceAlerts::Table, PriceAlerts::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // The alert-list ordering key: newest-updated first per user, id tiebreak.
        manager
            .create_index(
                Index::create()
                    .name("idx_price_alerts_user_updated_at_id")
                    .table(PriceAlerts::Table)
                    .col(PriceAlerts::UserId)
                    .col(PriceAlerts::UpdatedAt)
                    .col(PriceAlerts::Id)
                    .to_owned(),
            )
            .await?;

        // Backs the evaluator's per-tick scan of every armed alert (`WHERE is_active`).
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PriceAlerts::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PriceAlerts {
    Table,
    Id,
    UserId,
    Game,
    TargetKind,
    CardId,
    ProductId,
    Finish,
    Direction,
    Threshold,
    IsActive,
    Triggered,
    LastTriggeredAt,
    LastPrice,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
