use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Two per-user opt-ins for *release* notifications, delivered over the same channels as
        // price alerts: a heads-up the day before a Secret Lair drop, and the day before a new
        // regular set. Both default `false` — unlike the per-channel enabled flags (which
        // default on so a saved channel keeps delivering), these are subscriptions a user must
        // deliberately opt into, so an existing settings row stays silent until they do. Two
        // separate ALTERs: SQLite adds one column per statement.
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .add_column(
                        ColumnDef::new(AlertChannels::SldReleaseEnabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .add_column(
                        ColumnDef::new(AlertChannels::SetReleaseEnabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .drop_column(AlertChannels::SldReleaseEnabled)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .drop_column(AlertChannels::SetReleaseEnabled)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertChannels {
    Table,
    SldReleaseEnabled,
    SetReleaseEnabled,
}
