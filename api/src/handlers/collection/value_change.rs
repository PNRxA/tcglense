//! Collection daily movement: how much the signed-in user's **whole basket** moved between
//! the two most recent daily price captures — the headline delta the landing shows beside its
//! total value, and beside the cards' and sealed products' own totals.
//!
//! Where [`super::price_movements`] ranks the biggest *single-copy* moves and
//! [`super::value_history`] draws the basket's value over every captured day, this answers one
//! question per holding kind: *what is my collection worth at the latest capture, and how much
//! of that is the last day's movement?* Like both siblings it reconstructs the answer from the
//! daily card/product price snapshots and the user's **current** counts — there is no
//! per-holding quantity history, so the figure is "today's basket, re-priced", not "what I
//! owned yesterday".
//!
//! Per holding kind (cards, sealed products) the reference date `as_of` is the newest snapshot
//! across the user's held items of that kind — each kind has its own capture cadence, so the
//! two are anchored independently (mirroring the movers' per-kind `as_of`). The baseline is
//! the calendar day before it, carried forward: an item's price at the baseline is its latest
//! snapshot **at or before** that day, so a capture gap never blanks the figure. Both anchors
//! are gathered by the same per-item `LIMIT 1` point-seeks the movers use ([`SnapshotSeek`]) —
//! two tiny index descents per held item, never a scan of anyone's whole history.
//!
//! The fold is deliberately conservative about what counts as movement. A finish contributes
//! to `value_usd` whenever it is priced at `as_of`; it contributes to `change_usd` only when it
//! is priced at **both** anchors — a printing whose history began today is worth its price,
//! but it did not *gain* that price overnight (the movers apply the same both-anchors rule,
//! and the value-history chart, which shows every priced day, is the place to see such a step).
//! `previous_usd` is then `value_usd - change_usd`: what today's basket was worth a day earlier
//! with the newly-priced finishes held flat — so the three figures are always coherent by
//! construction, and `change_pct` is the movement over that baseline. All money math is
//! integer cents; `f64` is used only for the reported percentage.

use std::collections::HashMap;

use axum::extract::State;
use chrono::{Duration, NaiveDate};
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect};
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
use crate::extract::Path;
use crate::handlers::shared::require_game;
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::scryfall::format_date;
use crate::state::AppState;

use super::price_movements::{PRICE_ID_CHUNK, SnapshotSeek, decode_snapshot, format_signed_cents};

/// The collection's movement since the previous daily price capture, per holding kind and
/// rolled up. Each kind is anchored to its own newest snapshot (`as_of`) — cards and sealed
/// products are captured on independent cadences.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionValueChange {
    /// The card holdings' movement, anchored to the newest captured card snapshot.
    pub cards: ValueChange,
    /// The sealed-product holdings' movement, anchored to the newest captured product snapshot.
    pub sealed: ValueChange,
    /// Cards + sealed products rolled together: the sums of the two kinds' figures, each
    /// measured to its own `as_of`; this `as_of` is the later of the two.
    pub total: ValueChange,
}

/// One holding kind's (or the rolled-up basket's) value at the latest capture and its
/// movement since the day before. Every field is `null` when nothing of that kind is held
/// or nothing held has captured price history.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ValueChange {
    /// The reference ("as of") date the figures are measured to — the newest snapshot date
    /// across the held items of this kind, `"YYYY-MM-DD"`. The baseline is the calendar day
    /// before it, carried forward across capture gaps.
    pub as_of: Option<String>,
    /// The holdings' total USD value at `as_of` (every finish priced that day), 2-dp string.
    pub value_usd: Option<String>,
    /// What today's basket was worth at the baseline: `value_usd - change_usd`, so a finish
    /// first priced at `as_of` is held flat rather than read as a gain. 2-dp USD string.
    pub previous_usd: Option<String>,
    /// The day's movement — `Σ (price_now - price_prev) × copies` over every held finish
    /// priced at **both** anchors — as a signed 2-dp USD string (`"-3.50"` for a loss,
    /// `"0.00"` for an unchanged capture). `null` when no held finish has a baseline price
    /// (a single captured day), even if `value_usd` is set.
    pub change_usd: Option<String>,
    /// `change_usd / previous_usd × 100`. `null` when `change_usd` is, or the baseline is 0.
    pub change_pct: Option<f64>,
}

