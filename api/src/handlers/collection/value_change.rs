//! Collection value movement: how much the signed-in user's **whole basket** moved over a
//! window — a day, a week, a month, one to three years, or all captured history — the
//! headline delta the landing shows beside its total value, and beside the cards' and sealed
//! products' own totals.
//!
//! Where [`super::price_movements`] ranks the biggest *single-copy* moves and
//! [`super::value_history`] draws the basket's value over every captured day, this answers one
//! question per holding kind: *what is my collection worth at the latest capture, and how much
//! of that is the window's movement?* Like both siblings it reconstructs the answer from the
//! daily card/product price snapshots and the user's **current** counts — there is no
//! per-holding quantity history, so the figure is "today's basket, re-priced", not "what I
//! owned back then".
//!
//! Per holding kind (cards, sealed products) the reference date `as_of` is the newest snapshot
//! across the user's held items of that kind — each kind has its own capture cadence, so the
//! two are anchored independently (mirroring the movers' per-kind `as_of`). The window picks
//! the baseline exactly as the movers do ([`WindowTargets`]): a fixed window's baseline is the
//! calendar day `as_of - N`, carried forward — an item's baseline price is its latest snapshot
//! **at or before** that day, so a capture gap never blanks the figure — and all-time compares
//! each finish with its own earliest captured non-null price, so a newer printing is measured
//! across all of *its* history rather than excluded by an older item's. Every anchor is a
//! per-item `LIMIT 1` point-seek ([`SnapshotSeek`]) — two tiny index descents per held item
//! (three for all-time, whose `first_priced` walk is the movers' honest non-point-seek too),
//! never a scan of anyone's whole history.
//!
//! The fold is deliberately conservative about what counts as movement. A finish contributes
//! to `value_usd` whenever its newest snapshot prices it (that snapshot is at or before `as_of`
//! by construction — an item whose feed stopped is carried forward at its last capture, as the
//! movers and the chart carry it); it contributes to `change_usd` only when it is priced at
//! **both** anchors — a printing whose history began inside the window is worth its price,
//! but it did not *gain* that price within the window (the movers apply the same both-anchors
//! rule, and the value-history chart, which shows every priced day, is the place to see such
//! a step). `previous_usd` is then `value_usd - change_usd`: what today's basket was worth at
//! the baseline with the newly-priced finishes held flat — so the three figures are always
//! coherent by construction, and `change_pct` is the movement over that baseline. All money
//! math is integer cents; `f64` is used only for the reported percentage.

use std::collections::HashMap;

use axum::extract::State;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};

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
use crate::handlers::shared::require_game;
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::state::AppState;

use super::analytics_inputs::{
    HistoryTable, HoldingRow, latest_snapshot_date, load_card_holdings, load_product_holdings,
};
use super::price_movements::{
    MoverWindowSel, SnapshotSeek, WindowTargets, decode_snapshot, format_signed_cents,
    null_snapshot,
};

/// The window the landing shows when the user has not picked one: a week. Daily moves are
/// often tiny or empty on a young collection, while longer windows hide the news — the same
/// reasoning as the movers panel's default.
const DEFAULT_WINDOW: MoverWindowSel = MoverWindowSel::Week;

/// Query params for [`collection_value_change`]: an optional `window`. Parsed as an optional
/// string, not a typed enum, so an unknown value becomes our JSON `422` via
/// [`MoverWindowSel::parse`] rather than axum's default query rejection.
#[derive(Debug, Deserialize)]
pub struct ValueChangeParams {
    pub window: Option<String>,
}

/// The collection's movement over a window, per holding kind and rolled up. Each kind is
/// anchored to its own newest snapshot (`as_of`) — cards and sealed products are captured on
/// independent cadences.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionValueChange {
    /// The window the figures cover, echoed as its wire token
    /// (`day`/`week`/`month`/`year`/`two_year`/`three_year`/`all_time`).
    pub window: String,
    /// The card holdings' movement, anchored to the newest captured card snapshot.
    pub cards: ValueChange,
    /// The sealed-product holdings' movement, anchored to the newest captured product snapshot.
    pub sealed: ValueChange,
    /// Cards + sealed products rolled together: the sums of the two kinds' figures, each
    /// measured to its own `as_of`; this `as_of` is the later of the kinds that contributed.
    pub total: ValueChange,
}

