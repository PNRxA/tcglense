use sea_orm_migration::prelude::*;

/// Adds `sealed_contents.component`: the display name of the **box component** a membership
/// row was inherited through, or NULL for a product's own (direct) contents.
///
/// The MTGJSON ingest resolves a parent product's nested `sealed` references down to cards
/// and attributes every inherited row to the top-level component it came through (the same
/// name the matching `sealed_components` row stores). That lets the product-cards handler
/// split a product's card list by *source*: cards that arrived through a sub-product that
/// is **not** individually listed in the catalog (a bundle's land pack, a starter kit's
/// half-decks) get their own named display section instead of being silently merged into
/// "Guaranteed cards", and sections whose every card arrived through a **listed**
/// sub-product can be flagged `inherited` (the SPA sends readers to the sub-product's own
/// page rather than duplicating its pool).
///
/// The unique key gains the new column: a card can now legitimately appear twice for one
/// product at the same `(membership, foil)` — once per source component — and the ingest's
/// `ON CONFLICT` target must keep naming a real unique index on both backends. The table is
/// rebuilt wholesale each sync, so existing rows just carry NULL until the next rebuild
/// (which the ingest's bumped `DERIVATION_VERSION` forces).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SealedContents::Table)
                    .add_column(ColumnDef::new(SealedContents::Component).string().null())
                    .to_owned(),
            )
            .await?;

        // Re-key the upsert index to one row per (game, product, card, membership, foil,
        // component) — drop-and-recreate, since neither backend alters an index in place.
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sealed_contents_unique")
                    .table(SealedContents::Table)
                    .to_owned(),
            )
            .await?;
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
                    .col(SealedContents::Component)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The re-keyed index permits rows that differ only in `component` — a card
        // inherited through two box components, or held both directly and via one — and
        // those rows collide under the 5-column key recreated below, so a naive rollback
        // fails on real post-upgrade data. The table is rebuilt wholesale by every sync,
        // so the honest rollback is to clear it and let the next sync repopulate under the
        // old key.
        manager
            .exec_stmt(Query::delete().from_table(SealedContents::Table).to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sealed_contents_unique")
                    .table(SealedContents::Table)
                    .to_owned(),
            )
            .await?;
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
        manager
            .alter_table(
                Table::alter()
                    .table(SealedContents::Table)
                    .drop_column(SealedContents::Component)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SealedContents {
    Table,
    Game,
    ProductId,
    CardId,
    Membership,
    Foil,
    Component,
}
