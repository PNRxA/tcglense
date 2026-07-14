use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DeckCards::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DeckCards::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DeckCards::DeckId).integer().not_null())
                    .col(ColumnDef::new(DeckCards::SectionId).integer().not_null())
                    // Internal `cards.id` (not the external id), like `collection_items`,
                    // so a deck card survives a catalog re-import. Deliberately NOT
                    // foreign-keyed to `cards` (orphan-tolerant across re-imports —
                    // the reads LEFT-join and skip a card whose row is gone).
                    .col(ColumnDef::new(DeckCards::CardId).integer().not_null())
                    .col(
                        ColumnDef::new(DeckCards::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DeckCards::FoilQuantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DeckCards::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DeckCards::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a deck removes its cards.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deck_cards_deck_id")
                            .from(DeckCards::Table, DeckCards::DeckId)
                            .to(Decks::Table, Decks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    // Deleting a section removes the cards filed under it.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deck_cards_section_id")
                            .from(DeckCards::Table, DeckCards::SectionId)
                            .to(DeckSections::Table, DeckSections::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One row per (deck, card, section): an add for a card already in that section
        // upserts onto this row. A card MAY appear in several sections (separate rows).
        manager
            .create_index(
                Index::create()
                    .name("idx_deck_cards_deck_card_section")
                    .table(DeckCards::Table)
                    .col(DeckCards::DeckId)
                    .col(DeckCards::CardId)
                    .col(DeckCards::SectionId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Fetch a whole deck's cards (all sections) as one indexed scan.
        manager
            .create_index(
                Index::create()
                    .name("idx_deck_cards_deck_id")
                    .table(DeckCards::Table)
                    .col(DeckCards::DeckId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeckCards::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DeckCards {
    Table,
    Id,
    DeckId,
    SectionId,
    CardId,
    Quantity,
    FoilQuantity,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Decks {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DeckSections {
    Table,
    Id,
}
