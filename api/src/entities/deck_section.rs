use sea_orm::entity::prelude::*;

/// SeaORM entity for the `deck_sections` table.
///
/// One row per section (category) within a deck (issue #363) — Archidekt-style
/// buckets like "Commander" / "Lands" / "Ramp" / "Removal". A deck is seeded with a
/// default set on creation; the user can add custom sections, rename them, reorder
/// them (`position`), and move cards between them. Each `deck_cards` row points at
/// exactly one section (`section_id`), and a section name is unique per deck.
/// Deleting a deck cascades its sections away.
///
/// `is_maybeboard` (issue #570) marks a section as **outside the deck proper**: cards
/// the owner is only considering. They still live in the deck and are edited exactly
/// like any other section's, but every "what is this deck" reader skips them — the
/// list's `card_count`, the detail `summary`, format legality, the analytics panel, and
/// the cross-deck "cards needed" list. It's a column rather than a name match so
/// renaming the section (or having several) keeps working.
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
    /// Whether this section sits outside the deck proper (a "maybeboard").
    pub is_maybeboard: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
