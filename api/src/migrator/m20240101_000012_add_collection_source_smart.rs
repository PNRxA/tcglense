use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Whether a saved re-sync uses smart (incremental) sync instead of a full mirror.
        // Defaults false so existing saved links keep their current full-mirror behaviour.
        manager
            .alter_table(
                Table::alter()
                    .table(CollectionSources::Table)
                    .add_column(
                        ColumnDef::new(CollectionSources::Smart)
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
                    .table(CollectionSources::Table)
                    .drop_column(CollectionSources::Smart)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionSources {
    Table,
    Smart,
}
