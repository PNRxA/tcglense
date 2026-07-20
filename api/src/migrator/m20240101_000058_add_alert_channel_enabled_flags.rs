use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Per-channel on/off, so a user can keep a saved Discord webhook / Telegram bot
        // configured but pause its delivery (email already has `email_enabled`). Default
        // `true`, so an already-configured channel keeps delivering — matching the prior
        // "a saved value = enabled" behaviour. Two separate ALTERs: SQLite adds one column
        // per statement.
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .add_column(
                        ColumnDef::new(AlertChannels::DiscordEnabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .add_column(
                        ColumnDef::new(AlertChannels::TelegramEnabled)
                            .boolean()
                            .not_null()
                            .default(true),
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
                    .drop_column(AlertChannels::DiscordEnabled)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AlertChannels::Table)
                    .drop_column(AlertChannels::TelegramEnabled)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertChannels {
    Table,
    DiscordEnabled,
    TelegramEnabled,
}
