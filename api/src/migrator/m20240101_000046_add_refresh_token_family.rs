use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Adds `family_id` to `refresh_tokens`: the id of the lineage's first token
/// (the login/registration grant), copied to every successor on rotation.
///
/// Reuse detection revokes only the replayed token's family — the OAuth-BCP
/// (RFC 9700 §4.14.2) blast radius — instead of every session the user has on
/// every device. Nullable: pre-migration rows keep `NULL`, for which the burn
/// falls back to the old revoke-everything behavior; those rows age out with
/// the 30-day token expiry.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RefreshTokens::Table)
                    .add_column(ColumnDef::new(RefreshTokens::FamilyId).integer().null())
                    .to_owned(),
            )
            .await?;

        // Supports the family-scoped burn (`WHERE user_id = ? AND family_id = ?`
        // rides idx_refresh_tokens_user_id; this covers direct family lookups).
        manager
            .create_index(
                Index::create()
                    .name("idx_refresh_tokens_family_id")
                    .table(RefreshTokens::Table)
                    .col(RefreshTokens::FamilyId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_refresh_tokens_family_id")
                    .table(RefreshTokens::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(RefreshTokens::Table)
                    .drop_column(RefreshTokens::FamilyId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum RefreshTokens {
    Table,
    FamilyId,
}
