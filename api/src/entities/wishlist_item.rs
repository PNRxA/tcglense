use sea_orm::entity::prelude::*;

/// SeaORM entity for the `wishlist_items` table.
///
/// One row per `(user, game, card)` recording how many copies of a card a signed-in
/// user wants to buy — `quantity` regular plus `foil_quantity` foil. A card the user
/// does not want has no row (the row is deleted once both counts reach zero), so the
/// table holds only wish-listed cards.
///
/// The exact shape of `collection_items` ([`super::collection_item`]) — the wish list
/// is the collection's "want" twin, so the two share the entity-agnostic handler layer
/// (`handlers::shared::holdings`) — but a separate table, so a card can be owned and
/// wanted independently. `card_id` references `cards.id` — the internal integer id, not
/// the provider's external id — so a wish-list row survives a catalog re-import; `game`
/// is denormalised from the card so a user's per-game wish list can be listed and
/// counted without joining `cards`.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "wishlist_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `cards.id` this wish-list row is for (internal integer id).
    pub card_id: i32,
    /// Regular (non-foil) copies wanted.
    pub quantity: i32,
    /// Foil copies wanted.
    pub foil_quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The card this wish-list row is for (`card_id` -> `cards.id`). Lets the wish-list
    /// list join `cards` (`find_also_related`) so it can be searched and sorted on
    /// card columns, reusing the catalog's Scryfall-syntax search and card sorts.
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
