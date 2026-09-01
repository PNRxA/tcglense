//! Gap detection for the historic price backfill (issue #655): which days is the
//! daily price history missing?
//!
//! The 2026-08 sync outage left blocks of absent days in `card_price_history` /
//! `product_price_history` that the daily snapshot can never repair (it only ever
//! writes *today's* date), and the backfill's original binary "ran once" gate had no
//! way to revisit them. So instead of a gate, every backfill run starts here: one
//! `GROUP BY as_of_date` row count per history table, folded by [`compute_plan`]
//! into the exact day list to walk. A day is a gap when **either** table's count is
//! zero (an absent day — or a table the original one-shot never covered) or
//! suspiciously far below the median day (a partially-written day; see
//! [`GAP_SANITY_DIVISOR`]). Gap days behind the walkable window — before TCGCSV's
//! first archive, or outside `PRICE_BACKFILL_DAYS` — are reported separately so the
//! caller can log what cannot be filled rather than silently narrowing.

use std::collections::HashMap;

use chrono::NaiveDate;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
    sea_query::{Asterisk, Expr},
};

use super::GAME;
use crate::entities::prelude::{CardPriceHistory, Product, ProductPriceHistory};
use crate::entities::{card_price_history, product, product_price_history};

/// A nonzero day counting fewer than `1/GAP_SANITY_DIVISOR` of its table's median
/// nonzero day is treated as partially written and re-walked (`ON CONFLICT DO
/// NOTHING` fills only its missing rows). The margin is deliberately wide: an
/// organic snapshot day covers the whole catalog while a TCGCSV-filled day covers
/// only entities with a TCGplayer id and a USD price that day (roughly half to
/// two-thirds of it), and the catalog itself grows over time — neither may read as a
/// gap, or every healthy backfilled day would be re-fetched on every boot, forever.
/// A crashed capture leaves a few insert batches (thousands of rows against ~100k),
/// far below a quarter of the median.
const GAP_SANITY_DIVISOR: i64 = 4;

/// The planned walk, split by fillability. Both lists are ascending and disjoint.
pub(super) struct GapPlan {
    /// Gap days inside the walkable window — the days to fetch and fill.
    pub walkable: Vec<NaiveDate>,
    /// Gap days behind the walkable window — unfillable (before TCGCSV's first
    /// archive, or outside `PRICE_BACKFILL_DAYS`); the caller logs them.
    pub unfillable: Vec<NaiveDate>,
}

