use sea_orm::entity::prelude::*;

/// SeaORM entity for the `product_price_history` table.
///
/// One row per `(game, product_id, as_of_date)` capturing a sealed product's daily
/// market-price snapshot — the sealed-product mirror of `card_price_history`. Unlike
/// the single, overwritten price snapshot on `products`, these rows accumulate so the
/// API can serve a price-over-time series for charting. Populated daily by
/// [`crate::tcgcsv::price_history::snapshot_prices`] (reading the already-committed
/// `products` rows, so it runs on every sync tick regardless of the version gate) and
/// backfilled from TCGCSV's price archives (see [`crate::tcgcsv::backfill`]).
///
/// TCGCSV is USD-only, so there is no eur/tix column (both would always be NULL).
///
/// `Eq` is derivable — every column is an integer or string (prices are kept as the
/// decimal strings the provider sends, never `f64`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "product_price_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `products.id` this snapshot belongs to.
    pub product_id: i32,
    /// Snapshot date as `"YYYY-MM-DD"` (mirrors how `released_at` is stored).
    pub as_of_date: String,
    pub price_usd: Option<String>,
    pub price_usd_foil: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
