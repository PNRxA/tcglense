use sea_orm::entity::prelude::*;

/// SeaORM entity for the `products` table: sealed (non-single) TCGplayer products
/// sourced from [TCGCSV](https://tcgcsv.com) — booster boxes, bundles, Commander
/// decks, and so on. Generic across games via the `game` discriminator (MTG first,
/// TCGplayer category 1). One row per TCGplayer `productId` (stored in `external_id`
/// as a string, mirroring how `cards` store their provider id).
///
/// Only *sealed* products are stored — a TCGplayer product with a `Rarity` or
/// `Number` entry in its `extendedData` is a single card and is filtered out during
/// ingest (see [`crate::tcgcsv::classify`]).
///
/// `Eq` is derivable — every column is an integer, bool, or string (prices are kept
/// as the decimal strings the provider sends, never `f64`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "products")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// TCGplayer product id, unique within a game (stored as a string like the
    /// card `external_id`; it is the join key onto TCGCSV's `productId`).
    pub external_id: String,
    pub name: String,
    /// TCGplayer's normalised name, when present.
    pub clean_name: Option<String>,
    /// The product's TCGplayer group abbreviation, lowercased, so products join to
    /// `card_sets` the same way cards do. May not resolve to a `card_sets` row (a
    /// group with no matching set); responses fall back gracefully.
    pub set_code: String,
    /// Derived product category (e.g. `"collector_display"`, `"bundle"`), classified
    /// from the name by [`crate::tcgcsv::classify::classify_product_type`].
    pub product_type: String,
    /// The tcgplayer.com product page URL (stored for a future buy-links feature).
    pub url: Option<String>,
    /// The provider's product image URL, when present.
    pub image_url: Option<String>,
    pub price_usd: Option<String>,
    pub price_usd_foil: Option<String>,
    /// Manufacturer's suggested retail price (USD), as a decimal string like the market
    /// prices above. No upstream feed carries sealed-product MSRP (TCGCSV/MTGJSON both
    /// omit it), so this is populated from the curated, committed
    /// [`crate::tcgcsv::msrp`] map (keyed by TCGplayer product id) during ingest — `None`
    /// for any product not listed there.
    pub msrp: Option<String>,
    /// Release date as an ISO `YYYY-MM-DD` string (from the group's `publishedOn`),
    /// or `None` when the provider gives none.
    pub released_at: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
