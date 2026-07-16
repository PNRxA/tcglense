//! Collection value-over-time: the signed-in user's card and sealed-product values across
//! the same `?range` windows the per-item price charts use, reconstructed from historic
//! prices and the collection's current contents.
//!
//! We have daily card/product price snapshots, but **no** per-holding quantity history. The
//! graph therefore re-prices the user's *current* basket on every historic snapshot day,
//! deliberately ignoring when each row was added: on day `D` we value every card and sealed
//! product the user owns **today** at its price on `D`. Quantity changes are likewise not
//! reconstructed. Prices before daily snapshots began (or the 2024-02-08 backfill floor)
//! simply don't exist, so the series starts where the data does.
//!
//! That has a consequence worth stating plainly, because it is visible in the chart. The date
//! axis is the union of *every* holding's snapshot days, and a single priced holding is enough
//! to make a day a real total — so a holding whose own price history starts later than the axis
//! does (a recent printing sitting next to older cards) contributes nothing to the earlier days
//! instead of blanking them. Those days are reported as values, not gaps, over whichever part
//! of the basket was priced then: the left edge of the line under-counts the collection and
//! ramps up in steps as each holding's history begins. It is a floor on what the basket was
//! worth, not a measurement of it.

use std::collections::HashMap;

use axum::extract::State;
use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use crate::analytics_cache::json_body_response;
use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{
    CardPriceHistory, CollectionItem, CollectionProductItem, ProductPriceHistory,
};
use crate::entities::{
    card_price_history, collection_item, collection_product_item, product_price_history,
};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::handlers::shared::{
    DataBody, PriceParams, PriceRange, cutoff_date, downsample_rows, require_game,
};
use crate::state::AppState;

use super::price_movements::{decode_snapshot, latest_snapshot};

/// How many card ids to bind per `IN (...)` chunk. Kept well under SQLite's 32766
/// bound-parameter cap (each chunk also binds `game` + the optional cutoff), so an
/// arbitrarily large collection still fetches in a handful of queries.
const PRICE_ID_CHUNK: usize = 10_000;

/// One held item's last captured snapshot before an explicit range cutoff. It seeds the
/// carry-forward cursor without exposing an out-of-range day in the response.
#[derive(FromQueryResult)]
struct CutoffAnchor {
    item_id: i32,
    snapshot: String,
}

/// One day in the collection's value-over-time series. Card and sealed values are separate
/// lines; each is `None` until one holding of that kind has a captured price on/before the
/// day (so a line gaps rather than fabricating a zero before its history begins).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionValuePoint {
    pub date: String,
    /// Total USD market value of the user's card holdings on this day.
    pub value_usd: Option<String>,
    /// Total USD market value of the user's sealed-product holdings on this day.
    pub sealed_value_usd: Option<String>,
}

/// Get collection value history
///
/// `GET /api/collection/{game}/value-history?range=` -> the signed-in user's current card
/// and sealed-product basket revalued at historic prices, ignoring holding add dates and
/// ordered oldest day first for charting. With no `range` the full daily series is returned;
/// an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows the series and
/// **downsamples** it to a coarser resolution the longer the window (the same vocabulary as
/// the per-card price chart). Each holding contributes only from the first day its own price
/// history covers, so a day predating part of the basket's history is still a real total over
/// the priced remainder rather than a gap — early days under-count. `404` if the game is
/// unknown; `422` for an unknown `range`; an empty `{ "data": [] }` when the user owns nothing
/// or no captured price history falls in the window.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/value-history",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("range" = Option<String>, Query, description = "Window + resolution (`7d`/`30d`/`1y`/`2y`/`3y`/`all`); absent = the full daily series"),
    ),
    responses(
        (status = 200, description = "The user's current card and sealed-product basket revalued at historic prices regardless of holding add dates, oldest day first (empty when nothing is owned or no captured price falls in the window).", body = DataBody<Vec<CollectionValuePoint>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown `range` value."),
    ),
)]
pub async fn collection_value_history(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<PriceParams>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;

    // Blank/absent range -> the full daily series; an explicit range windows + downsamples.
    // Unknown values 422, like a bad `sort` (mirrors the per-card price endpoint).
    let range = match params
        .range
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(value) => Some(PriceRange::parse(value)?),
    };

    // Version-keyed response cache (issues #413/#365): between the user's own edits
    // and the daily price capture this response cannot change, and it is the app's
    // most expensive per-user read. `None` key = cache degraded, compute as normal.
    let cache_key = state
        .analytics_cache
        .body_key(
            user.id,
            &game,
            "value-history",
            range.map_or("full", PriceRange::token),
        )
        .await;
    if let Some(key) = &cache_key
        && let Some(body) = state.analytics_cache.get_body(key).await
    {
        return Ok(json_body_response(body));
    }

    let payload = value_history_payload(state.clone(), user, game, range).await?;
    let body = serde_json::to_vec(&payload)
        .map_err(|err| AppError::Internal(format!("serialize value history: {err}")))?;
    if let Some(key) = &cache_key {
        state.analytics_cache.put_body(key, &body).await;
    }
    Ok(json_body_response(body))
}

