use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ApiKeys::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ApiKeys::UserId).integer().not_null())
                    .col(ColumnDef::new(ApiKeys::TokenHash).string().not_null())
                    .col(ColumnDef::new(ApiKeys::Name).string().not_null())
                    .col(ColumnDef::new(ApiKeys::KeyPrefix).string().not_null())
                    .col(ColumnDef::new(ApiKeys::Scope).string().not_null())
                    .col(
                        ColumnDef::new(ApiKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::LastUsedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::ExpiresAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::RevokedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_api_keys_user_id")
                            .from(ApiKeys::Table, ApiKeys::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // SHA-256 hex of the opaque key is unique across all rows — the seam the
        // auth path resolves a presented key by (a single indexed lookup).
        manager
            .create_index(
                Index::create()
                    .name("idx_api_keys_token_hash")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::TokenHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports the per-user list (the management UI) and the per-user cap check.
        manager
            .create_index(
                Index::create()
                    .name("idx_api_keys_user_id")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ApiKeys::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
    UserId,
    TokenHash,
    Name,
    KeyPrefix,
    Scope,
    CreatedAt,
    LastUsedAt,
    ExpiresAt,
    RevokedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
