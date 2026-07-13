use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionVisibility::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionVisibility::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionVisibility::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionVisibility::Game)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionVisibility::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(CollectionVisibility::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CollectionVisibility::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their visibility rows.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_visibility_user_id")
                            .from(CollectionVisibility::Table, CollectionVisibility::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One visibility row per (user, game): the per-game toggle upserts onto it, and
        // the public read resolves "is this (user, game) public" with a single indexed
        // lookup (then filters is_public).
        manager
            .create_index(
                Index::create()
                    .name("idx_collection_visibility_user_game")
                    .table(CollectionVisibility::Table)
                    .col(CollectionVisibility::UserId)
                    .col(CollectionVisibility::Game)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollectionVisibility::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionVisibility {
    Table,
    Id,
    UserId,
    Game,
    IsPublic,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
