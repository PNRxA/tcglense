use sea_orm::entity::prelude::*;

/// SeaORM entity for the `sealed_contents` table: which sealed **products** a card is
/// found in — or can be pulled from — sourced from [MTGJSON](https://mtgjson.com)
/// (see [`crate::mtgjson`]).
///
/// One row per `(game, product, card, membership, foil)`. `product_id` references
/// `products.id` and `card_id` references `cards.id` — internal integer ids, not the
/// providers' external ids, so a row survives a catalog / product re-import (mirroring
/// how [`super::collection_item`] links to `cards.id`). `game` is denormalised so the
/// by-card lookup filters without joining. The whole table is rebuilt on each sync, so
/// stale membership never lingers.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sealed_contents")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `products.id` of the sealed product (internal integer id).
    pub product_id: i32,
    /// `cards.id` the membership is for (internal integer id).
    pub card_id: i32,
    /// How the card relates to the product: `"contains"` (definitely in — decks / fixed
    /// promos / Secret Lair), `"booster"` (can be pulled from a booster sheet), or
    /// `"variable"` (may be in a randomized configuration). See [`Membership`].
    pub membership: String,
    /// Whether this is the foil printing in the product.
    pub foil: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// The three membership buckets a `sealed_contents` row can carry, as stored in the
/// `membership` column. The single source of truth for the string values, shared by the
/// ingest (which writes them) and the handler (which orders/groups by them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    /// The product **definitely** includes the card (precon deck, fixed promo, Secret
    /// Lair). Surfaced as "found in".
    Contains,
    /// The card **can be pulled** from the product's booster packs (a probabilistic
    /// booster sheet). Surfaced as "can be opened from".
    Booster,
    /// The card **may be** in the product (a randomized / either-or configuration).
    /// Surfaced as "may be in".
    Variable,
}

impl Membership {
    /// The stored string value.
    pub fn as_str(self) -> &'static str {
        match self {
            Membership::Contains => "contains",
            Membership::Booster => "booster",
            Membership::Variable => "variable",
        }
    }

    /// Display order: definitely-in first, then boosters, then maybe. Drives both the
    /// handler's ordering and (via the string) the UI grouping.
    pub fn rank(value: &str) -> u8 {
        match value {
            "contains" => 0,
            "booster" => 1,
            "variable" => 2,
            _ => 3,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