impl ValueChange {
    /// Nothing held, or nothing held has any captured price history.
    fn empty() -> Self {
        Self {
            as_of: None,
            value_usd: None,
            previous_usd: None,
            change_usd: None,
            change_pct: None,
        }
    }
}

/// Get collection value change
///
/// `GET /api/collection/{game}/value-change` -> how much the signed-in user's collection
/// moved since the previous daily price capture: the card, sealed-product and rolled-up
/// values at the latest captured day, the day-earlier baseline, and the signed difference.
/// Each kind is anchored to its own newest snapshot; a finish counts toward the movement only
/// when it is priced at both anchors, so a newly-priced printing never reads as a gain.
/// `404` if the game is unknown; all-`null` figures when the user owns nothing or no owned
/// item has captured price history.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/value-change",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The collection's value at the latest daily price capture and its movement since the day before, for cards, sealed products and both together (the current basket re-priced — quantity history is not reconstructed). All fields null when nothing owned has captured price history.", body = CollectionValueChange),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn collection_value_change(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;

    // Version-keyed, single-flight response cache (issues #413/#365), like the sibling
    // analytics reads: between the user's own edits and the daily price capture this
    // response cannot change. `None` key = cache degraded, compute as normal.
    let cache_key = state
        .analytics_cache
        .body_key(user.id, &game, "value-change", "")
        .await;
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, user, game) = (state.clone(), user.clone(), game.clone());
            async move {
                let payload = value_change_payload(state, user, game).await?;
                serde_json::to_vec(&payload)
                    .map_err(|err| AppError::Internal(format!("serialize value change: {err}")))
            }
        })
        .await?;
    Ok(json_body_response(body))
}

/// Compute the payload (the handler above wraps this in the analytics response cache).
async fn value_change_payload(
    state: AppState,
    user: crate::entities::user::Model,
    game: String,
) -> Result<CollectionValueChange, AppError> {
    // The user's current card + sealed holdings, reduced to ids/counts — the counts scale each
    // finish's price into a held value (unlike the movers, this *is* a quantity-weighted read:
    // it answers what the whole basket did, not what one copy did).
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

    let to_holding = |(item_id, quantity, foil_quantity): (i32, i32, i32)| HoldingRow {
        item_id,
        quantity,
        foil_quantity,
    };
    let card_holdings: Vec<HoldingRow> = card_holdings.into_iter().map(to_holding).collect();
    let product_holdings: Vec<HoldingRow> = product_holdings.into_iter().map(to_holding).collect();

    // Each kind's reference date: the newest snapshot across its held items (a cheap
    // `MAX(as_of_date)` per id chunk on the history index, as the movers do). `None` = this
    // kind has nothing priced, so its figures are all null.
    let card_latest =
        latest_snapshot_date(&state, &game, &card_holdings, HistoryTable::Cards).await?;
    let product_latest =
        latest_snapshot_date(&state, &game, &product_holdings, HistoryTable::Products).await?;

    let cards = match &card_latest {
        None => ValueChange::empty(),
        Some(latest) => {
            let baseline = previous_day(latest)?;
            let seek = SnapshotSeek::new(
                CardPriceHistory,
                card_price_history::Column::Game,
                card_price_history::Column::CardId,
                card_price_history::Column::AsOfDate,
                card_price_history::Column::PriceUsd,
                card_price_history::Column::PriceUsdFoil,
                (collection_item::Entity, collection_item::Column::CardId),
                &game,
            );
            // One row per held card carrying its two anchors, driven from the holdings table:
            // the newest snapshot (the `as_of` value) and the newest at or before the baseline.
            let rows = CollectionItem::find()
                .select_only()
                .column_as(collection_item::Column::CardId, "item_id")
                .expr_as(seek.latest(), "latest")
                .expr_as(seek.at_or_before(&baseline), "baseline")
                .filter(collection_item::Column::UserId.eq(user.id))
                .filter(collection_item::Column::Game.eq(game.as_str()))
                .into_model::<AnchorSnapshots>()
                .all(&state.db)
                .await?;
            fold_value_change(&card_holdings, &decode_anchors(rows)?, latest)
        }
    };

    let sealed = match &product_latest {
        None => ValueChange::empty(),
        Some(latest) => {
            let baseline = previous_day(latest)?;
            let seek = SnapshotSeek::new(
                ProductPriceHistory,
                product_price_history::Column::Game,
                product_price_history::Column::ProductId,
                product_price_history::Column::AsOfDate,
                product_price_history::Column::PriceUsd,
                product_price_history::Column::PriceUsdFoil,
                (
                    collection_product_item::Entity,
                    collection_product_item::Column::ProductId,
                ),
                &game,
            );
            let rows = CollectionProductItem::find()
                .select_only()
                .column_as(collection_product_item::Column::ProductId, "item_id")
                .expr_as(seek.latest(), "latest")
                .expr_as(seek.at_or_before(&baseline), "baseline")
                .filter(collection_product_item::Column::UserId.eq(user.id))
                .filter(collection_product_item::Column::Game.eq(game.as_str()))
                .into_model::<AnchorSnapshots>()
                .all(&state.db)
                .await?;
            fold_value_change(&product_holdings, &decode_anchors(rows)?, latest)
        }
    };

    let total = roll_up(&cards, &sealed);
    Ok(CollectionValueChange {
        cards,
        sealed,
        total,
    })
}

