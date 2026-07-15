use sea_orm_migration::prelude::*;

/// Indexes the reverse sealed-product composition lookup introduced for issue #415.
///
/// The original composition read starts from a parent and is covered by
/// `idx_sealed_components_unique` (`game`, `product_id`, `position`). The reverse
/// `/products/{id}/containers` read starts from the linked child instead, so without this
/// sibling index every booster-pack page would scan all component rows for the game.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_components_game_child_product")
                    .table(SealedComponents::Table)
                    .col(SealedComponents::Game)
                    .col(SealedComponents::ChildProductId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sealed_components_game_child_product")
                    .table(SealedComponents::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SealedComponents {
    Table,
    Game,
    ChildProductId,
}