/// Compute the value-history payload (the handler above wraps this in the
/// analytics response cache; every early return below is cached the same way).
async fn value_history_payload(
    state: AppState,
    user: crate::entities::user::Model,
    game: String,
    range: Option<PriceRange>,
) -> Result<DataBody<Vec<CollectionValuePoint>>, AppError> {
    // The user's current card + sealed holdings. Only the three columns the fold reads are
    // pulled (never the wide catalog rows); acquisition dates deliberately do not affect a
    // historic revaluation of the current basket.
    let card_holdings: Vec<(i32, i32, i32)> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .into_tuple()
        .all(&state.db)
        .await?;

    let product_holdings: Vec<(i32, i32, i32)> = CollectionProductItem::find()
        .select_only()
        .column(collection_product_item::Column::ProductId)
        .column(collection_product_item::Column::Quantity)
        .column(collection_product_item::Column::FoilQuantity)
        .filter(collection_product_item::Column::UserId.eq(user.id))
        .filter(collection_product_item::Column::Game.eq(game.as_str()))
        .into_tuple()
        .all(&state.db)
        .await?;

    if card_holdings.is_empty() && product_holdings.is_empty() {
        return Ok(DataBody { data: Vec::new() });
    }

    let to_holding = |(item_id, quantity, foil_quantity): (i32, i32, i32)| HoldingRow {
        item_id,
        quantity,
        foil_quantity,
    };
    let card_holdings: Vec<HoldingRow> = card_holdings.into_iter().map(to_holding).collect();
    let product_holdings: Vec<HoldingRow> = product_holdings.into_iter().map(to_holding).collect();

    let card_ids: Vec<i32> = card_holdings.iter().map(|h| h.item_id).collect();
    let product_ids: Vec<i32> = product_holdings.iter().map(|h| h.item_id).collect();
    let cutoff = range.and_then(|r| cutoff_date(Utc::now().date_naive(), r));

    // Historic prices for exactly those cards, windowed to the range. Chunk the id list so
    // the `IN (...)` never exceeds the bound-parameter cap; the (game, card_id, as_of_date)
    // unique index serves each chunk (equality on game + card_id, range on as_of_date). Only
    // the four columns the fold reads are fetched — so the query stays cheap on a cold DB.
    let mut price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();
    for chunk in card_ids.chunks(PRICE_ID_CHUNK) {
        if let Some(cutoff) = &cutoff {
            let anchors = CardPriceHistory::find()
                .select_only()
                .column_as(card_price_history::Column::CardId, "item_id")
                .column_as(
                    latest_snapshot(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                    ),
                    "snapshot",
                )
                .filter(card_price_history::Column::Game.eq(game.as_str()))
                .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()))
                .filter(card_price_history::Column::AsOfDate.lt(cutoff.as_str()))
                .group_by(card_price_history::Column::CardId)
                .into_model::<CutoffAnchor>()
                .all(&state.db)
                .await?;
            for anchor in anchors {
                let (date, usd, foil) = decode_snapshot(&anchor.snapshot)?;
                price_rows.push((anchor.item_id, date, usd, foil));
            }
        }
        let mut query = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::CardId)
            .column(card_price_history::Column::AsOfDate)
            .column(card_price_history::Column::PriceUsd)
            .column(card_price_history::Column::PriceUsdFoil)
            .filter(card_price_history::Column::Game.eq(game.as_str()))
            .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()));
        if let Some(cutoff) = &cutoff {
            query = query.filter(card_price_history::Column::AsOfDate.gte(cutoff.as_str()));
        }
        let rows = query
            .order_by_asc(card_price_history::Column::CardId)
            .order_by_asc(card_price_history::Column::AsOfDate)
            .into_tuple::<(i32, String, Option<String>, Option<String>)>()
            .all(&state.db)
            .await?;
        price_rows.extend(rows);
    }

    // Group each card's snapshots (already ascending by date within a card) and parse the
    // decimal-string prices to integer cents once, up front.
    let mut prices: HashMap<i32, Vec<PriceCell>> = HashMap::new();
    for (card_id, date, usd, foil) in price_rows {
        prices.entry(card_id).or_default().push(PriceCell {
            date,
            usd_cents: price_cents(usd.as_deref()),
            foil_cents: price_cents(foil.as_deref()),
        });
    }

    let mut product_price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();
    for chunk in product_ids.chunks(PRICE_ID_CHUNK) {
        if let Some(cutoff) = &cutoff {
            let anchors = ProductPriceHistory::find()
                .select_only()
                .column_as(product_price_history::Column::ProductId, "item_id")
                .column_as(
                    latest_snapshot(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                    ),
                    "snapshot",
                )
                .filter(product_price_history::Column::Game.eq(game.as_str()))
                .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()))
                .filter(product_price_history::Column::AsOfDate.lt(cutoff.as_str()))
                .group_by(product_price_history::Column::ProductId)
                .into_model::<CutoffAnchor>()
                .all(&state.db)
                .await?;
            for anchor in anchors {
                let (date, usd, foil) = decode_snapshot(&anchor.snapshot)?;
                product_price_rows.push((anchor.item_id, date, usd, foil));
            }
        }
        let mut query = ProductPriceHistory::find()
            .select_only()
            .column(product_price_history::Column::ProductId)
            .column(product_price_history::Column::AsOfDate)
            .column(product_price_history::Column::PriceUsd)
            .column(product_price_history::Column::PriceUsdFoil)
            .filter(product_price_history::Column::Game.eq(game.as_str()))
            .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()));
        if let Some(cutoff) = &cutoff {
            query = query.filter(product_price_history::Column::AsOfDate.gte(cutoff.as_str()));
        }
        let rows = query
            .order_by_asc(product_price_history::Column::ProductId)
            .order_by_asc(product_price_history::Column::AsOfDate)
            .into_tuple::<(i32, String, Option<String>, Option<String>)>()
            .all(&state.db)
            .await?;
        product_price_rows.extend(rows);
    }
    let mut product_prices: HashMap<i32, Vec<PriceCell>> = HashMap::new();
    for (product_id, date, usd, foil) in product_price_rows {
        product_prices
            .entry(product_id)
            .or_default()
            .push(PriceCell {
                date,
                usd_cents: price_cents(usd.as_deref()),
                foil_cents: price_cents(foil.as_deref()),
            });
    }

    let points = fold_collection_value_history(
        &card_holdings,
        &prices,
        &product_holdings,
        &product_prices,
        range.map_or(1, PriceRange::bucket_days),
        cutoff.as_deref(),
    );
    Ok(DataBody { data: points })
}