/// Which price-history table a reference-date lookup reads.
#[derive(Clone, Copy)]
enum HistoryTable {
    Cards,
    Products,
}

/// The newest captured snapshot date across the held items of one kind, or `None` when none
/// of them has any history (or nothing is held). Chunked so the `IN (...)` never exceeds the
/// bound-parameter cap; each chunk is a `MAX` over the history index.
async fn latest_snapshot_date(
    state: &AppState,
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
                    .one(&state.db)
                    .await?
            }
            HistoryTable::Products => {
                ProductPriceHistory::find()
                    .select_only()
                    .column_as(product_price_history::Column::AsOfDate.max(), "latest")
                    .filter(product_price_history::Column::Game.eq(game))
                    .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()))
                    .into_tuple::<Option<String>>()
                    .one(&state.db)
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

/// The calendar day before a stored `"YYYY-MM-DD"` snapshot date — the carry-forward
/// baseline target. A malformed date is an internal invariant failure (these dates are
/// ours), so it surfaces as a 500.
fn previous_day(latest: &str) -> Result<String, AppError> {
    let date = NaiveDate::parse_from_str(latest, "%Y-%m-%d")
        .map_err(|e| AppError::Internal(format!("unparseable snapshot date {latest:?}: {e}")))?;
    Ok(format_date(date - Duration::days(1)))
}

/// A holding reduced to what the fold needs: the item and its current counts.
struct HoldingRow {
    item_id: i32,
    quantity: i32,
    foil_quantity: i32,
}

/// One held item's two anchor snapshots as the compact `date|usd|foil` strings the
/// point-seeks return. `latest` is `None` for a held item with no captured history at all
/// (skipped); `baseline` is `None` when nothing was captured at or before the baseline day.
#[derive(FromQueryResult)]
struct AnchorSnapshots {
    item_id: i32,
    latest: Option<String>,
    baseline: Option<String>,
}

impl AnchorSnapshots {
    /// Decode both anchors to integer cents, or `None` for an item with no history.
    fn decode(self) -> Result<Option<(i32, ItemAnchors)>, AppError> {
        let Some(latest) = self.latest else {
            return Ok(None);
        };
        let (_, usd, foil) = decode_snapshot(&latest)?;
        let now = FinishPrices {
            usd: price_cents(usd.as_deref()),
            foil: price_cents(foil.as_deref()),
        };
        let prev = match self.baseline {
            None => FinishPrices::default(),
            Some(baseline) => {
                let (_, usd, foil) = decode_snapshot(&baseline)?;
                FinishPrices {
                    usd: price_cents(usd.as_deref()),
                    foil: price_cents(foil.as_deref()),
                }
            }
        };
        Ok(Some((self.item_id, ItemAnchors { now, prev })))
    }
}

