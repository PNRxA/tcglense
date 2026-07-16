use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionProductItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionProductItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::Game)
                            .string()
                            .not_null(),
                    )
                    // Deliberately orphan-tolerant like collection_items.card_id: a catalog
                    // re-import may remove a product, and reads skip a missing joined row.
                    .col(
                        ColumnDef::new(CollectionProductItems::ProductId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::FoilQuantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CollectionProductItems::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_product_items_user_id")
                            .from(
                                CollectionProductItems::Table,
                                CollectionProductItems::UserId,
                            )
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_product_items_user_game_product")
                    .table(CollectionProductItems::Table)
                    .col(CollectionProductItems::UserId)
                    .col(CollectionProductItems::Game)
                    .col(CollectionProductItems::ProductId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_product_items_user_game_updated_at_id")
                    .table(CollectionProductItems::Table)
                    .col(CollectionProductItems::UserId)
                    .col(CollectionProductItems::Game)
                    .col(CollectionProductItems::UpdatedAt)
                    .col(CollectionProductItems::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollectionProductItems::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionProductItems {
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
