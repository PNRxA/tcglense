use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Per-collection display preferences (issue #381), living on the visibility row the
        // entity was designed to grow — the settings menu on the owner's collection landing
        // hides the value-over-time chart and/or the biggest-movers panel per game. Both
        // default true (shown), so every existing row and every collection with no row yet
        // keeps both sections. Added one column per statement — SQLite's ALTER TABLE takes a
        // single ADD COLUMN at a time.
        manager
            .alter_table(
                Table::alter()
                    .table(CollectionVisibility::Table)
                    .add_column(
                        ColumnDef::new(CollectionVisibility::ShowValueChart)
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
                    .table(CollectionVisibility::Table)
                    .add_column(
                        ColumnDef::new(CollectionVisibility::ShowMovers)
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
                    .table(CollectionVisibility::Table)
                    .drop_column(CollectionVisibility::ShowMovers)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(CollectionVisibility::Table)
                    .drop_column(CollectionVisibility::ShowValueChart)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionVisibility {
    Table,
    ShowValueChart,
    ShowMovers,
}
