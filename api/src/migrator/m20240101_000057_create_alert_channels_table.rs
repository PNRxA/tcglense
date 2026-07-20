use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AlertChannels::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertChannels::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // One settings row per user (upserted on save).
                    .col(
                        ColumnDef::new(AlertChannels::UserId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::DiscordWebhookUrl)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::TelegramBotToken)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::TelegramChatId)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::EmailEnabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(AlertChannels::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their notification settings.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_channels_user_id")
                            .from(AlertChannels::Table, AlertChannels::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertChannels::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertChannels {
    Table,
    Id,
    UserId,
    DiscordWebhookUrl,
    TelegramBotToken,
    TelegramChatId,
    EmailEnabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
