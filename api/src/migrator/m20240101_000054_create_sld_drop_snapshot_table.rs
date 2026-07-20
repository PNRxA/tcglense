use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SldDropSnapshot::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SldDropSnapshot::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // Constant discriminator (e.g. "mtg/sld") that keeps this a singleton and gives
                    // the upsert an ON CONFLICT target — the same (game, dataset)-keyed idiom as
                    // ingest_state, so a second drop-grouped set never needs a schema change.
                    .col(
                        ColumnDef::new(SldDropSnapshot::SnapshotKey)
                            .string()
                            .not_null(),
                    )
                    // The canonical snapshot JSON — hundreds of drops, so TEXT (portable to both
                    // SQLite and Postgres), like cards.oracle_text / card_faces.
                    .col(
                        ColumnDef::new(SldDropSnapshot::SnapshotJson)
                            .text()
                            .not_null(),
                    )
                    // The drop-data content hash (16 hex chars) the mirror ETag is built from —
                    // persisted for observability: which snapshot version is loaded.
                    .col(
                        ColumnDef::new(SldDropSnapshot::ContentVersion)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SldDropSnapshot::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_sld_drop_snapshot_key")
                    .table(SldDropSnapshot::Table)
                    .col(SldDropSnapshot::SnapshotKey)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SldDropSnapshot::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SldDropSnapshot {
    Table,
    Id,
    SnapshotKey,
    SnapshotJson,
    ContentVersion,
    UpdatedAt,
}
