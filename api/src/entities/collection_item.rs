use sea_orm::entity::prelude::*;

/// SeaORM entity for the `collection_items` table.
///
/// One row per `(user, game, card)` recording how many copies of a card a signed-in
/// user owns — `quantity` regular plus `foil_quantity` foil. A card the user does
/// not own has no row (the row is deleted once both counts reach zero), so the table
/// holds only owned cards.
///
/// `card_id` references `cards.id` — the internal integer id, not the provider's
/// external id — mirroring how `card_price_history` links to a card, so a collection
/// row survives a catalog re-import (external ids are stable, but the join key stays
/// internal). `game` is denormalised from the card so a user's per-game collection
/// can be listed and counted without joining `cards`.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `cards.id` this holding is for (internal integer id).
    pub card_id: i32,
    /// Regular (non-foil) copies owned.
    pub quantity: i32,
    /// Foil copies owned.
    pub foil_quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
