use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionItems::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CollectionItems::Game).string().not_null())
                    .col(
                        ColumnDef::new(CollectionItems::CardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionItems::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CollectionItems::FoilQuantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CollectionItems::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CollectionItems::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their whole collection.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_items_user_id")
                            .from(CollectionItems::Table, CollectionItems::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One holding per (user, game, card): an add for a card already owned upserts
        // onto this row rather than inserting a duplicate.
        manager
            .create_index(
                Index::create()
                    .name("idx_collection_items_user_game_card")
                    .table(CollectionItems::Table)
                    .col(CollectionItems::UserId)
                    .col(CollectionItems::Game)
                    .col(CollectionItems::CardId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollectionItems::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionItems {
    Table,
    Id,
    UserId,
    Game,
    CardId,
    Quantity,
    FoilQuantity,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
