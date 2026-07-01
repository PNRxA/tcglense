//! Daily price-history capture: reading each card's current prices and upserting
//! one `card_price_history` row per `(game, card, day)`. Shared by the live sync
//! (`snapshot_prices`) and the offline dummy seeder.

use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
    sea_query::OnConflict,
};

use super::ingest::IngestError;
use crate::entities::prelude::{Card, CardPriceHistory};
use crate::entities::{card, card_price_history};

/// Rows per price-history upsert. A history row has 8 columns, so ~2000 rows ≈ 16k
/// bound parameters — comfortably under SQLite's 32 766 limit.
pub(super) const PRICE_HISTORY_BATCH: usize = 2000;

/// A card's id plus its four current price columns, as read for a price snapshot
/// (`usd`, `usd_foil`, `eur`, `tix`). Shared by the live snapshot and dummy seeder.
pub(super) type PriceColumns = (
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Load every card's id + four price columns for `game`. Selects only those five
/// columns (skipping the heavy text columns), shared by the live snapshot and the
/// dummy seeder.
pub(super) async fn load_price_columns(
    db: &DatabaseConnection,
    game: &str,
) -> Result<Vec<PriceColumns>, IngestError> {
    Ok(Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::PriceUsd)
        .column(card::Column::PriceUsdFoil)
        .column(card::Column::PriceEur)
        .column(card::Column::PriceTix)
        .filter(card::Column::Game.eq(game))
        .into_tuple()
        .all(db)
        .await?)
}

/// Format a date as the `"YYYY-MM-DD"` string used for `as_of_date` (and matching
/// how `cards.released_at` is stored).
pub(crate) fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Capture today's price snapshot for a game by reading the **already-committed**
/// `cards` rows (their current price columns) and upserting one
/// `card_price_history` row per card for `as_of_date`.
///
/// Reading the committed cards table — rather than hooking the streaming import — is
/// deliberate: the import is version-gated and skipped whenever Scryfall's dataset
/// `updated_at` is unchanged, so capturing inside the stream would leave gaps in the
/// daily series. This runs on every sync tick and records `as_of_date` with the
/// last-known prices, keeping the series continuous. Idempotent: a same-day re-run
/// upserts on `(game, card_id, as_of_date)`. Returns the number of rows captured (0
/// when the game has no cards yet — no error).
pub async fn snapshot_prices(
    db: &DatabaseConnection,
    game: &str,
    as_of_date: &str,
) -> Result<u64, IngestError> {
    // Only the id + four price columns; avoids loading the heavy text columns.
    let rows = load_price_columns(db, game).await?;

    let now = Utc::now();
    let mut total: u64 = 0;
    let mut batch: Vec<card_price_history::ActiveModel> = Vec::with_capacity(PRICE_HISTORY_BATCH);
    for (card_id, usd, usd_foil, eur, tix) in rows {
        batch.push(card_price_history::ActiveModel {
            id: NotSet,
            game: Set(game.to_string()),
            card_id: Set(card_id),
            as_of_date: Set(as_of_date.to_string()),
            price_usd: Set(usd),
            price_usd_foil: Set(usd_foil),
            price_eur: Set(eur),
            price_tix: Set(tix),
            created_at: Set(now),
        });
        if batch.len() >= PRICE_HISTORY_BATCH {
            let n = batch.len() as u64;
            upsert_price_history(db, std::mem::take(&mut batch)).await?;
            total += n;
            batch.reserve(PRICE_HISTORY_BATCH);
        }
    }
    if !batch.is_empty() {
        let n = batch.len() as u64;
        upsert_price_history(db, batch).await?;
        total += n;
    }
    Ok(total)
}

/// Batched upsert of price-history rows on the `(game, card_id, as_of_date)` unique
/// key, updating only the four price columns (so `created_at` is preserved on a
/// same-day re-run). Shared by the live snapshot and the dummy seeder.
pub(super) async fn upsert_price_history(
    db: &DatabaseConnection,
    batch: Vec<card_price_history::ActiveModel>,
) -> Result<(), IngestError> {
    if batch.is_empty() {
        return Ok(());
    }
    CardPriceHistory::insert_many(batch)
        .on_conflict(
            OnConflict::columns([
                card_price_history::Column::Game,
                card_price_history::Column::CardId,
                card_price_history::Column::AsOfDate,
            ])
            .update_columns([
                card_price_history::Column::PriceUsd,
                card_price_history::Column::PriceUsdFoil,
                card_price_history::Column::PriceEur,
                card_price_history::Column::PriceTix,
            ])
            .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_date_is_iso() {
        let d = NaiveDate::from_ymd_opt(2026, 6, 30).unwrap();
        assert_eq!(format_date(d), "2026-06-30");
    }
}
