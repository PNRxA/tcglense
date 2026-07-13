use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WishlistProductItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WishlistProductItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::Game)
                            .string()
                            .not_null(),
                    )
                    // No FK on product_id (deliberate): a row is orphan-tolerant, mirroring
                    // wishlist_items.card_id — orphans are skipped at the LEFT join.
                    .col(
                        ColumnDef::new(WishlistProductItems::ProductId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::FoilQuantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(WishlistProductItems::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their whole sealed-product wish list.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_wishlist_product_items_user_id")
                            .from(WishlistProductItems::Table, WishlistProductItems::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One wish-list row per (user, game, product): an add for a product already
        // wanted upserts onto this row rather than inserting a duplicate — the conflict
        // target of the set-entry upsert.
        manager
            .create_index(
                Index::create()
                    .name("idx_wishlist_product_items_user_game_product")
                    .table(WishlistProductItems::Table)
                    .col(WishlistProductItems::UserId)
                    .col(WishlistProductItems::Game)
                    .col(WishlistProductItems::ProductId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Backs the wanted-products list's recency sort: filter by (user_id, game),
        // order by (updated_at, id) — the same shape as the wishlist_items twin index.
        manager
            .create_index(
                Index::create()
                    .name("idx_wishlist_product_items_user_game_updated_at_id")
                    .table(WishlistProductItems::Table)
                    .col(WishlistProductItems::UserId)
                    .col(WishlistProductItems::Game)
                    .col(WishlistProductItems::UpdatedAt)
                    .col(WishlistProductItems::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(WishlistProductItems::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum WishlistProductItems {
    Table,
    Id,
    UserId,
    Game,
    ProductId,
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
