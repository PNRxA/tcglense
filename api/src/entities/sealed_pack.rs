use sea_orm::entity::prelude::*;

/// SeaORM entity for the `sealed_packs` table: **which boosters one copy of a sealed
/// product opens, and how many** — the link from a [`super::product`] to a
/// [`super::booster_config`], sourced from MTGJSON's sealed-product `contents.pack`
/// references, with nested `contents.sealed` references flattened at ingest (a booster box
/// listing 36 of a pack product gets one row of quantity 36; a case of six boxes, 216).
/// See [`crate::mtgjson::boosters`].
///
/// One row per `(game, product, configuration)`; a product reaching the same booster
/// through two paths sums into one row. `product_id` is `products.id` and is deliberately
/// **not** foreign-keyed (orphan-tolerant, like [`super::sealed_content`]); `config_id`
/// cascades with its configuration, and the ingest deletes it explicitly anyway (SQLite
/// doesn't enforce foreign keys by default). Rebuilt wholesale on each sync.
///
/// A product with **no** row here has no booster data — its expected value is `null` and it
/// can't be opened. That covers products MTGJSON doesn't describe, products whose packs
/// are a *randomised* choice (`contents.variable`), and anything not on TCGplayer.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sealed_packs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `products.id` of the sealed product (internal integer id).
    pub product_id: i32,
    /// `booster_configs.id` of the booster one copy opens.
    pub config_id: i32,
    /// How many of that booster one copy of the product opens (`>= 1`).
    pub quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