/// One holding kind's (or the rolled-up basket's) value at the latest capture and its
/// movement over the window. Every field is `null` when nothing of that kind is held
/// or nothing held has captured price history.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ValueChange {
    /// The reference ("as of") date the figures are measured to — the newest snapshot date
    /// across the held items of this kind, `"YYYY-MM-DD"`. A fixed window's baseline is the
    /// calendar day `as_of - N` carried forward across capture gaps; all-time's is each
    /// finish's own earliest captured price.
    pub as_of: Option<String>,
    /// The holdings' total USD value at `as_of`, each held finish carried forward from its
    /// newest snapshot at or before that day (an item whose feed stopped keeps its last
    /// captured price, as in the chart and the movers), 2-dp string.
    pub value_usd: Option<String>,
    /// What today's basket was worth at the baseline: `value_usd - change_usd`, so a finish
    /// first priced inside the window is held flat rather than read as a gain. 2-dp USD string.
    pub previous_usd: Option<String>,
    /// The window's movement — `Σ (price_now - price_prev) × copies` over every held finish
    /// priced at **both** anchors — as a signed 2-dp USD string (`"-3.50"` for a loss,
    /// `"0.00"` for an unchanged capture). `null` when no held finish has a baseline price
    /// (history shorter than the window), even if `value_usd` is set.
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
/// `GET /api/collection/{game}/value-change?window=` -> how much the signed-in user's
/// collection moved over a window: the card, sealed-product and rolled-up values at the
/// latest captured day, the baseline, and the signed difference. `window` takes the movers'
/// tokens (`day`/`week`/`month`/`year`/`two_year`/`three_year`/`all_time`) and defaults to
/// `week`. Each kind is anchored to its own newest snapshot; a fixed window's baseline is the
/// day `as_of - N` carried forward, all-time's each finish's earliest captured price; a finish
/// counts toward the movement only when it is priced at both anchors, so a newly-priced
/// printing never reads as a gain. `404` if the game is unknown; `422` for an unknown
/// `window`; all-`null` figures when the user owns nothing or no owned item has captured
/// price history.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/value-change",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("window" = Option<String>, Query, description = "The movement window (`day`/`week`/`month`/`year`/`two_year`/`three_year`/`all_time`); absent = `week`. A fixed window measures from the newest snapshot back N calendar days (carried forward across capture gaps); `all_time` measures each finish from its own earliest captured price."),
    ),
    responses(
        (status = 200, description = "The collection's value at the latest daily price capture and its movement over the window, for cards, sealed products and both together (the current basket re-priced — quantity history is not reconstructed). All fields null when nothing owned has captured price history.", body = CollectionValueChange),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown `window` value."),
    ),
)]
pub async fn collection_value_change(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ValueChangeParams>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;

    // Blank/absent `window` -> the week default; a value picks the window. Unknown values
    // are a 422, mirroring the movers' `window` and value-history's `range`.
    let window = match params
        .window
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => DEFAULT_WINDOW,
        Some(value) => MoverWindowSel::parse(value)?,
    };

    // Version-keyed, single-flight response cache (issues #413/#365), like the sibling
    // analytics reads: between the user's own edits and the daily price capture this
    // response cannot change. The window token is the params segment, so each window caches
    // independently. `None` key = cache degraded, compute as normal.
    let cache_key = state
        .analytics_cache
        .body_key(user.id, &game, "value-change", window.token())
        .await;
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, user, game) = (state.clone(), user.clone(), game.clone());
            async move {
                let payload = value_change_payload(state, user, game, window).await?;
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
    window: MoverWindowSel,
) -> Result<CollectionValueChange, AppError> {
    // The user's current card + sealed holdings, reduced to ids/counts (the shared analytics
    // preamble) — the counts scale each finish's price into a held value (unlike the movers,
    // this *is* a quantity-weighted read: it answers what the whole basket did, not what one
    // copy did).
    let card_holdings = load_card_holdings(&state.db, user.id, &game).await?;
    let product_holdings = load_product_holdings(&state.db, user.id, &game).await?;

    // Each kind's reference date: the newest snapshot across its held items, as the movers
    // anchor. `None` = this kind has nothing captured, so its figures are all null.
    let card_latest =
        latest_snapshot_date(&state.db, &game, &card_holdings, HistoryTable::Cards).await?;
    let product_latest =
        latest_snapshot_date(&state.db, &game, &product_holdings, HistoryTable::Products).await?;

    let cards = match &card_latest {
        None => ValueChange::empty(),
        Some(latest) => {
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
            let anchors = BaselineAnchors::for_window(
                &seek,
                window,
                latest,
                card_price_history::Column::PriceUsd,
                card_price_history::Column::PriceUsdFoil,
            )?;
            // One row per held card carrying its anchors, driven from the holdings table: the
            // newest snapshot (the `as_of` value) plus the window's baseline — the newest at
            // or before the target day, or each finish's first priced row for all-time (the
            // unused columns are literal NULLs, never evaluated).
            let rows = CollectionItem::find()
                .select_only()
                .column_as(collection_item::Column::CardId, "item_id")
                .expr_as(seek.latest(), "latest")
                .expr_as(anchors.baseline, "baseline")
                .expr_as(anchors.first_usd, "first_usd")
                .expr_as(anchors.first_foil, "first_foil")
                .filter(collection_item::Column::UserId.eq(user.id))
                .filter(collection_item::Column::Game.eq(game.as_str()))
                .into_model::<AnchorSnapshots>()
                .all(&state.db)
                .await?;
            fold_value_change(&card_holdings, &decode_anchors(rows, window)?, latest)
        }
    };

    let sealed = match &product_latest {
        None => ValueChange::empty(),
        Some(latest) => {
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
            let anchors = BaselineAnchors::for_window(
                &seek,
                window,
                latest,
                product_price_history::Column::PriceUsd,
                product_price_history::Column::PriceUsdFoil,
            )?;
            let rows = CollectionProductItem::find()
                .select_only()
                .column_as(collection_product_item::Column::ProductId, "item_id")
                .expr_as(seek.latest(), "latest")
                .expr_as(anchors.baseline, "baseline")
                .expr_as(anchors.first_usd, "first_usd")
                .expr_as(anchors.first_foil, "first_foil")
                .filter(collection_product_item::Column::UserId.eq(user.id))
                .filter(collection_product_item::Column::Game.eq(game.as_str()))
                .into_model::<AnchorSnapshots>()
                .all(&state.db)
                .await?;
            fold_value_change(&product_holdings, &decode_anchors(rows, window)?, latest)
        }
    };

    let total = roll_up(&cards, &sealed);
    Ok(CollectionValueChange {
        window: window.token().to_string(),
        cards,
        sealed,
        total,
    })
}

