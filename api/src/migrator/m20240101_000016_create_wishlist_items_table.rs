use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WishlistItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WishlistItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WishlistItems::UserId).integer().not_null())
                    .col(ColumnDef::new(WishlistItems::Game).string().not_null())
                    .col(ColumnDef::new(WishlistItems::CardId).integer().not_null())
                    .col(
                        ColumnDef::new(WishlistItems::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WishlistItems::FoilQuantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WishlistItems::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(WishlistItems::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their whole wish list.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_wishlist_items_user_id")
                            .from(WishlistItems::Table, WishlistItems::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One wish-list row per (user, game, card): an add for a card already wanted
        // upserts onto this row rather than inserting a duplicate.
        manager
            .create_index(
                Index::create()
                    .name("idx_wishlist_items_user_game_card")
                    .table(WishlistItems::Table)
                    .col(WishlistItems::UserId)
                    .col(WishlistItems::Game)
                    .col(WishlistItems::CardId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(WishlistItems::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum WishlistItems {
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
