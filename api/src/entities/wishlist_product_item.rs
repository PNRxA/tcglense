use sea_orm::entity::prelude::*;

/// SeaORM entity for the `wishlist_product_items` table.
///
/// One row per `(user, game, product)` recording how many copies of a sealed product a
/// signed-in user wants to buy — `quantity` regular plus `foil_quantity` foil. A product
/// the user does not want has no row (the row is deleted once both counts reach zero), so
/// the table holds only wish-listed products.
///
/// The sealed-product twin of [`super::wishlist_item`]: the wish list holds both cards and
/// sealed products, in separate tables, so a product can be wanted independently of any
/// card. Wishlist-only — the collection deliberately has **no** sealed-product holdings
/// (issue #364). `product_id` references `products.id` — the internal integer id, not the
/// provider's external id — so a wish-list row survives a catalog re-import (the daily
/// TCGCSV sweep is upsert-only); `game` is denormalised from the product so a user's
/// per-game sealed wish list can be listed and counted without joining `products`.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "wishlist_product_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `products.id` this wish-list row is for (internal integer id).
    pub product_id: i32,
    /// Regular (non-foil) copies wanted.
    pub quantity: i32,
    /// Foil copies wanted.
    pub foil_quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The product this wish-list row is for (`product_id` -> `products.id`). Lets the
    /// wanted-products list join `products` (`find_also_related`) so each row can carry
    /// the full product payload.
    #[sea_orm(
        belongs_to = "super::product::Entity",
        from = "Column::ProductId",
        to = "super::product::Column::Id"
    )]
    Product,
}

impl Related<super::product::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Product.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
