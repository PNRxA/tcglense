use sea_orm_migration::prelude::*;

/// Creates the `sealed_contents` table: which **sealed products** a card is found in
/// (or can be pulled from). One row per `(game, product, card, membership, foil)`,
/// derived from [MTGJSON](https://mtgjson.com)'s sealed-product contents + booster
/// sheets (see `crate::mtgjson`).
///
/// `membership` is one of three buckets:
/// - `"contains"` — the product **definitely** includes the card (a precon deck, a
///   fixed promo, a Secret Lair drop's cards): "found in".
/// - `"booster"` — the card **can be pulled** from that product's booster packs (a
///   probabilistic booster sheet): "can be opened from".
/// - `"variable"` — the card **may be** in the product (a randomized/either-or
///   configuration): "may be in".
///
/// Both `product_id` and `card_id` are the **internal** integer ids (not the provider
/// external ids), resolved at ingest time — mirroring how `collection_items.card_id`
/// links to `cards.id` — so a row survives a catalog / product re-import (the external
/// ids are stable, but the join key stays internal). Deleting a product or card
/// cascades its membership rows away. The table is rebuilt wholesale on each sync, so
/// stale contents never linger.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SealedContents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SealedContents::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SealedContents::Game).string().not_null())
                    .col(ColumnDef::new(SealedContents::ProductId).integer().not_null())
                    .col(ColumnDef::new(SealedContents::CardId).integer().not_null())
                    .col(ColumnDef::new(SealedContents::Membership).string().not_null())
                    .col(
                        ColumnDef::new(SealedContents::Foil)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(SealedContents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SealedContents::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Removing a product drops its membership rows.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_contents_product_id")
                            .from(SealedContents::Table, SealedContents::ProductId)
                            .to(Products::Table, Products::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    // Removing a card drops its membership rows.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_contents_card_id")
                            .from(SealedContents::Table, SealedContents::CardId)
                            .to(Cards::Table, Cards::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One row per (game, product, card, membership, foil); the sync upserts on this
        // key. Its `(game, product_id, …)` left-prefix also serves the "cards in this
        // product" lookup.
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_contents_unique")
                    .table(SealedContents::Table)
                    .col(SealedContents::Game)
                    .col(SealedContents::ProductId)
                    .col(SealedContents::CardId)
                    .col(SealedContents::Membership)
                    .col(SealedContents::Foil)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // The feature's hot path: "which sealed products is this card in?".
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_contents_game_card")
                    .table(SealedContents::Table)
                    .col(SealedContents::Game)
                    .col(SealedContents::CardId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SealedContents::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SealedContents {
    Table,
    Id,
    Game,
    ProductId,
    CardId,
    Membership,
    Foil,
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
