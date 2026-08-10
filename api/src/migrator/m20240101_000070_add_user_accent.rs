use sea_orm::DbErr;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // UI accent-colour preset slug (the design system's brand hue). The default
        // preserves the standard look for every existing account; the handler restricts
        // writes to `crate::accent::SUPPORTED_ACCENTS`.
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::Accent)
                            .string_len(16)
                            .not_null()
                            .default(crate::accent::DEFAULT_ACCENT),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::Accent)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Accent,
}
