use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EmailTokens::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(EmailTokens::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(EmailTokens::UserId).integer().not_null())
                    .col(ColumnDef::new(EmailTokens::Purpose).string().not_null())
                    .col(ColumnDef::new(EmailTokens::TokenHash).string().not_null())
                    .col(
                        ColumnDef::new(EmailTokens::ExpiresAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::ConsumedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_tokens_user_id")
                            .from(EmailTokens::Table, EmailTokens::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // SHA-256 hex of the emailed token is unique across all rows.
        manager
            .create_index(
                Index::create()
                    .name("idx_email_tokens_token_hash")
                    .table(EmailTokens::Table)
                    .col(EmailTokens::TokenHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports the per-user lookups (resend cooldown, housekeeping).
        manager
            .create_index(
                Index::create()
                    .name("idx_email_tokens_user_id")
                    .table(EmailTokens::Table)
                    .col(EmailTokens::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(EmailTokens::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum EmailTokens {
    Table,
    Id,
    UserId,
    Purpose,
    TokenHash,
    ExpiresAt,
    ConsumedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
