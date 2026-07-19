use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Per-(user, game) wish-list public sharing (issue #493). Reuses the existing
        // `collection_visibility` row — the one place per-(user, game) sharing state lives —
        // so the wish-list toggle and the read gate share the same indexed lookup as the
        // collection's `is_public`. Private for every existing row; a wish list is only
        // exposed once its owner flips this on. The collection's `is_public` is unaffected,
        // so the two surfaces share independently.
        manager
            .alter_table(
                Table::alter()
                    .table(CollectionVisibility::Table)
                    .add_column(
                        ColumnDef::new(CollectionVisibility::WishlistIsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CollectionVisibility::Table)
                    .drop_column(CollectionVisibility::WishlistIsPublic)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CollectionVisibility {
    Table,
    WishlistIsPublic,
}
