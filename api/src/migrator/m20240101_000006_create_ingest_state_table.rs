use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(IngestState::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IngestState::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(IngestState::Game).string().not_null())
                    .col(ColumnDef::new(IngestState::Dataset).string().not_null())
                    .col(ColumnDef::new(IngestState::SourceUpdatedAt).string().null())
                    .col(ColumnDef::new(IngestState::Status).string().not_null())
                    .col(ColumnDef::new(IngestState::Detail).string().null())
                    .col(
                        ColumnDef::new(IngestState::SetsImported)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(IngestState::CardsImported)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(IngestState::StartedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(IngestState::FinishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // One bookkeeping row per (game, dataset); the import upserts on this key.
        manager
            .create_index(
                Index::create()
                    .name("idx_ingest_state_game_dataset")
                    .table(IngestState::Table)
                    .col(IngestState::Game)
                    .col(IngestState::Dataset)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(IngestState::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum IngestState {
    Table,
    Id,
    Game,
    Dataset,
    SourceUpdatedAt,
    Status,
    Detail,
    SetsImported,
    CardsImported,
    StartedAt,
    FinishedAt,
}
