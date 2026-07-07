use sea_orm_migration::prelude::*;

/// Creates the `sealed_components` table: the **structural composition** of a sealed
/// product — "what's in the box" — derived from [MTGJSON](https://mtgjson.com)'s
/// sealed-product `contents` (see `crate::mtgjson`).
///
/// Where `sealed_contents` flattens a product to the individual *cards* it can yield, this
/// table keeps the product's *packaging*: one row per component (a nested pack/box, a
/// precon deck, a fixed promo card, or a physical extra), each with a `quantity` and a
/// `position` for display order. A `sealed` component that resolves to a catalog product
/// carries a `child_product_id` (so the SPA links "the products this box contains"); a
/// `card` component that resolves carries a `child_card_id`.
///
/// `product_id` (the parent) and the `child_*` links are the **internal** integer ids
/// (like `sealed_contents`), resolved at ingest time so a row survives a catalog / product
/// re-import. Deleting the parent product cascades its component rows away; deleting a
/// *linked* child nulls the link (the component stays, losing only its hyperlink). The
/// table is rebuilt wholesale per game on each sync, so stale composition never lingers.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SealedComponents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SealedComponents::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SealedComponents::Game).string().not_null())
                    .col(
                        ColumnDef::new(SealedComponents::ProductId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SealedComponents::Position)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(SealedComponents::Kind).string().not_null())
                    .col(ColumnDef::new(SealedComponents::Name).string().not_null())
                    .col(
                        ColumnDef::new(SealedComponents::Quantity)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // Nullable: only a `sealed` component that resolved to a catalog product.
                    .col(ColumnDef::new(SealedComponents::ChildProductId).integer())
                    // Nullable: only a `card` component that resolved to a catalog card.
                    .col(ColumnDef::new(SealedComponents::ChildCardId).integer())
                    .col(
                        ColumnDef::new(SealedComponents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SealedComponents::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Removing the parent product drops its component rows.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_components_product_id")
                            .from(SealedComponents::Table, SealedComponents::ProductId)
                            .to(Products::Table, Products::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    // Removing a *linked* sub-product nulls the link but keeps the line item.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_components_child_product_id")
                            .from(SealedComponents::Table, SealedComponents::ChildProductId)
                            .to(Products::Table, Products::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    // Removing a *linked* promo card nulls the link but keeps the line item.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_components_child_card_id")
                            .from(SealedComponents::Table, SealedComponents::ChildCardId)
                            .to(Cards::Table, Cards::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // One row per (game, product, position); the sync rebuilds wholesale. Its
        // `(game, product_id, …)` left-prefix serves the "what's in this product" read,
        // ordered by the trailing `position`.
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_components_unique")
                    .table(SealedComponents::Table)
                    .col(SealedComponents::Game)
                    .col(SealedComponents::ProductId)
                    .col(SealedComponents::Position)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SealedComponents::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SealedComponents {
    Table,
    Id,
    Game,
    ProductId,
    Position,
    Kind,
    Name,
    Quantity,
    ChildProductId,
    ChildCardId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Id,
}
