use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Promote "maybeboard" from a section *name* to a section *property* (issue #570).
        // A flagged section holds cards the deck is only considering, so they're excluded
        // from the deck's card count, value summary, format legality, analytics, and the
        // cross-deck "cards needed" shopping list — none of which a name match could do
        // safely (renaming "Maybeboard" to "Maybe / cuts" would silently fold it back into
        // the deck). Defaults `false`: every existing section stays part of its deck until
        // the backfill below, or the owner, says otherwise.
        manager
            .alter_table(
                Table::alter()
                    .table(DeckSections::Table)
                    .add_column(
                        ColumnDef::new(DeckSections::IsMaybeboard)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        // Backfill the sections that were already acting as maybeboards by convention —
        // the seeded `Maybeboard`, the name the deck importer maps provider maybeboard /
        // "considering" boards onto, and the spellings the analytics panel already excluded
        // from draw odds. Case-insensitive so a hand-typed "maybeboard" is caught too. This
        // keeps existing decks' counts consistent with what their owners already saw in the
        // draw-odds selector; anything else remains part of the deck.
        manager
            .get_connection()
            .execute_unprepared(
                r#"UPDATE "deck_sections" SET "is_maybeboard" = TRUE
                   WHERE LOWER("name") IN ('maybeboard', 'maybe board', 'considering')"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DeckSections::Table)
                    .drop_column(DeckSections::IsMaybeboard)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DeckSections {
    Table,
    IsMaybeboard,
}
