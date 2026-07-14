use sea_orm::entity::prelude::*;

/// SeaORM entity for the `deck_cards` table.
///
/// One row per `(deck, card, section)` recording how many copies of a card a deck
/// holds in one of its sections — `quantity` regular plus `foil_quantity` foil, the
/// same two-count shape as a collection / wish-list holding (so it implements
/// [`HoldingCounts`](crate::handlers::shared::holdings) and reuses the shared
/// valuation / summary / sort machinery). A card may appear in more than one section
/// (e.g. the mainboard and a "Maybeboard"), so the unique key is
/// `(deck_id, card_id, section_id)`; both counts zero deletes the row.
///
/// `card_id` references the internal `cards.id` (not the provider external id), so a
/// deck card survives a catalog re-import — matching `collection_items`. `deck_id` is
/// denormalised alongside `section_id` so a deck's whole card list is one indexed
/// filter (no join through `deck_sections`). Deleting a deck or one of its sections
/// cascades the row away.
///
/// `Eq` is derivable — every column is an integer or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "deck_cards")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning deck (`decks.id`).
    pub deck_id: i32,
    /// The section (`deck_sections.id`) this card sits in.
    pub section_id: i32,
    /// `cards.id` this row is for (internal integer id).
    pub card_id: i32,
    /// Regular (non-foil) copies in the deck.
    pub quantity: i32,
    /// Foil copies in the deck.
    pub foil_quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The card this row is for (`card_id` -> `cards.id`). Lets deck lists join
    /// `cards` (`find_also_related`) so they reuse the catalog's Scryfall-syntax
    /// search and card sorts, exactly as the collection does.
    #[sea_orm(
        belongs_to = "super::card::Entity",
        from = "Column::CardId",
        to = "super::card::Column::Id"
    )]
    Card,
}

impl Related<super::card::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Card.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