/// Decode one kind's anchor rows into a per-item map, skipping held items with no history.
fn decode_anchors(rows: Vec<AnchorSnapshots>) -> Result<HashMap<i32, ItemAnchors>, AppError> {
    let mut anchors = HashMap::with_capacity(rows.len());
    for row in rows {
        if let Some((item_id, item)) = row.decode()? {
            anchors.insert(item_id, item);
        }
    }
    Ok(anchors)
}

/// An item's regular/foil prices at one anchor, in integer cents (`None` = unpriced).
#[derive(Debug, Default, Clone, Copy)]
struct FinishPrices {
    usd: Option<i128>,
    foil: Option<i128>,
}

/// An item's prices at the two anchors: its newest snapshot and its carry-forward baseline.
#[derive(Debug, Default, Clone, Copy)]
struct ItemAnchors {
    now: FinishPrices,
    prev: FinishPrices,
}

/// Fold one holding kind's anchors into its [`ValueChange`]. See the module docs for the
/// rules: value over every finish priced now; movement over every finish priced at both
/// anchors; `previous = value - change`.
fn fold_value_change(
    holdings: &[HoldingRow],
    anchors: &HashMap<i32, ItemAnchors>,
    as_of: &str,
) -> ValueChange {
    let mut value_cents: i128 = 0;
    let mut change_cents: i128 = 0;
    let mut any_priced = false;
    let mut any_compared = false;

    let mut fold_finish = |now: Option<i128>, prev: Option<i128>, copies: i32| {
        if copies <= 0 {
            return;
        }
        let Some(now) = now else {
            return;
        };
        any_priced = true;
        value_cents += now * i128::from(copies);
        if let Some(prev) = prev {
            any_compared = true;
            change_cents += (now - prev) * i128::from(copies);
        }
    };
    for holding in holdings {
        let Some(item) = anchors.get(&holding.item_id) else {
            continue;
        };
        fold_finish(item.now.usd, item.prev.usd, holding.quantity);
        fold_finish(item.now.foil, item.prev.foil, holding.foil_quantity);
    }

    shape_change(
        Some(as_of.to_string()),
        any_priced.then_some(value_cents),
        any_compared.then_some(change_cents),
    )
}

/// Shape cents into the wire figures. `value` is `None` when nothing is priced, `change`
/// when nothing could be compared; `previous`/`pct` derive from the two.
fn shape_change(as_of: Option<String>, value: Option<i128>, change: Option<i128>) -> ValueChange {
    let previous = match (value, change) {
        (Some(value), Some(change)) => Some(value - change),
        _ => None,
    };
    let change_pct = match (change, previous) {
        (Some(change), Some(previous)) if previous > 0 => {
            Some(change as f64 / previous as f64 * 100.0)
        }
        _ => None,
    };
    ValueChange {
        as_of,
        value_usd: value.map(format_cents),
        previous_usd: previous.map(format_cents),
        change_usd: change.map(format_signed_cents),
        change_pct,
    }
}