/// The baseline sub-selects one window needs, as scalar expressions for `expr_as`. A fixed
/// window seeks the newest row at or before its calendar target and leaves the two all-time
/// columns as literal NULLs; all-time does the reverse, seeking each finish's first priced row
/// (two seeks, since the regular and foil prices can begin on different days).
struct BaselineAnchors {
    baseline: SimpleExpr,
    first_usd: SimpleExpr,
    first_foil: SimpleExpr,
}

impl BaselineAnchors {
    fn for_window<U, F>(
        seek: &SnapshotSeek,
        window: MoverWindowSel,
        latest: &str,
        usd_col: U,
        foil_col: F,
    ) -> Result<Self, AppError>
    where
        U: sea_orm::sea_query::IntoColumnRef,
        F: sea_orm::sea_query::IntoColumnRef,
    {
        if window == MoverWindowSel::AllTime {
            return Ok(Self {
                baseline: null_snapshot(),
                first_usd: seek.first_priced(usd_col),
                first_foil: seek.first_priced(foil_col),
            });
        }
        let targets = WindowTargets::from_latest(latest)?;
        let target = match window {
            MoverWindowSel::Day => &targets.day,
            MoverWindowSel::Week => &targets.week,
            MoverWindowSel::Month => &targets.month,
            MoverWindowSel::Year => &targets.year,
            MoverWindowSel::TwoYear => &targets.two_year,
            MoverWindowSel::ThreeYear => &targets.three_year,
            MoverWindowSel::AllTime => unreachable!("handled above"),
        };
        Ok(Self {
            baseline: seek.at_or_before(target),
            first_usd: null_snapshot(),
            first_foil: null_snapshot(),
        })
    }
}

