use sea_orm::entity::prelude::*;

/// SeaORM entity for the `deck_sections` table.
///
/// One row per section (category) within a deck (issue #363) — Archidekt-style
/// buckets like "Commander" / "Lands" / "Ramp" / "Removal". A deck is seeded with a
/// default set on creation; the user can add custom sections, rename them, reorder
/// them (`position`), and move cards between them. Each `deck_cards` row points at
/// exactly one section (`section_id`), and a section name is unique per deck.
/// Deleting a deck cascades its sections away.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "deck_sections")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning deck (`decks.id`).
    pub deck_id: i32,
    /// Display name, unique per deck.
    pub name: String,
    /// Sort position within the deck (ascending).
    pub position: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
