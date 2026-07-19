use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CliAuthCodes::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CliAuthCodes::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CliAuthCodes::UserId).integer().not_null())
                    .col(ColumnDef::new(CliAuthCodes::CodeHash).string().not_null())
                    .col(
                        ColumnDef::new(CliAuthCodes::CodeChallenge)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CliAuthCodes::SessionVersion)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CliAuthCodes::ClientName).string().null())
                    .col(
                        ColumnDef::new(CliAuthCodes::ExpiresAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CliAuthCodes::ConsumedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CliAuthCodes::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_cli_auth_codes_user_id")
                            .from(CliAuthCodes::Table, CliAuthCodes::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // SHA-256 hex of the one-time code is unique across all rows (the lookup key).
        manager
            .create_index(
                Index::create()
                    .name("idx_cli_auth_codes_code_hash")
                    .table(CliAuthCodes::Table)
                    .col(CliAuthCodes::CodeHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Supports the per-user cascade and expiry-prune housekeeping.
        manager
            .create_index(
                Index::create()
                    .name("idx_cli_auth_codes_user_id")
                    .table(CliAuthCodes::Table)
                    .col(CliAuthCodes::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CliAuthCodes::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CliAuthCodes {
    Table,
    Id,
    UserId,
    CodeHash,
    CodeChallenge,
    SessionVersion,
    ClientName,
    ExpiresAt,
    ConsumedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