/// Count history rows per day for both tables and fold them into the walk plan.
///
/// Product-side detection is skipped while the `products` table is empty for the
/// game: the backfill's join map would match nothing, so an all-zero product history
/// is not evidence of a gap there — and without the guard such an install would
/// re-walk the whole window on every boot to fill rows it can never write.
pub(super) async fn plan(
    db: &DatabaseConnection,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Result<GapPlan, DbErr> {
    let card_counts = card_day_counts(db).await?;
    let products_exist = Product::find()
        .filter(product::Column::Game.eq(GAME))
        .count(db)
        .await?
        > 0;
    let product_counts = if products_exist {
        Some(product_day_counts(db).await?)
    } else {
        None
    };
    Ok(compute_plan(
        window_start,
        window_end,
        &card_counts,
        product_counts.as_ref(),
    ))
}

/// Per-day `card_price_history` row counts for the game, keyed by parsed date
/// (rows whose `as_of_date` isn't `YYYY-MM-DD` are ignored — none are written).
///
/// `COUNT(*)` rather than `COUNT(id)`: `id` isn't in the `(game, card_id,
/// as_of_date)` unique index, so counting it would force Postgres to visit the
/// multi-million-row heap; `COUNT(*)` answers from an index-only scan (whose
/// visibility map the daily post-capture `VACUUM (ANALYZE)` keeps fresh).
async fn card_day_counts(db: &DatabaseConnection) -> Result<HashMap<NaiveDate, i64>, DbErr> {
    let rows: Vec<(String, i64)> = CardPriceHistory::find()
        .select_only()
        .column(card_price_history::Column::AsOfDate)
        .column_as(Expr::col(Asterisk).count(), "n")
        .filter(card_price_history::Column::Game.eq(GAME))
        .group_by(card_price_history::Column::AsOfDate)
        .into_tuple()
        .all(db)
        .await?;
    Ok(parse_day_counts(rows))
}

/// Per-day `product_price_history` row counts for the game — the sealed-product
/// mirror of [`card_day_counts`].
async fn product_day_counts(db: &DatabaseConnection) -> Result<HashMap<NaiveDate, i64>, DbErr> {
    let rows: Vec<(String, i64)> = ProductPriceHistory::find()
        .select_only()
        .column(product_price_history::Column::AsOfDate)
        .column_as(Expr::col(Asterisk).count(), "n")
        .filter(product_price_history::Column::Game.eq(GAME))
        .group_by(product_price_history::Column::AsOfDate)
        .into_tuple()
        .all(db)
        .await?;
    Ok(parse_day_counts(rows))
}

fn parse_day_counts(rows: Vec<(String, i64)>) -> HashMap<NaiveDate, i64> {
    rows.into_iter()
        .filter_map(|(date, n)| {
            NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .ok()
                .map(|d| (d, n))
        })
        .collect()
}

/// Pure core of [`plan`]: classify every day from the earliest observed history day
/// (or `window_start`, whichever is earlier) through `window_end`. A day absent from
/// a count map counts as zero. `product_counts` is `None` when product-side
/// detection is disabled (no products imported yet).
fn compute_plan(
    window_start: NaiveDate,
    window_end: NaiveDate,
    card_counts: &HashMap<NaiveDate, i64>,
    product_counts: Option<&HashMap<NaiveDate, i64>>,
) -> GapPlan {
    let card_median = median_nonzero(card_counts);
    let product_median = product_counts.map(median_nonzero).unwrap_or(0);

    // Scan from the earliest day any history exists so gaps *behind* the walkable
    // window are noticed (and logged by the caller) rather than silently skipped;
    // days before all recorded history aren't gaps, they're pre-history.
    let detect_start = card_counts
        .keys()
        .chain(product_counts.into_iter().flat_map(HashMap::keys))
        .min()
        .copied()
        .unwrap_or(window_start)
        .min(window_start);

    let mut walkable = Vec::new();
    let mut unfillable = Vec::new();
    let mut day = detect_start;
    while day <= window_end {
        let card_gap = side_gap(card_counts.get(&day).copied().unwrap_or(0), card_median);
        let product_gap = product_counts
            .map(|counts| side_gap(counts.get(&day).copied().unwrap_or(0), product_median))
            .unwrap_or(false);
        if card_gap || product_gap {
            if day >= window_start {
                walkable.push(day);
            } else {
                unfillable.push(day);
            }
        }
        day += chrono::Duration::days(1);
    }
    GapPlan {
        walkable,
        unfillable,
    }
}

/// Whether one table's day is a gap: absent/zero, or partially written (nonzero but
/// under a quarter of the table's median nonzero day).
fn side_gap(count: i64, median: i64) -> bool {
    count == 0 || count.saturating_mul(GAP_SANITY_DIVISOR) < median
}

/// Lower median of the map's nonzero counts; `0` when there are none (then only the
/// zero-count check applies — an all-empty table is all gaps). The lower order
/// statistic keeps the threshold lenient when day counts straddle two eras.
fn median_nonzero(counts: &HashMap<NaiveDate, i64>) -> i64 {
    let mut nonzero: Vec<i64> = counts.values().copied().filter(|n| *n > 0).collect();
    if nonzero.is_empty() {
        return 0;
    }
    nonzero.sort_unstable();
    nonzero[(nonzero.len() - 1) / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("valid test date")
    }

    fn counts(pairs: &[(&str, i64)]) -> HashMap<NaiveDate, i64> {
        pairs.iter().map(|(date, n)| (d(date), *n)).collect()
    }

    #[test]
    fn fresh_tables_walk_the_whole_window() {
        let plan = compute_plan(d("2024-02-08"), d("2024-02-12"), &HashMap::new(), None);
        assert_eq!(
            plan.walkable,
            [
                "2024-02-08",
                "2024-02-09",
                "2024-02-10",
                "2024-02-11",
                "2024-02-12"
            ]
            .map(d)
        );
        assert!(plan.unfillable.is_empty());
    }

    #[test]
    fn absent_days_inside_the_window_are_walkable_gaps() {
        // 02-10 and 02-12 have no rows at all (the outage shape): exactly those walk.
        let cards = counts(&[
            ("2024-02-08", 100),
            ("2024-02-09", 100),
            ("2024-02-11", 100),
        ]);
        let plan = compute_plan(d("2024-02-08"), d("2024-02-12"), &cards, None);
        assert_eq!(plan.walkable, ["2024-02-10", "2024-02-12"].map(d));
        assert!(plan.unfillable.is_empty());
    }

    #[test]
    fn partially_written_days_are_gaps_but_organic_variation_is_not() {
        let cards = counts(&[
            // Early era: the catalog was smaller and a filled day is USD-only —
            // roughly half of a modern organic day. Healthy, must not re-walk.
            ("2024-02-08", 55),
            ("2024-02-09", 60),
            // A crashed capture: a sliver of the day's rows. Re-walk.
            ("2024-02-10", 10),
            // Modern organic days.
            ("2024-02-11", 100),
            ("2024-02-12", 100),
        ]);
        let plan = compute_plan(d("2024-02-08"), d("2024-02-12"), &cards, None);
        assert_eq!(plan.walkable, [d("2024-02-10")]);
    }

    #[test]
    fn gaps_behind_the_window_are_reported_unfillable() {
        // Window starts 02-10 (days_cap narrowed it); history began 02-08 with a
        // hole at 02-09 — unfillable — and another at 02-11 — walkable.
        let cards = counts(&[
            ("2024-02-08", 100),
            ("2024-02-10", 100),
            ("2024-02-12", 100),
        ]);
        let plan = compute_plan(d("2024-02-10"), d("2024-02-12"), &cards, None);
        assert_eq!(plan.walkable, [d("2024-02-11")]);
        assert_eq!(plan.unfillable, [d("2024-02-09")]);
    }

    #[test]
    fn a_product_gap_walks_a_day_with_healthy_cards() {
        let cards = counts(&[
            ("2024-02-08", 100),
            ("2024-02-09", 100),
            ("2024-02-10", 100),
        ]);
        let products = counts(&[("2024-02-08", 20), ("2024-02-10", 20)]);
        let plan = compute_plan(d("2024-02-08"), d("2024-02-10"), &cards, Some(&products));
        assert_eq!(plan.walkable, [d("2024-02-09")]);
    }

    #[test]
    fn product_detection_disabled_ignores_missing_product_history() {
        let cards = counts(&[("2024-02-08", 100), ("2024-02-09", 100)]);
        // No products imported: an empty product history is not a gap signal.
        let plan = compute_plan(d("2024-02-08"), d("2024-02-09"), &cards, None);
        assert!(plan.walkable.is_empty());
        assert!(plan.unfillable.is_empty());
    }

    #[test]
    fn an_all_zero_product_history_walks_everything_when_products_exist() {
        // Products imported after the original card-only one-shot: the product side
        // is entirely empty, so every day re-walks once to heal it (cards no-op).
        let cards = counts(&[("2024-02-08", 100), ("2024-02-09", 100)]);
        let products = HashMap::new();
        let plan = compute_plan(d("2024-02-08"), d("2024-02-09"), &cards, Some(&products));
        assert_eq!(plan.walkable, ["2024-02-08", "2024-02-09"].map(d));
    }

    #[test]
    fn median_ignores_zero_days_and_takes_the_lower_middle() {
        assert_eq!(median_nonzero(&HashMap::new()), 0);
        assert_eq!(median_nonzero(&counts(&[("2024-02-08", 0)])), 0);
        // Nonzero values [10, 100]: the lower middle keeps the threshold lenient.
        let two = counts(&[("2024-02-08", 10), ("2024-02-09", 100), ("2024-02-10", 0)]);
        assert_eq!(median_nonzero(&two), 10);
    }

    #[tokio::test]
    async fn day_counts_group_per_day_and_filter_by_game() {
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;
        use sea_orm::ActiveValue::{NotSet, Set};

        let db = crate::test_support::migrated_memory_db().await;
        let card_id = crate::test_support::insert_card(&db, "gap-a").await;
        let other_card = crate::test_support::insert_card(&db, "gap-b").await;
        let product_id =
            crate::test_support::insert_product(&db, "9001", "Box", "tst", "bundle", Some("5.00"))
                .await;
        let now = Utc::now();

        let card_row = |card: i32, game: &str, date: &str| card_price_history::ActiveModel {
            id: NotSet,
            game: Set(game.to_string()),
            card_id: Set(card),
            as_of_date: Set(date.to_string()),
            price_usd: Set(Some("1.00".to_string())),
            price_usd_foil: Set(None),
            price_eur: Set(None),
            price_tix: Set(None),
            created_at: Set(now),
        };
        CardPriceHistory::insert_many(vec![
            card_row(card_id, GAME, "2024-02-08"),
            card_row(other_card, GAME, "2024-02-08"),
            card_row(card_id, GAME, "2024-02-09"),
            // Another game's row must not leak into the counts.
            card_row(card_id, "othergame", "2024-02-10"),
        ])
        .exec(&db)
        .await
        .expect("insert card history");

        product_price_history::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            product_id: Set(product_id),
            as_of_date: Set("2024-02-09".to_string()),
            price_usd: Set(Some("5.00".to_string())),
            price_usd_foil: Set(None),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert product history");

        let cards = card_day_counts(&db).await.expect("card counts");
        assert_eq!(cards.get(&d("2024-02-08")), Some(&2));
        assert_eq!(cards.get(&d("2024-02-09")), Some(&1));
        assert_eq!(cards.get(&d("2024-02-10")), None);

        let products = product_day_counts(&db).await.expect("product counts");
        assert_eq!(products.get(&d("2024-02-09")), Some(&1));

        // The full plan over that state: 02-10 is a card gap; 02-08 is a product gap.
        let plan = plan(&db, d("2024-02-08"), d("2024-02-10"))
            .await
            .expect("plan");
        assert_eq!(plan.walkable, ["2024-02-08", "2024-02-10"].map(d));
    }
}