/// Roll the two kinds into one basket-wide figure: values and changes sum where present
/// (a kind with nothing priced simply contributes nothing), `as_of` is the later of the two.
fn roll_up(cards: &ValueChange, sealed: &ValueChange) -> ValueChange {
    let sum = |a: Option<i128>, b: Option<i128>| match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    };
    // The kinds' 2-dp strings are this module's own output, so re-parsing them is exact.
    let value = sum(
        price_cents(cards.value_usd.as_deref()),
        price_cents(sealed.value_usd.as_deref()),
    );
    let change = sum(
        price_cents(cards.change_usd.as_deref()),
        price_cents(sealed.change_usd.as_deref()),
    );
    let as_of = match (&cards.as_of, &sealed.as_of) {
        (Some(a), Some(b)) => Some(if a >= b { a.clone() } else { b.clone() }),
        (a, b) => a.clone().or_else(|| b.clone()),
    };
    shape_change(as_of, value, change)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(item_id: i32, quantity: i32, foil_quantity: i32) -> HoldingRow {
        HoldingRow {
            item_id,
            quantity,
            foil_quantity,
        }
    }

    fn prices(usd: Option<&str>, foil: Option<&str>) -> FinishPrices {
        FinishPrices {
            usd: price_cents(usd),
            foil: price_cents(foil),
        }
    }

    fn anchors(now: FinishPrices, prev: FinishPrices) -> ItemAnchors {
        ItemAnchors { now, prev }
    }

    #[test]
    fn fold_weights_each_finish_by_its_copies_and_signs_the_change() {
        let holdings = vec![holding(1, 2, 1), holding(2, 1, 0)];
        let mut map = HashMap::new();
        // Card 1: regular 10 -> 12 (×2 = +4.00), foil 30 -> 25 (×1 = -5.00).
        map.insert(
            1,
            anchors(
                prices(Some("12.00"), Some("25.00")),
                prices(Some("10.00"), Some("30.00")),
            ),
        );
        // Card 2: regular 3.25 -> 3.00 (-0.25); the unowned foil finish is ignored even though
        // it moved.
        map.insert(
            2,
            anchors(
                prices(Some("3.00"), Some("99.00")),
                prices(Some("3.25"), Some("1.00")),
            ),
        );
        let change = fold_value_change(&holdings, &map, "2026-09-22");
        assert_eq!(change.as_of.as_deref(), Some("2026-09-22"));
        assert_eq!(change.value_usd.as_deref(), Some("52.00"), "24 + 25 + 3");
        assert_eq!(change.change_usd.as_deref(), Some("-1.25"), "+4 - 5 - 0.25");
        assert_eq!(change.previous_usd.as_deref(), Some("53.25"));
        let pct = change.change_pct.expect("pct");
        assert!((pct - (-125.0 / 5325.0 * 100.0)).abs() < 1e-9, "{pct}");
    }

    #[test]
    fn a_finish_first_priced_today_counts_toward_value_but_not_movement() {
        let holdings = vec![holding(1, 1, 0), holding(2, 3, 0)];
        let mut map = HashMap::new();
        // Card 1 has both anchors: 10 -> 11.
        map.insert(
            1,
            anchors(prices(Some("11.00"), None), prices(Some("10.00"), None)),
        );
        // Card 2's history began today: no baseline price, so its $100 × 3 is value, not gain.
        map.insert(2, anchors(prices(Some("100.00"), None), prices(None, None)));
        let change = fold_value_change(&holdings, &map, "2026-09-22");
        assert_eq!(change.value_usd.as_deref(), Some("311.00"));
        assert_eq!(change.change_usd.as_deref(), Some("1.00"));
        // The newly-priced card is held flat in the baseline: 311 - 1.
        assert_eq!(change.previous_usd.as_deref(), Some("310.00"));
        let pct = change.change_pct.expect("pct");
        assert!((pct - (100.0 / 31000.0 * 100.0)).abs() < 1e-9, "{pct}");
    }

    #[test]
    fn no_baseline_anywhere_reports_a_value_but_a_null_change() {
        let holdings = vec![holding(1, 1, 0)];
        let mut map = HashMap::new();
        map.insert(1, anchors(prices(Some("5.00"), None), prices(None, None)));
        let change = fold_value_change(&holdings, &map, "2026-09-22");
        assert_eq!(change.value_usd.as_deref(), Some("5.00"));
        assert_eq!(
            change.change_usd, None,
            "a single captured day has no movement"
        );
        assert_eq!(change.previous_usd, None);
        assert_eq!(change.change_pct, None);
    }

    #[test]
    fn unpriced_now_contributes_nothing_and_an_unchanged_capture_is_zero_not_null() {
        let holdings = vec![holding(1, 2, 0), holding(2, 1, 0), holding(3, 1, 0)];
        let mut map = HashMap::new();
        // Card 1: priced yesterday, unpriced today -> nothing (no value, no movement).
        map.insert(1, anchors(prices(None, None), prices(Some("4.00"), None)));
        // Card 2: flat capture.
        map.insert(
            2,
            anchors(prices(Some("7.00"), None), prices(Some("7.00"), None)),
        );
        // Card 3: held but never captured — absent from the anchors map entirely.
        let change = fold_value_change(&holdings, &map, "2026-09-22");
        assert_eq!(change.value_usd.as_deref(), Some("7.00"));
        assert_eq!(change.change_usd.as_deref(), Some("0.00"));
        assert_eq!(change.previous_usd.as_deref(), Some("7.00"));
        assert_eq!(change.change_pct, Some(0.0));
    }

    #[test]
    fn nothing_priced_is_all_null() {
        let holdings = vec![holding(1, 1, 0)];
        let change = fold_value_change(&holdings, &HashMap::new(), "2026-09-22");
        // The reference date is still reported (the kind *has* history, just not for a priced
        // finish), but every figure is null.
        assert_eq!(change.as_of.as_deref(), Some("2026-09-22"));
        assert_eq!(change.value_usd, None);
        assert_eq!(change.change_usd, None);
        assert_eq!(change.previous_usd, None);
        assert_eq!(change.change_pct, None);
    }

    #[test]
    fn pct_is_null_when_the_baseline_is_zero() {
        let holdings = vec![holding(1, 1, 0)];
        let mut map = HashMap::new();
        map.insert(
            1,
            anchors(prices(Some("2.00"), None), prices(Some("0.00"), None)),
        );
        let change = fold_value_change(&holdings, &map, "2026-09-22");
        assert_eq!(change.change_usd.as_deref(), Some("2.00"));
        assert_eq!(change.previous_usd.as_deref(), Some("0.00"));
        assert_eq!(change.change_pct, None);
    }

    #[test]
    fn roll_up_sums_the_kinds_and_takes_the_later_as_of() {
        let cards = shape_change(Some("2026-09-22".into()), Some(10_000), Some(-250));
        let sealed = shape_change(Some("2026-09-21".into()), Some(5_000), Some(1_000));
        let total = roll_up(&cards, &sealed);
        assert_eq!(total.as_of.as_deref(), Some("2026-09-22"));
        assert_eq!(total.value_usd.as_deref(), Some("150.00"));
        assert_eq!(total.change_usd.as_deref(), Some("7.50"));
        assert_eq!(total.previous_usd.as_deref(), Some("142.50"));
        let pct = total.change_pct.expect("pct");
        assert!((pct - (750.0 / 14250.0 * 100.0)).abs() < 1e-9, "{pct}");
    }

    #[test]
    fn roll_up_with_one_empty_kind_is_the_other_kind() {
        let cards = shape_change(Some("2026-09-22".into()), Some(10_000), None);
        let sealed = ValueChange::empty();
        let total = roll_up(&cards, &sealed);
        assert_eq!(total, cards);

        let neither = roll_up(&ValueChange::empty(), &ValueChange::empty());
        assert_eq!(neither, ValueChange::empty());
    }

    #[test]
    fn roll_up_a_kind_without_a_baseline_holds_it_flat() {
        // Cards have a movement; sealed has a value but no baseline yet. The total's change is
        // the cards' alone and the sealed value is carried flat into the baseline.
        let cards = shape_change(Some("2026-09-22".into()), Some(10_000), Some(500));
        let sealed = shape_change(Some("2026-09-22".into()), Some(2_000), None);
        let total = roll_up(&cards, &sealed);
        assert_eq!(total.value_usd.as_deref(), Some("120.00"));
        assert_eq!(total.change_usd.as_deref(), Some("5.00"));
        assert_eq!(total.previous_usd.as_deref(), Some("115.00"));
    }

    #[test]
    fn previous_day_steps_back_one_calendar_day() {
        assert_eq!(previous_day("2026-03-01").unwrap(), "2026-02-28");
        assert_eq!(previous_day("2026-01-01").unwrap(), "2025-12-31");
        assert!(previous_day("not-a-date").is_err());
    }
}