/// A holding reduced to what the fold needs: the item and its current counts.
struct HoldingRow {
    item_id: i32,
    quantity: i32,
    foil_quantity: i32,
}

/// One item's snapshot for a day: the date and its regular/foil price already in integer
/// cents (`None` = unpriced that day, so it contributes nothing).
struct PriceCell {
    date: String,
    usd_cents: Option<i128>,
    foil_cents: Option<i128>,
}

/// Fold both holding kinds into parallel card and sealed-product value lines over their
/// shared union date axis, then downsample to `bucket_days`.
///
/// The date axis is the union of every card/product snapshot day. Walking it ascending, each
/// holding carries its last-seen price forward across days it has no snapshot (so one card
/// missing a day doesn't make the aggregate line jitter). Every current holding contributes
/// regardless of when it was added. A day is `None` (a gap) until at least one owned holding
/// has a captured price — matching the per-card chart and the summary's "unpriced = null"
/// rule.
fn fold_collection_value_history(
    card_holdings: &[HoldingRow],
    card_prices: &HashMap<i32, Vec<PriceCell>>,
    product_holdings: &[HoldingRow],
    product_prices: &HashMap<i32, Vec<PriceCell>>,
    bucket_days: i64,
    range_floor: Option<&str>,
) -> Vec<CollectionValuePoint> {
    // The sorted, de-duplicated union of every snapshot day. Zero-padded `YYYY-MM-DD`
    // sorts chronologically as a plain string.
    let mut axis: Vec<&str> = card_prices
        .values()
        .chain(product_prices.values())
        .flat_map(|cells| cells.iter().map(|c| c.date.as_str()))
        .collect();
    if let Some(floor) = range_floor {
        axis.retain(|day| *day >= floor);
        // A synthetic first day lets a sparse series expose the carried pre-cutoff anchor
        // even when that item has no fresh snapshot inside the requested window.
        if !card_prices.is_empty() || !product_prices.is_empty() {
            axis.push(floor);
        }
    }
    axis.sort_unstable();
    axis.dedup();

    let card_values = fold_value_series(card_holdings, card_prices, &axis);
    let product_values = fold_value_series(product_holdings, product_prices, &axis);
    let points: Vec<CollectionValuePoint> = axis
        .into_iter()
        .zip(card_values)
        .zip(product_values)
        .map(
            |((day, value_usd), sealed_value_usd)| CollectionValuePoint {
                date: day.to_string(),
                value_usd,
                sealed_value_usd,
            },
        )
        .collect();

    downsample_rows(points, bucket_days, |p| p.date.as_str())
}

