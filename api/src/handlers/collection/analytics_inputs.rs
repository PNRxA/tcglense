//! The inputs every collection analytics read starts from — the shared preamble of
//! [`super::value_history`], [`super::price_movements`] and [`super::value_change`], extracted
//! once the third reader would otherwise have copied it (AGENTS.md's rule of three).
//!
//! Each of those reads reconstructs its answer from the daily price snapshots and the user's
//! **current** counts, so each begins the same way: reduce the card and sealed-product
//! holdings to `(item id, regular copies, foil copies)` — never the wide catalog rows — and,
//! for the reads anchored to a capture date, find the newest snapshot across the held items
//! of a kind. Keeping those two steps here means the three surfaces can never disagree about
//! *which* holdings they value or *which* capture is "the latest" for a kind.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use crate::entities::prelude::{
    CardPriceHistory, CollectionItem, CollectionProductItem, ProductPriceHistory,
};
use crate::entities::{
    card_price_history, collection_item, collection_product_item, product_price_history,
};
use crate::error::AppError;

/// How many item ids to bind per `IN (...)` chunk. Kept well under SQLite's 32766
/// bound-parameter cap (each chunk also binds `game` + at most a date or two), so an
/// arbitrarily large collection still fetches in a handful of queries.
pub(super) const PRICE_ID_CHUNK: usize = 10_000;

/// A holding reduced to what an analytics fold needs: the internal item id (the same id the
/// price-history tables key on) and its current counts per finish.
pub(super) struct HoldingRow {
    pub(super) item_id: i32,
    pub(super) quantity: i32,
    pub(super) foil_quantity: i32,
}

impl From<(i32, i32, i32)> for HoldingRow {
    fn from((item_id, quantity, foil_quantity): (i32, i32, i32)) -> Self {
        Self {
            item_id,
            quantity,
            foil_quantity,
        }
    }
}

/// The user's current card holdings in a game, reduced to the three fold columns.
/// Acquisition dates are deliberately not read: every analytics read revalues the current
/// basket, never a reconstruction of what was owned when.
pub(super) async fn load_card_holdings(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<Vec<HoldingRow>, AppError> {
    let rows: Vec<(i32, i32, i32)> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().map(HoldingRow::from).collect())
}

/// The user's current sealed-product holdings in a game, reduced like the cards.
pub(super) async fn load_product_holdings(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<Vec<HoldingRow>, AppError> {
    let rows: Vec<(i32, i32, i32)> = CollectionProductItem::find()
        .select_only()
        .column(collection_product_item::Column::ProductId)
        .column(collection_product_item::Column::Quantity)
        .column(collection_product_item::Column::FoilQuantity)
        .filter(collection_product_item::Column::UserId.eq(user_id))
        .filter(collection_product_item::Column::Game.eq(game))
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().map(HoldingRow::from).collect())
}

/// Which price-history table a reference-date lookup reads.
#[derive(Debug, Clone, Copy)]
pub(super) enum HistoryTable {
    Cards,
    Products,
}

/// The newest captured snapshot date across the held items of one kind — the reference
/// ("as of") date the movers and the daily value change anchor to — or `None` when none of
/// them has any history (or nothing is held). One `MAX(as_of_date)` per id chunk on the
/// history index (the `m…050` descending-latest index serves it); zero-padded ISO dates
/// compare chronologically as plain strings, so the chunk maxima fold with `>`.
pub(super) async fn latest_snapshot_date(
    db: &DatabaseConnection,
    game: &str,
    holdings: &[HoldingRow],
    table: HistoryTable,
) -> Result<Option<String>, AppError> {
    let ids: Vec<i32> = holdings.iter().map(|h| h.item_id).collect();
    let mut latest: Option<String> = None;
    for chunk in ids.chunks(PRICE_ID_CHUNK) {
        let chunk_latest = match table {
            HistoryTable::Cards => {
                CardPriceHistory::find()
                    .select_only()
                    .column_as(card_price_history::Column::AsOfDate.max(), "latest")
                    .filter(card_price_history::Column::Game.eq(game))
                    .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()))
                    .into_tuple::<Option<String>>()
                    .one(db)
                    .await?
            }
            HistoryTable::Products => {
                ProductPriceHistory::find()
                    .select_only()
                    .column_as(product_price_history::Column::AsOfDate.max(), "latest")
                    .filter(product_price_history::Column::Game.eq(game))
                    .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()))
                    .into_tuple::<Option<String>>()
                    .one(db)
                    .await?
            }
        }
        .flatten();
        if let Some(candidate) = chunk_latest
            && latest
                .as_ref()
                .is_none_or(|current| candidate.as_str() > current.as_str())
        {
            latest = Some(candidate);
        }
    }
    Ok(latest)
}
