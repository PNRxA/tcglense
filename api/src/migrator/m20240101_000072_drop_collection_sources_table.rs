use sea_orm_migration::prelude::*;

/// Drop `collection_sources` — the saved external-collection link table.
///
/// The saved link existed only to be re-synced on demand (`POST
/// /api/collection/{game}/sync`), and that surface — along with the incremental "smart"
/// sync it could opt into — has been removed. With nothing left to re-sync, the table
/// stores nothing anyone can read or act on, so it goes rather than lingering as dead
/// schema. Importing a collection from a provider URL is unaffected: it's a one-off that
/// never touched this table except to stamp `last_synced_at`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollectionSources::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    /// Recreate the table exactly as `m..011` + `m..012` left it (columns, the cascade to
    /// `users`, the unique `(user, game)` index and the `smart` flag), so a rollback lands
    /// on the schema the pre-removal code expects. The *rows* are gone for good — the drop
    /// above is what removed them.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionSources::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionSources::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CollectionSources::Game).string().not_null())
                    .col(
                        ColumnDef::new(CollectionSources::Provider)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::ExternalId)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::LastSyncedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::Smart)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CollectionSources::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_sources_user_id")
                            .from(CollectionSources::Table, CollectionSources::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_sources_user_game")
                    .table(CollectionSources::Table)
                    .col(CollectionSources::UserId)
                    .col(CollectionSources::Game)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionSources {
    Table,
    Id,
    UserId,
    Game,
    Provider,
    ExternalId,
    LastSyncedAt,
    Smart,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