/// Value one holding kind over a caller-provided union date axis. Each item carries its
/// latest price forward across days where only the other kind captured a snapshot.
fn fold_value_series(
    holdings: &[HoldingRow],
    prices: &HashMap<i32, Vec<PriceCell>>,
    axis: &[&str],
) -> Vec<Option<String>> {
    let cells: Vec<&[PriceCell]> = holdings
        .iter()
        .map(|h| prices.get(&h.item_id).map_or(&[][..], Vec::as_slice))
        .collect();
    let mut cursors: Vec<Cursor> = vec![Cursor::default(); holdings.len()];

    let mut values = Vec::with_capacity(axis.len());
    for &day in axis {
        let mut total_cents: i128 = 0;
        let mut any_priced = false;
        for (i, holding) in holdings.iter().enumerate() {
            let cursor = &mut cursors[i];
            let rows = cells[i];
            // Advance to the latest snapshot on or before `day`, carrying its price forward.
            while cursor.pos < rows.len() && rows[cursor.pos].date.as_str() <= day {
                cursor.usd = rows[cursor.pos].usd_cents;
                cursor.foil = rows[cursor.pos].foil_cents;
                cursor.pos += 1;
            }
            if let Some(cents) = cursor.usd {
                total_cents += cents * i128::from(holding.quantity);
                any_priced = true;
            }
            if let Some(cents) = cursor.foil {
                total_cents += cents * i128::from(holding.foil_quantity);
                any_priced = true;
            }
        }
        values.push(any_priced.then(|| format_cents(total_cents)));
    }
    values
}

#[cfg(test)]
fn fold_value_history(
    holdings: &[HoldingRow],
    prices: &HashMap<i32, Vec<PriceCell>>,
    bucket_days: i64,
) -> Vec<CollectionValuePoint> {
    fold_collection_value_history(holdings, prices, &[], &HashMap::new(), bucket_days, None)
}

/// Per-holding carry-forward cursor: how far we've advanced into its snapshots, and the
/// last-seen regular/foil price in cents.
#[derive(Default, Clone)]
struct Cursor {
    pos: usize,
    usd: Option<i128>,
    foil: Option<i128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(date: &str, usd: Option<&str>, foil: Option<&str>) -> PriceCell {
        PriceCell {
            date: date.to_string(),
            usd_cents: price_cents(usd),
            foil_cents: price_cents(foil),
        }
    }

    fn holding(item_id: i32, quantity: i32, foil_quantity: i32) -> HoldingRow {
        HoldingRow {
            item_id,
            quantity,
            foil_quantity,
        }
    }

    fn values(points: &[CollectionValuePoint]) -> Vec<(&str, Option<&str>)> {
        points
            .iter()
            .map(|p| (p.date.as_str(), p.value_usd.as_deref()))
            .collect()
    }

    #[test]
    fn empty_when_no_holdings() {
        let points = fold_value_history(&[], &HashMap::new(), 1);
        assert!(points.is_empty());
    }

    #[test]
    fn empty_when_holdings_have_no_price_history() {
        let holdings = vec![holding(1, 3, 0)];
        // No prices at all -> no date axis -> no points.
        let points = fold_value_history(&holdings, &HashMap::new(), 1);
        assert!(points.is_empty());
    }