/// One held item's anchor snapshots as the compact `date|usd|foil` strings the point-seeks
/// return. `latest` is `None` for a held item with no captured history at all (skipped);
/// `baseline` (a fixed window) is `None` when nothing was captured at or before the target
/// day; `first_usd`/`first_foil` (all-time) are `None` when that finish was never priced.
/// Whichever columns the window did not select read back as `None` too.
#[derive(FromQueryResult)]
struct AnchorSnapshots {
    item_id: i32,
    latest: Option<String>,
    baseline: Option<String>,
    first_usd: Option<String>,
    first_foil: Option<String>,
}

impl AnchorSnapshots {
    /// Decode the anchors to integer cents, or `None` for an item with no history. For a fixed
    /// window both baseline finishes come off the one baseline row; for all-time each finish
    /// takes its own first-priced row (only that finish's price is read from it).
    fn decode(self, window: MoverWindowSel) -> Result<Option<(i32, ItemAnchors)>, AppError> {
        let Some(latest) = self.latest else {
            return Ok(None);
        };
        let now = FinishPrices::decode(&latest)?;
        let prev = if window == MoverWindowSel::AllTime {
            FinishPrices {
                usd: self
                    .first_usd
                    .as_deref()
                    .map(FinishPrices::decode)
                    .transpose()?
                    .and_then(|p| p.usd),
                foil: self
                    .first_foil
                    .as_deref()
                    .map(FinishPrices::decode)
                    .transpose()?
                    .and_then(|p| p.foil),
            }
        } else {
            match self.baseline.as_deref() {
                None => FinishPrices::default(),
                Some(baseline) => FinishPrices::decode(baseline)?,
            }
        };
        Ok(Some((self.item_id, ItemAnchors { now, prev })))
    }
}

