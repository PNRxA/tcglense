use sea_orm_migration::prelude::*;

/// Preconstructed ("precon") decks published with a set — Commander decks, Planeswalker
/// decks, Challenger decks, Jumpstart themes, Secret Lair drops, … (issue: precon browser).
///
/// Catalog data, not user data: both tables are rebuilt wholesale by the MTGJSON sealed
/// sync (`crate::mtgjson::precons`), which is why the primary keys are **not** the stable
/// identity — `slug` is. Every read and link addresses a precon by `(game, slug)` so a
/// rebuild that re-mints ids can't break a bookmarked URL.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PreconDecks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PreconDecks::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PreconDecks::Game).string().not_null())
                    // URL identity, unique per game (`turtle-power-tmc`).
                    .col(ColumnDef::new(PreconDecks::Slug).string().not_null())
                    .col(ColumnDef::new(PreconDecks::Name).string().not_null())
                    // The set the deck ships with, lowercased (`tmc`, `sld`).
                    .col(ColumnDef::new(PreconDecks::SetCode).string().not_null())
                    // Upstream's deck category ("Commander Deck", "Secret Lair Drop", …).
                    .col(ColumnDef::new(PreconDecks::DeckType).string().not_null())
                    // ISO date string, as `products.released_at` stores it.
                    .col(ColumnDef::new(PreconDecks::ReleasedAt).string().null())
                    // WUBRG-ordered colour letters (`WUB`), empty for colourless, NULL when
                    // there was nothing to read a colour off. Derived at ingest so the
                    // public list needs no per-row card scan.
                    .col(ColumnDef::new(PreconDecks::ColorIdentity).string().null())
                    // Total copies in the deck proper (mainboard + command zone).
                    .col(
                        ColumnDef::new(PreconDecks::CardCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // Total copies in the sideboard (counted apart, like a deck's maybeboard).
                    .col(
                        ColumnDef::new(PreconDecks::SideboardCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // `cards.id` of the tile's face card (the first commander, else the
                    // first mainboard card). Orphan-tolerant: no FK, reads LEFT-join it.
                    .col(ColumnDef::new(PreconDecks::FaceCardId).integer().null())
                    // `products.id` of the sealed product that ships this deck, when it
                    // resolved. Orphan-tolerant for the same reason.
                    .col(ColumnDef::new(PreconDecks::ProductId).integer().null())
                    .col(
                        ColumnDef::new(PreconDecks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PreconDecks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // The URL identity: every detail read and copy is one lookup on this.
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_decks_game_slug")
                    .table(PreconDecks::Table)
                    .col(PreconDecks::Game)
                    .col(PreconDecks::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // The browse list's default order (newest first) and its two facet filters.
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_decks_game_released")
                    .table(PreconDecks::Table)
                    .col(PreconDecks::Game)
                    .col(PreconDecks::ReleasedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_decks_game_type")
                    .table(PreconDecks::Table)
                    .col(PreconDecks::Game)
                    .col(PreconDecks::DeckType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_decks_game_set")
                    .table(PreconDecks::Table)
                    .col(PreconDecks::Game)
                    .col(PreconDecks::SetCode)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PreconDeckCards::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PreconDeckCards::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PreconDeckCards::PreconDeckId)
                            .integer()
                            .not_null(),
                    )
                    // Internal `cards.id`, like `deck_cards` — so a precon card survives a
                    // catalog re-import. Deliberately NOT foreign-keyed to `cards`
                    // (orphan-tolerant: reads LEFT-join and skip a card whose row is gone).
                    .col(ColumnDef::new(PreconDeckCards::CardId).integer().not_null())
                    // `main` | `commander` | `side` — upstream's board, not a section name.
                    .col(ColumnDef::new(PreconDeckCards::Board).string().not_null())
                    .col(
                        ColumnDef::new(PreconDeckCards::Quantity)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(PreconDeckCards::Foil)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // Upstream's listing order within the board — the order a Secret Lair
                    // drop's cards are meant to be read in.
                    .col(
                        ColumnDef::new(PreconDeckCards::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // Rebuilding a precon removes its cards.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_precon_deck_cards_precon_deck_id")
                            .from(PreconDeckCards::Table, PreconDeckCards::PreconDeckId)
                            .to(PreconDecks::Table, PreconDecks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One row per (deck, board, card, finish) — the ingest aggregates copies into it.
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_deck_cards_unique")
                    .table(PreconDeckCards::Table)
                    .col(PreconDeckCards::PreconDeckId)
                    .col(PreconDeckCards::Board)
                    .col(PreconDeckCards::CardId)
                    .col(PreconDeckCards::Foil)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Fetch a whole precon's cards (every board) as one indexed scan.
        manager
            .create_index(
                Index::create()
                    .name("idx_precon_deck_cards_deck_id")
                    .table(PreconDeckCards::Table)
                    .col(PreconDeckCards::PreconDeckId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PreconDeckCards::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PreconDecks::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PreconDecks {
    Table,
    Id,
    Game,
    Slug,
    Name,
    SetCode,
    DeckType,
    ReleasedAt,
    ColorIdentity,
    CardCount,
    SideboardCount,
    FaceCardId,
    ProductId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PreconDeckCards {
    Table,
    Id,
    PreconDeckId,
    CardId,
    Board,
    Quantity,
    Foil,
    Position,
}
