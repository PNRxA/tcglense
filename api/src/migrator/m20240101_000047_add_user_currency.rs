use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ISO 4217 display preference (issue #412). USD preserves the existing display for
        // every account; the handler restricts writes to the app's supported currency set.
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::Currency)
                            .string_len(3)
                            .not_null()
                            .default("USD"),
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
                    .drop_column(Users::Currency)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Currency,
}
