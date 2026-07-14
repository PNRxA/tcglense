use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DeckSections::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DeckSections::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DeckSections::DeckId).integer().not_null())
                    .col(ColumnDef::new(DeckSections::Name).string().not_null())
                    .col(
                        ColumnDef::new(DeckSections::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DeckSections::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DeckSections::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a deck removes its sections.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deck_sections_deck_id")
                            .from(DeckSections::Table, DeckSections::DeckId)
                            .to(Decks::Table, Decks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // A section name is unique within a deck (so an add can't create a duplicate).
        manager
            .create_index(
                Index::create()
                    .name("idx_deck_sections_deck_name")
                    .table(DeckSections::Table)
                    .col(DeckSections::DeckId)
                    .col(DeckSections::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Ordered read of a deck's sections (by position).
        manager
            .create_index(
                Index::create()
                    .name("idx_deck_sections_deck_position")
                    .table(DeckSections::Table)
                    .col(DeckSections::DeckId)
                    .col(DeckSections::Position)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeckSections::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DeckSections {
    Table,
    Id,
    DeckId,
    Name,
    Position,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Decks {
    Table,
    Id,
}