    #[test]
    fn revalues_current_holding_across_its_entire_price_history() {
        let holdings = vec![holding(1, 2, 0)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("10.00"), None),
                cell("2024-01-02", Some("10.00"), None),
                cell("2024-01-03", Some("12.00"), None),
                cell("2024-01-04", Some("12.00"), None),
            ],
        );
        let points = fold_value_history(&holdings, &prices, 1);
        assert_eq!(
            values(&points),
            vec![
                ("2024-01-01", Some("20.00")),
                ("2024-01-02", Some("20.00")),
                ("2024-01-03", Some("24.00")), // 12.00 × 2
                ("2024-01-04", Some("24.00")),
            ],
        );
    }

    #[test]
    fn carries_price_forward_over_missing_days() {
        // Card 1 has no snapshot on the 2nd; card 2 does, so the 2nd is on the axis and
        // card 1 must carry its last price ($5) forward rather than dropping out.
        let holdings = vec![
            holding(1, 1, 0),
            holding(2, 1, 0),
        ];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("5.00"), None),
                cell("2024-01-03", Some("8.00"), None),
            ],
        );
        prices.insert(
            2,
            vec![
                cell("2024-01-01", Some("1.00"), None),
                cell("2024-01-02", Some("1.00"), None),
                cell("2024-01-03", Some("1.00"), None),
            ],
        );
        let points = fold_value_history(&holdings, &prices, 1);
        assert_eq!(
            values(&points),
            vec![
                ("2024-01-01", Some("6.00")), // 5 + 1
                ("2024-01-02", Some("6.00")), // 5 carried + 1
                ("2024-01-03", Some("9.00")), // 8 + 1
            ],
        );
    }

    #[test]
    fn values_regular_and_foil_copies_separately() {
        let holdings = vec![holding(1, 1, 2)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", Some("3.00"), Some("10.00"))]);
        let points = fold_value_history(&holdings, &prices, 1);
        // 3.00 × 1 regular + 10.00 × 2 foil = 23.00
        assert_eq!(values(&points), vec![("2024-01-01", Some("23.00"))]);
    }

    #[test]
    fn unpriced_card_does_not_gate_the_day() {
        // Card 1 is unpriced on the 1st; card 2 is priced, so the day is a real total, not
        // a null — one unpriced holding must not blank the whole collection's value.
        let holdings = vec![
            holding(1, 1, 0),
            holding(2, 1, 0),
        ];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", None, None)]);
        prices.insert(2, vec![cell("2024-01-01", Some("4.00"), None)]);
        let points = fold_value_history(&holdings, &prices, 1);
        assert_eq!(values(&points), vec![("2024-01-01", Some("4.00"))]);
    }

    #[test]
    fn downsamples_wide_windows_to_one_point_per_bucket() {
        // Three daily points inside one weekly bucket collapse to the newest.
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("1.00"), None),
                cell("2024-01-02", Some("2.00"), None),
                cell("2024-01-03", Some("3.00"), None),
            ],
        );
        let points = fold_value_history(&holdings, &prices, 7);
        assert_eq!(values(&points), vec![("2024-01-03", Some("3.00"))]);
    }

    #[test]
    fn card_and_sealed_lines_share_the_union_axis_and_carry_forward() {
        let card_holdings = vec![holding(1, 2, 0)];
        let product_holdings = vec![holding(1, 1, 0)];
        let mut card_prices = HashMap::new();
        card_prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("3.00"), None),
                cell("2024-01-03", Some("4.00"), None),
            ],
        );
        let mut product_prices = HashMap::new();
        product_prices.insert(1, vec![cell("2024-01-02", Some("20.00"), None)]);

        let points = fold_collection_value_history(
            &card_holdings,
            &card_prices,
            &product_holdings,
            &product_prices,
            1,
            None,
        );
        let values: Vec<_> = points
            .iter()
            .map(|p| {
                (
                    p.date.as_str(),
                    p.value_usd.as_deref(),
                    p.sealed_value_usd.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            values,
            vec![
                ("2024-01-01", Some("6.00"), None),
                ("2024-01-02", Some("6.00"), Some("20.00")),
                ("2024-01-03", Some("8.00"), Some("20.00")),
            ]
        );
    }

    #[test]
    fn range_floor_seeds_sparse_series_from_its_pre_cutoff_anchor() {
        let product_holdings = vec![holding(1, 1, 0)];
        let mut product_prices = HashMap::new();
        product_prices.insert(1, vec![cell("2024-01-02", Some("25.00"), None)]);

        let points = fold_collection_value_history(
            &[],
            &HashMap::new(),
            &product_holdings,
            &product_prices,
            1,
            Some("2024-01-05"),
        );

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].date, "2024-01-05");
        assert_eq!(points[0].sealed_value_usd.as_deref(), Some("25.00"));
    }
}
