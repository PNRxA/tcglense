//! Daily price-history capture for sealed products: reading each product's current
//! prices and upserting one `product_price_history` row per `(game, product, day)`.
//! The sealed-product mirror of `scryfall::price_history::snapshot_prices`.

use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
    sea_query::OnConflict,
};

use super::BackfillError;
use crate::db::upsert_changed_guard;
use crate::entities::prelude::{Product, ProductPriceHistory};
use crate::entities::{product, product_price_history};

/// Rows per price-history upsert. A history row has 6 columns, so ~2000 rows ≈ 12k
/// bound parameters — under SQLite's 32 766 limit.
pub(super) const PRICE_HISTORY_BATCH: usize = 2000;

/// Capture today's price snapshot for a game's sealed products by reading the
/// **already-committed** `products` rows and upserting one `product_price_history` row
/// per product for `as_of_date`.
///
/// Reads the committed table (not the streaming sync) so the daily series stays
/// continuous even on a tick where the version-gated products sync is skipped — the
/// same rationale as the card snapshot. Idempotent on `(game, product_id, as_of_date)`.
/// Returns the number of rows captured (0 when the game has no products yet).
pub async fn snapshot_prices(
    db: &DatabaseConnection,
    game: &str,
    as_of_date: &str,
) -> Result<u64, BackfillError> {
    // Only id + the two price columns; skip the heavier text columns.
    let rows: Vec<(i32, Option<String>, Option<String>)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::PriceUsd)
        .column(product::Column::PriceUsdFoil)
        .filter(product::Column::Game.eq(game))
        .into_tuple()
        .all(db)
        .await?;

    let now = Utc::now();
    let mut total: u64 = 0;
    let mut batch: Vec<product_price_history::ActiveModel> =
        Vec::with_capacity(PRICE_HISTORY_BATCH);
    for (product_id, usd, usd_foil) in rows {
        batch.push(product_price_history::ActiveModel {
            id: NotSet,
            game: Set(game.to_string()),
            product_id: Set(product_id),
            as_of_date: Set(as_of_date.to_string()),
            price_usd: Set(usd),
            price_usd_foil: Set(usd_foil),
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

/// Batched upsert of product price-history rows on the `(game, product_id, as_of_date)`
/// unique key, updating only the price columns (so `created_at` is preserved on a
/// same-day re-run). A change-guard skips the write entirely when the day's row already
/// holds these exact prices, so a same-day restart/tick doesn't rewrite unchanged rows.
/// Shared by the daily snapshot and the dummy seeder.
pub(crate) async fn upsert_price_history(
    db: &DatabaseConnection,
    batch: Vec<product_price_history::ActiveModel>,
) -> Result<(), BackfillError> {
    if batch.is_empty() {
        return Ok(());
    }
    ProductPriceHistory::insert_many(batch)
        .on_conflict(
            OnConflict::columns([
                product_price_history::Column::Game,
                product_price_history::Column::ProductId,
                product_price_history::Column::AsOfDate,
            ])
            .update_columns([
                product_price_history::Column::PriceUsd,
                product_price_history::Column::PriceUsdFoil,
            ])
            // Skip the write when the day's row already holds these exact prices — the
            // sealed-product mirror of the card snapshot's guard. This runs on every boot and
            // sync tick, so a same-day restart would otherwise rewrite every product row to
            // identical values. `created_at` is excluded from the compare (always-`now()`,
            // never updated); a real intra-day price move still differs and still writes.
            .action_and_where(upsert_changed_guard::<product_price_history::Column>(
                "product_price_history",
                |c| {
                    matches!(
                        c,
                        product_price_history::Column::Id
                            | product_price_history::Column::Game
                            | product_price_history::Column::ProductId
                            | product_price_history::Column::AsOfDate
                            | product_price_history::Column::CreatedAt
                    )
                },
            ))
            .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::ProductPriceHistory;

    async fn insert_product(db: &DatabaseConnection, ext: &str, usd: Option<&str>) -> i32 {
        use sea_orm::ActiveModelTrait;
        let now = Utc::now();
        product::ActiveModel {
            game: Set(super::super::GAME.to_string()),
            external_id: Set(ext.to_string()),
            name: Set(format!("Product {ext}")),
            set_code: Set("tst".to_string()),
            product_type: Set("bundle".to_string()),
            price_usd: Set(usd.map(str::to_string)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert product")
        .id
    }

    #[tokio::test]
    async fn snapshots_products_and_upserts_same_day() {
        let db = crate::test_support::migrated_memory_db().await;
        insert_product(&db, "100", Some("199.99")).await;
        insert_product(&db, "200", None).await;

        let n = snapshot_prices(&db, super::super::GAME, "2024-06-01")
            .await
            .expect("snapshot");
        assert_eq!(n, 2, "one row per product");

        // Re-run same day upserts (no duplicate rows).
        let n = snapshot_prices(&db, super::super::GAME, "2024-06-01")
            .await
            .expect("snapshot again");
        assert_eq!(n, 2);
        let all = ProductPriceHistory::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    /// The change-guard must still write a genuine intra-day price move on a same-day
    /// re-snapshot — the sealed-product mirror of the card-side guard test.
    #[tokio::test]
    async fn same_day_resnapshot_writes_changed_product_price() {
        use sea_orm::ActiveModelTrait;
        let db = crate::test_support::migrated_memory_db().await;
        let day = "2024-06-01";
        let mover = insert_product(&db, "100", Some("10.00")).await;
        let steady = insert_product(&db, "200", Some("20.00")).await;

        assert_eq!(
            snapshot_prices(&db, super::super::GAME, day).await.unwrap(),
            2
        );

        // A real intra-day price move on one product only.
        product::ActiveModel {
            id: Set(mover),
            price_usd: Set(Some("11.00".to_string())),
            ..Default::default()
        }
        .update(&db)
        .await
        .expect("bump product price");

        assert_eq!(
            snapshot_prices(&db, super::super::GAME, day).await.unwrap(),
            2
        );

        let mover_rows = ProductPriceHistory::find()
            .filter(product_price_history::Column::ProductId.eq(mover))
            .filter(product_price_history::Column::AsOfDate.eq(day))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            mover_rows.len(),
            1,
            "same-day re-snapshot upserts, never duplicates"
        );
        assert_eq!(
            mover_rows[0].price_usd.as_deref(),
            Some("11.00"),
            "the guard let the real intra-day change through"
        );

        let steady_rows = ProductPriceHistory::find()
            .filter(product_price_history::Column::ProductId.eq(steady))
            .filter(product_price_history::Column::AsOfDate.eq(day))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(steady_rows.len(), 1);
        assert_eq!(
            steady_rows[0].price_usd.as_deref(),
            Some("20.00"),
            "the unchanged product's row is preserved"
        );
    }
}