/// Decode one kind's anchor rows into a per-item map, skipping held items with no history.
fn decode_anchors(
    rows: Vec<AnchorSnapshots>,
    window: MoverWindowSel,
) -> Result<HashMap<i32, ItemAnchors>, AppError> {
    let mut anchors = HashMap::with_capacity(rows.len());
    for row in rows {
        if let Some((item_id, item)) = row.decode(window)? {
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

impl FinishPrices {
    /// Both finishes' prices off one encoded snapshot.
    fn decode(encoded: &str) -> Result<Self, AppError> {
        let (_, usd, foil) = decode_snapshot(encoded)?;
        Ok(Self {
            usd: price_cents(usd.as_deref()),
            foil: price_cents(foil.as_deref()),
        })
    }
}

/// An item's prices at the two anchors: its newest snapshot and its window baseline.
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
    // The reference date names the capture the *reported* figures are measured to, so only
    // a kind that contributed a value may donate it (a kind with history but nothing priced
    // carries an `as_of` and no figures); with neither contributing, keep whichever exists.
    let contributed = |k: &ValueChange| k.value_usd.is_some().then(|| k.as_of.clone()).flatten();
    let as_of = match (contributed(cards), contributed(sealed)) {
        (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => cards.as_of.clone().or_else(|| sealed.as_of.clone()),
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

    fn snapshot(date: &str, usd: Option<&str>, foil: Option<&str>) -> String {
        format!("{date}|{}|{}", usd.unwrap_or(""), foil.unwrap_or(""))
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
    fn a_finish_first_priced_inside_the_window_counts_toward_value_but_not_movement() {
        let holdings = vec![holding(1, 1, 0), holding(2, 3, 0)];
        let mut map = HashMap::new();
        // Card 1 has both anchors: 10 -> 11.
        map.insert(
            1,
            anchors(prices(Some("11.00"), None), prices(Some("10.00"), None)),
        );
        // Card 2's history began inside the window: no baseline price, so its $100 × 3 is
        // value, not gain.
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
            "history shorter than the window has no movement"
        );
        assert_eq!(change.previous_usd, None);
        assert_eq!(change.change_pct, None);
    }

    #[test]
    fn unpriced_now_contributes_nothing_and_an_unchanged_capture_is_zero_not_null() {
        let holdings = vec![holding(1, 2, 0), holding(2, 1, 0), holding(3, 1, 0)];
        let mut map = HashMap::new();
        // Card 1: priced at the baseline, unpriced now -> nothing (no value, no movement).
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
    fn a_fixed_window_decodes_both_finishes_off_the_one_baseline_row() {
        let row = AnchorSnapshots {
            item_id: 7,
            latest: Some(snapshot("2026-09-22", Some("12.00"), Some("25.00"))),
            baseline: Some(snapshot("2026-09-15", Some("10.00"), None)),
            first_usd: None,
            first_foil: None,
        };
        let (id, item) = row
            .decode(MoverWindowSel::Week)
            .expect("decode")
            .expect("has history");
        assert_eq!(id, 7);
        assert_eq!(item.now.usd, Some(1200));
        assert_eq!(item.now.foil, Some(2500));
        assert_eq!(item.prev.usd, Some(1000));
        assert_eq!(
            item.prev.foil, None,
            "unpriced at the baseline -> no comparison"
        );

        // No baseline row inside the window -> both previous finishes unpriced.
        let short = AnchorSnapshots {
            item_id: 8,
            latest: Some(snapshot("2026-09-22", Some("1.00"), None)),
            baseline: None,
            first_usd: None,
            first_foil: None,
        };
        let (_, item) = short.decode(MoverWindowSel::Day).unwrap().unwrap();
        assert_eq!(item.prev.usd, None);
        assert_eq!(item.prev.foil, None);

        // Never captured at all -> skipped.
        let none = AnchorSnapshots {
            item_id: 9,
            latest: None,
            baseline: None,
            first_usd: None,
            first_foil: None,
        };
        assert!(none.decode(MoverWindowSel::Day).unwrap().is_none());
    }

    #[test]
    fn all_time_decodes_each_finish_off_its_own_first_priced_row() {
        // The regular price began on 09-01 (when the foil was still unpriced) and the foil on
        // 09-10 (when the regular had already moved) — each finish reads only its own column
        // from its own first row, never the other's stale value.
        let row = AnchorSnapshots {
            item_id: 3,
            latest: Some(snapshot("2026-09-22", Some("12.00"), Some("25.00"))),
            baseline: None,
            first_usd: Some(snapshot("2026-09-01", Some("8.00"), None)),
            first_foil: Some(snapshot("2026-09-10", Some("9.50"), Some("20.00"))),
        };
        let (_, item) = row.decode(MoverWindowSel::AllTime).unwrap().unwrap();
        assert_eq!(item.prev.usd, Some(800));
        assert_eq!(item.prev.foil, Some(2000));

        // A finish never priced has no first row -> no comparison for it.
        let partial = AnchorSnapshots {
            item_id: 4,
            latest: Some(snapshot("2026-09-22", Some("3.00"), None)),
            baseline: None,
            first_usd: Some(snapshot("2026-09-22", Some("3.00"), None)),
            first_foil: None,
        };
        let (_, item) = partial.decode(MoverWindowSel::AllTime).unwrap().unwrap();
        assert_eq!(
            item.prev.usd,
            Some(300),
            "its own first row is today's -> flat"
        );
        assert_eq!(item.prev.foil, None);
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
    fn roll_up_never_takes_the_date_of_a_kind_that_contributed_nothing() {
        // Cards have history (so an `as_of`) but nothing priced; sealed carries the figures.
        // The total's date must be the sealed capture the figures were measured to, not the
        // newer card date that contributed no number.
        let cards = shape_change(Some("2026-09-22".into()), None, None);
        let sealed = shape_change(Some("2026-09-20".into()), Some(9_900), Some(900));
        let total = roll_up(&cards, &sealed);
        assert_eq!(total.as_of.as_deref(), Some("2026-09-20"));
        assert_eq!(total.value_usd.as_deref(), Some("99.00"));
        assert_eq!(total.change_usd.as_deref(), Some("9.00"));

        // Neither contributing: the date still survives (matching the per-kind shape).
        let neither = roll_up(&cards, &ValueChange::empty());
        assert_eq!(neither.as_of.as_deref(), Some("2026-09-22"));
        assert_eq!(neither.value_usd, None);
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
}
