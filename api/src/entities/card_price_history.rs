use sea_orm::entity::prelude::*;

/// SeaORM entity for the `card_price_history` table.
///
/// One row per `(game, card_id, as_of_date)` capturing the day's price snapshot for
/// a card. Unlike the single, overwritten price snapshot on `cards`, these rows
/// accumulate so the API can serve a price-over-time series for charting. Populated
/// daily by `scryfall::price_history::snapshot_prices` (which reads the already-committed
/// `cards` rows, so it runs on every sync tick regardless of the version gate).
///
/// `Eq` is derivable — every column is an integer or string (prices are kept as the
/// decimal strings the provider sends, never `f64`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "card_price_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `cards.id` this snapshot belongs to.
    pub card_id: i32,
    /// Snapshot date as `"YYYY-MM-DD"` (mirrors how `released_at` is stored).
    pub as_of_date: String,
    pub price_usd: Option<String>,
    pub price_usd_foil: Option<String>,
    pub price_eur: Option<String>,
    pub price_tix: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
