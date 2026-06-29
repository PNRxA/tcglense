use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Successor pointer used by reuse detection. Nullable (default NULL) so the
        // ADD COLUMN is valid on SQLite and existing rows stay untouched.
        manager
            .alter_table(
                Table::alter()
                    .table(RefreshTokens::Table)
                    .add_column(ColumnDef::new(RefreshTokens::ReplacedById).integer().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RefreshTokens::Table)
                    .drop_column(RefreshTokens::ReplacedById)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum RefreshTokens {
    Table,
    ReplacedById,
}
