use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    // Deleting a user removes their saved collection links.
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

        // One saved link per (user, game): saving a new link upserts onto this row.
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollectionSources::Table).to_owned())
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
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
