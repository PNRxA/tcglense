//! Collection price movements: the biggest **gain and loss movements** across the cards a
//! signed-in user owns, from one day through all captured history.
//!
//! A "movement" is the change in the USD value of the user's *holding* of a card over a
//! window — a card's per-unit price change × the quantity owned, summed over the regular
//! and foil finishes. Cards are ranked by that value change: top gainers (largest positive)
//! and top losers (largest negative), for seven windows (1d / 7d / 30d / 1y / 2y / 3y /
//! all time), all measured back from the most recent snapshot date across the user's priced
//! holdings. Fixed windows carry the most recent price at or before their baseline; all time
//! uses each finish's own earliest captured non-null price so a newer printing is compared
//! across all of *its* available history rather than being excluded by a global start date.
//!
//! Like [`super::value_history`], this reconstructs everything from the daily
//! `card_price_history` snapshots (keyed by the same internal `card_id` a holding stores)
//! plus the user's *current* counts — there's no per-holding quantity history, so a card's
//! today counts are used at both window anchors. A finish contributes to a window only when
//! both anchors are priced (else the delta would be bogus), and a card counts as a mover
//! only when its total value actually moved. All money math is integer cents; f64 is used
//! only for the reported percentage.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use chrono::{Duration, NaiveDate};
use sea_orm::sea_query::{Expr, Func, SimpleExpr};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{Card, CardPriceHistory, CollectionItem};
use crate::entities::{card, card_price_history, collection_item};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::require_game;
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::handlers::shared::CardResponse;
use crate::scryfall::format_date;
use crate::state::AppState;

/// How many card ids to bind per `IN (...)` chunk — kept well under SQLite's
/// bound-parameter cap so an arbitrarily large collection still fetches in a handful of
/// queries (mirrors [`super::value_history`]).
const PRICE_ID_CHUNK: usize = 10_000;

/// How many cards' exact anchor-date predicates to OR into one snapshot fetch. Each card
/// binds its id plus at most nine dates, so 80 stays below SQLite's conservative 999-bind
/// floor even after the game value is included.
const PRICE_ANCHOR_CHUNK: usize = 80;

/// How many movers to return per direction, per window.
const TOP_N: usize = 5;

/// The biggest gain/loss movements across a user's collection, for each supported window.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionMovers {
    /// The reference ("as of") date the movements are measured to: the most recent
    /// snapshot date across the user's priced holdings, `"YYYY-MM-DD"`. `None` when no
    /// owned card has any captured price history (all lists then empty).
    pub as_of: Option<String>,
    pub day: CollectionMoverList,
    pub week: CollectionMoverList,
    pub month: CollectionMoverList,
    pub year: CollectionMoverList,
    pub two_year: CollectionMoverList,
    pub three_year: CollectionMoverList,
    pub all_time: CollectionMoverList,
}

/// The ranked movers for one window: the top gainers and the top losers.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionMoverList {
    pub gainers: Vec<CollectionMover>,
    pub losers: Vec<CollectionMover>,
}

/// One card's movement: the card, the counts held, and the value change of that holding.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionMover {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
    /// Current holding value over the finishes comparable at both anchors, 2-dp USD string.
    pub value_now: String,
    /// Holding value at the window baseline over the same finishes, 2-dp USD string.
    pub value_prev: String,
    /// `value_now - value_prev`, signed 2-dp USD string (negative for a loss, e.g. `"-3.50"`).
    pub change_usd: String,
    /// Percent change = change / value_prev * 100. `None` when `value_prev` is 0.
    pub change_pct: Option<f64>,
}

impl CollectionMovers {
    /// The all-empty response (no holdings, or no captured price history at all).
    fn empty() -> Self {
        Self {
            as_of: None,
            day: CollectionMoverList::empty(),
            week: CollectionMoverList::empty(),
            month: CollectionMoverList::empty(),
            year: CollectionMoverList::empty(),
            two_year: CollectionMoverList::empty(),
            three_year: CollectionMoverList::empty(),
            all_time: CollectionMoverList::empty(),
        }
    }
}

impl CollectionMoverList {
    fn empty() -> Self {
        Self {
            gainers: Vec::new(),
            losers: Vec::new(),
        }
    }
}

/// List collection movers
///
/// `GET /api/collection/{game}/movers` -> the signed-in user's biggest gain/loss movements
/// over the 1d / 7d / 30d / 1y / 2y / 3y / all-time windows. `404` if the game is unknown;
/// an all-empty `{ "as_of": null, ... }` when the user owns nothing or no owned card has
/// captured price history. No query params — the windows and top-N are fixed.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/movers",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The user's biggest gain/loss movements over the 1d / 7d / 30d / 1y / 2y / 3y / all-time windows (all-empty when nothing owned or no captured price history).", body = CollectionMovers),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn collection_movers(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionMovers>, AppError> {
    require_game(&game)?;

    // The user's current holdings: just the card + its counts (movements value today's
    // counts at both window anchors — there's no per-holding quantity history to honour).
    // The (user_id, game, card_id) unique index scopes this to one user's cards.
    let holdings: Vec<(i32, i32, i32)> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .into_tuple()
        .all(&state.db)
        .await?;

    if holdings.is_empty() {
        return Ok(Json(CollectionMovers::empty()));
    }

    let holdings: Vec<HoldingRow> = holdings
        .into_iter()
        .map(|(card_id, quantity, foil_quantity)| HoldingRow {
            card_id,
            quantity,
            foil_quantity,
        })
        .collect();

    let card_ids: Vec<i32> = holdings.iter().map(|h| h.card_id).collect();

    // Find the collection-wide reference date first. The aggregate stays inside the
    // (game, card_id, date) index and returns one scalar per id chunk, regardless of how
    // much history has accumulated.
    let mut latest: Option<String> = None;
    for chunk in card_ids.chunks(PRICE_ID_CHUNK) {
        let chunk_latest = CardPriceHistory::find()
            .select_only()
            .column_as(card_price_history::Column::AsOfDate.max(), "latest")
            .filter(card_price_history::Column::Game.eq(game.as_str()))
            .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()))
            .into_tuple::<Option<String>>()
            .one(&state.db)
            .await?
            .flatten();
        if let Some(candidate) = chunk_latest
            && latest.as_ref().map_or(true, |current| candidate.as_str() > current.as_str())
        {
            latest = Some(candidate);
        }
    }
    let Some(latest) = latest else {
        return Ok(Json(CollectionMovers::empty()));
    };

    // The reference date is DB-sourced and always well-formed; a parse failure is an
    // internal invariant break, not a client error (there's no `From<ParseError>` for
    // `AppError`, so map it explicitly rather than leaking a bare `?`).
    let latest_date = NaiveDate::parse_from_str(&latest, "%Y-%m-%d")
        .map_err(|e| AppError::Internal(format!("unparseable snapshot date {latest:?}: {e}")))?;

    let day_target = format_date(latest_date - Duration::days(1));
    let week_target = format_date(latest_date - Duration::days(7));
    let month_target = format_date(latest_date - Duration::days(30));
    let year_target = format_date(latest_date - Duration::days(365));
    let two_year_target = format_date(latest_date - Duration::days(730));
    let three_year_target = format_date(latest_date - Duration::days(1095));

    // Aggregate the exact snapshot dates needed per card: the latest row, the last row at
    // or before each fixed baseline, and the first non-null row for each finish. The database
    // scans the covering index for the all-time minima, but only these compact date anchors
    // cross the wire — never the complete daily series.
    let mut anchors: Vec<PriceAnchorDates> = Vec::new();
    for chunk in card_ids.chunks(PRICE_ID_CHUNK) {
        let rows = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::CardId)
            .column_as(card_price_history::Column::AsOfDate.max(), "latest")
            .column_as(last_date_at_or_before(&day_target), "day")
            .column_as(last_date_at_or_before(&week_target), "week")
            .column_as(last_date_at_or_before(&month_target), "month")
            .column_as(last_date_at_or_before(&year_target), "year")
            .column_as(last_date_at_or_before(&two_year_target), "two_year")
            .column_as(last_date_at_or_before(&three_year_target), "three_year")
            .column_as(first_priced_date(card_price_history::Column::PriceUsd), "first_usd")
            .column_as(
                first_priced_date(card_price_history::Column::PriceUsdFoil),
                "first_foil",
            )
            .filter(card_price_history::Column::Game.eq(game.as_str()))
            .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()))
            .group_by(card_price_history::Column::CardId)
            .into_model::<PriceAnchorDates>()
            .all(&state.db)
            .await?;
        anchors.extend(rows);
    }

    // Fetch only rows that match those per-card anchors. A card has at most nine distinct
    // dates, so the handler retains O(cards × windows) price cells instead of O(cards × days).
    let mut price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();
    for chunk in anchors.chunks(PRICE_ANCHOR_CHUNK) {
        let mut anchor_filter = Condition::any();
        for anchor in chunk {
            anchor_filter = anchor_filter.add(
                Condition::all()
                    .add(card_price_history::Column::CardId.eq(anchor.card_id))
                    .add(card_price_history::Column::AsOfDate.is_in(anchor.dates())),
            );
        }
        let rows = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::CardId)
            .column(card_price_history::Column::AsOfDate)
            .column(card_price_history::Column::PriceUsd)
            .column(card_price_history::Column::PriceUsdFoil)
            .filter(card_price_history::Column::Game.eq(game.as_str()))
            .filter(anchor_filter)
            .order_by_asc(card_price_history::Column::CardId)
            .order_by_asc(card_price_history::Column::AsOfDate)
            .into_tuple::<(i32, String, Option<String>, Option<String>)>()
            .all(&state.db)
            .await?;
        price_rows.extend(rows);
    }

    // Group each card's snapshots (already ascending by date) and parse the decimal-string
    // prices to integer cents once, up front.
    let mut prices: HashMap<i32, Vec<PriceCell>> = HashMap::new();
    for (card_id, date, usd, foil) in price_rows {
        prices.entry(card_id).or_default().push(PriceCell {
            date,
            usd_cents: price_cents(usd.as_deref()),
            foil_cents: price_cents(foil.as_deref()),
        });
    }

    let day = window_movers(&holdings, &prices, &latest, &day_target, TOP_N);
    let week = window_movers(&holdings, &prices, &latest, &week_target, TOP_N);
    let month = window_movers(&holdings, &prices, &latest, &month_target, TOP_N);
    let year = window_movers(&holdings, &prices, &latest, &year_target, TOP_N);
    let two_year = window_movers(&holdings, &prices, &latest, &two_year_target, TOP_N);
    let three_year = window_movers(&holdings, &prices, &latest, &three_year_target, TOP_N);
    let all_time = all_time_movers(&holdings, &prices, &latest, TOP_N);

    // The union of every card that survived into any final list. A card can rank in several
    // windows (and as a gainer in one, a loser in another), so de-duplicate before fetching.
    let mut needed: HashSet<i32> = HashSet::new();
    for (gainers, losers) in [&day, &week, &month, &year, &two_year, &three_year, &all_time] {
        for raw in gainers.iter().chain(losers) {
            needed.insert(raw.card_id);
        }
    }

    // Fetch just those cards, keyed by the internal id the raw movers carry (capture the id
    // before `From` consumes the model). A raw mover whose card row is gone (a catalog
    // re-import dropped it) is skipped when shaped below — a slightly short list is correct;
    // we deliberately don't backfill.
    let cards: HashMap<i32, CardResponse> = if needed.is_empty() {
        HashMap::new()
    } else {
        Card::find()
            .filter(card::Column::Game.eq(game.as_str()))
            .filter(card::Column::Id.is_in(needed.iter().copied()))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|model| {
                let id = model.id;
                (id, CardResponse::from(model))
            })
            .collect()
    };

    Ok(Json(CollectionMovers {
        as_of: Some(latest),
        day: shape_window(day, &cards),
        week: shape_window(week, &cards),
        month: shape_window(month, &cards),
        year: shape_window(year, &cards),
        two_year: shape_window(two_year, &cards),
        three_year: shape_window(three_year, &cards),
        all_time: shape_window(all_time, &cards),
    }))
}

/// The exact history dates needed to rank one card across every response window. Fixed
/// baselines use the most recent row on/before their target; `first_*` can differ because a
/// finish may start receiving prices later than its sibling.
#[derive(FromQueryResult)]
struct PriceAnchorDates {
    card_id: i32,
    latest: Option<String>,
    day: Option<String>,
    week: Option<String>,
    month: Option<String>,
    year: Option<String>,
    two_year: Option<String>,
    three_year: Option<String>,
    first_usd: Option<String>,
    first_foil: Option<String>,
}

impl PriceAnchorDates {
    /// Sorted, de-duplicated anchor dates for the exact-row fetch.
    fn dates(&self) -> Vec<String> {
        let mut dates: Vec<String> = [
            self.latest.as_ref(),
            self.day.as_ref(),
            self.week.as_ref(),
            self.month.as_ref(),
            self.year.as_ref(),
            self.two_year.as_ref(),
            self.three_year.as_ref(),
            self.first_usd.as_ref(),
            self.first_foil.as_ref(),
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        dates.sort_unstable();
        dates.dedup();
        dates
    }
}

/// A holding reduced to what the ranking needs: the card and its current counts.
struct HoldingRow {
    card_id: i32,
    quantity: i32,
    foil_quantity: i32,
}

/// One card's snapshot for a day: the date and its regular/foil price already in integer
/// cents (`None` = unpriced that day, so it contributes nothing).
struct PriceCell {
    date: String,
    usd_cents: Option<i128>,
    foil_cents: Option<i128>,
}

/// A ranked movement before it's dressed with the card payload: the counts held and the
/// window's value change, all in integer cents.
struct RawMover {
    card_id: i32,
    quantity: i32,
    foil_quantity: i32,
    value_now_cents: i128,
    value_prev_cents: i128,
    change_cents: i128,
    change_pct: Option<f64>,
}

/// SQL expression for the most recent history date at or before a fixed target. `CASE`
/// makes rows after the target null; `MAX` then performs the carry-forward lookup per card.
fn last_date_at_or_before(target: &str) -> SimpleExpr {
    Func::max(Expr::case(
        card_price_history::Column::AsOfDate.lte(target),
        Expr::col(card_price_history::Column::AsOfDate),
    ))
    .into()
}

/// SQL expression for the earliest history date on which `price_column` is non-null.
fn first_priced_date(price_column: card_price_history::Column) -> SimpleExpr {
    Func::min(Expr::case(
        price_column.is_not_null(),
        Expr::col(card_price_history::Column::AsOfDate),
    ))
    .into()
}

/// Rank the holdings by their value change between `target` (the window baseline) and
/// `latest` (the reference date), returning `(gainers, losers)` — gainers sorted by change
/// descending, losers by change ascending (most negative first), each capped at `top_n`.
///
/// Per holding, each finish (regular at `quantity`, foil at `foil_quantity`) contributes
/// **only** when both anchors have a snapshot, that finish is priced at both, and its
/// quantity is positive — so an unpriced-at-one-anchor finish never fabricates a delta. A
/// card is a mover only when at least one finish contributed *and* its total value actually
/// changed. Ties break toward the higher current value, then the lower card id, so the
/// order is fully deterministic.
fn window_movers(
    holdings: &[HoldingRow],
    prices: &HashMap<i32, Vec<PriceCell>>,
    latest: &str,
    target: &str,
    top_n: usize,
) -> (Vec<RawMover>, Vec<RawMover>) {
    rank_movers(holdings, prices, latest, Some(target), top_n)
}

/// Rank movements across every captured price for each finish. Unlike a fixed window, an
/// all-time comparison has no shared calendar anchor: a card first printed last year should
/// still participate even when another owned card has ten years of history. Each finish
/// therefore uses its own earliest non-null captured price as the baseline.
fn all_time_movers(
    holdings: &[HoldingRow],
    prices: &HashMap<i32, Vec<PriceCell>>,
    latest: &str,
    top_n: usize,
) -> (Vec<RawMover>, Vec<RawMover>) {
    rank_movers(holdings, prices, latest, None, top_n)
}

/// Shared ranker for fixed-window (`target = Some(date)`) and all-time (`target = None`)
/// movement. Fixed windows take both baseline finish prices from the last snapshot at or
/// before `target`; all time finds the first non-null baseline independently per finish.
fn rank_movers(
    holdings: &[HoldingRow],
    prices: &HashMap<i32, Vec<PriceCell>>,
    latest: &str,
    target: Option<&str>,
    top_n: usize,
) -> (Vec<RawMover>, Vec<RawMover>) {
    let mut movers: Vec<RawMover> = Vec::new();

    for holding in holdings {
        let Some(cells) = prices.get(&holding.card_id) else {
            continue;
        };
        // The most recent snapshot at the common `latest` anchor, carried forward across a
        // missing day for this card.
        let Some(now_cell) = priced_at(cells, latest) else {
            continue;
        };
        let (prev_usd, prev_foil) = if let Some(target) = target {
            let Some(prev_cell) = priced_at(cells, target) else {
                continue;
            };
            (prev_cell.usd_cents, prev_cell.foil_cents)
        } else {
            (
                cells.iter().find_map(|cell| cell.usd_cents),
                cells.iter().find_map(|cell| cell.foil_cents),
            )
        };

        let mut value_now_cents: i128 = 0;
        let mut value_prev_cents: i128 = 0;
        let mut change_cents: i128 = 0;
        let mut contributed = false;

        for (now, prev, qty) in [
            (now_cell.usd_cents, prev_usd, holding.quantity),
            (now_cell.foil_cents, prev_foil, holding.foil_quantity),
        ] {
            if qty > 0
                && let (Some(now), Some(prev)) = (now, prev)
            {
                let q = i128::from(qty);
                value_now_cents += now * q;
                value_prev_cents += prev * q;
                change_cents += (now - prev) * q;
                contributed = true;
            }
        }

        if !contributed || change_cents == 0 {
            continue;
        }

        let change_pct = if value_prev_cents != 0 {
            Some(change_cents as f64 / value_prev_cents as f64 * 100.0)
        } else {
            None
        };

        movers.push(RawMover {
            card_id: holding.card_id,
            quantity: holding.quantity,
            foil_quantity: holding.foil_quantity,
            value_now_cents,
            value_prev_cents,
            change_cents,
            change_pct,
        });
    }

    // `movers` holds only non-zero changes, so `> 0` cleanly splits gainers from losers.
    let (mut gainers, mut losers): (Vec<RawMover>, Vec<RawMover>) =
        movers.into_iter().partition(|m| m.change_cents > 0);

    gainers.sort_by(|a, b| {
        b.change_cents
            .cmp(&a.change_cents)
            .then(b.value_now_cents.cmp(&a.value_now_cents))
            .then(a.card_id.cmp(&b.card_id))
    });
    gainers.truncate(top_n);

    losers.sort_by(|a, b| {
        a.change_cents
            .cmp(&b.change_cents)
            .then(b.value_now_cents.cmp(&a.value_now_cents))
            .then(a.card_id.cmp(&b.card_id))
    });
    losers.truncate(top_n);

    (gainers, losers)
}

/// The latest snapshot on or before `on_or_before` (cells are ascending by date, so scan
/// from the end). `None` when the card has no snapshot at or before that date — i.e. the
/// window's baseline predates the card's earliest captured price.
fn priced_at<'a>(cells: &'a [PriceCell], on_or_before: &str) -> Option<&'a PriceCell> {
    cells.iter().rev().find(|c| c.date.as_str() <= on_or_before)
}

/// Dress each raw mover with its card payload, dropping any whose card row is missing (a
/// catalog re-import gap) — a shorter list is acceptable and correct.
fn shape_movers(raws: Vec<RawMover>, cards: &HashMap<i32, CardResponse>) -> Vec<CollectionMover> {
    raws.into_iter()
        .filter_map(|raw| {
            cards.get(&raw.card_id).map(|card| CollectionMover {
                card: card.clone(),
                quantity: raw.quantity,
                foil_quantity: raw.foil_quantity,
                value_now: format_cents(raw.value_now_cents),
                value_prev: format_cents(raw.value_prev_cents),
                change_usd: format_signed_cents(raw.change_cents),
                change_pct: raw.change_pct,
            })
        })
        .collect()
}

/// Dress both sides of one raw window with their card payloads.
fn shape_window(
    (gainers, losers): (Vec<RawMover>, Vec<RawMover>),
    cards: &HashMap<i32, CardResponse>,
) -> CollectionMoverList {
    CollectionMoverList {
        gainers: shape_movers(gainers, cards),
        losers: shape_movers(losers, cards),
    }
}

/// Format a signed cent delta as a 2-dp USD string that always carries a leading `-` for a
/// negative value — including the `(-100, 0)` range where [`format_cents`] alone drops the
/// sign (its dollar part is a signless zero, so `-50` would render as `"0.50"`).
fn format_signed_cents(cents: i128) -> String {
    if cents < 0 {
        format!("-{}", format_cents(-cents))
    } else {
        format_cents(cents)
    }
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

    fn holding(card_id: i32, quantity: i32, foil_quantity: i32) -> HoldingRow {
        HoldingRow {
            card_id,
            quantity,
            foil_quantity,
        }
    }

    fn ids(movers: &[RawMover]) -> Vec<i32> {
        movers.iter().map(|m| m.card_id).collect()
    }

    fn changes(movers: &[RawMover]) -> Vec<i128> {
        movers.iter().map(|m| m.change_cents).collect()
    }

    #[test]
    fn gainers_desc_losers_asc_partitioned_by_sign() {
        // A single 1d window (latest 01-10, baseline 01-09). Each card holds one regular
        // copy; only the price differs, so the value change is the price change.
        let holdings = vec![holding(1, 1, 0), holding(2, 1, 0), holding(3, 1, 0), holding(4, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-09", Some("10.00"), None), cell("2024-01-10", Some("15.00"), None)]);
        prices.insert(2, vec![cell("2024-01-09", Some("10.00"), None), cell("2024-01-10", Some("12.00"), None)]);
        prices.insert(3, vec![cell("2024-01-09", Some("10.00"), None), cell("2024-01-10", Some("7.00"), None)]);
        prices.insert(4, vec![cell("2024-01-09", Some("10.00"), None), cell("2024-01-10", Some("4.00"), None)]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-10", "2024-01-09", TOP_N);
        // Gainers, biggest first: card 1 (+$5), card 2 (+$2).
        assert_eq!(ids(&gainers), vec![1, 2]);
        assert_eq!(changes(&gainers), vec![500, 200]);
        // Losers, most negative first: card 4 (-$6), card 3 (-$3).
        assert_eq!(ids(&losers), vec![4, 3]);
        assert_eq!(changes(&losers), vec![-600, -300]);
    }

    #[test]
    fn card_without_a_baseline_snapshot_is_excluded() {
        // Both of the card's snapshots are *after* the baseline (01-03), so it has no
        // price at the window's start -> no honest delta -> excluded.
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-05", Some("5.00"), None), cell("2024-01-10", Some("9.00"), None)]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-10", "2024-01-03", TOP_N);
        assert!(gainers.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn finish_unpriced_at_one_anchor_is_excluded() {
        // The regular finish is unpriced at the baseline but priced now: no delta can be
        // formed, so the card contributes nothing (and, foil-less, drops out entirely).
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", None, None), cell("2024-01-02", Some("10.00"), None)]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        assert!(gainers.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn regular_and_foil_contribute_quantity_weighted() {
        // Card 1: 2 regular + 3 foil. Regular $1->$2 (×2 = +$2), foil $2->$5 (×3 = +$9).
        // Card 2: foil-only, 2 copies, $3->$4 (×2 = +$2).
        let holdings = vec![holding(1, 2, 3), holding(2, 0, 2)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", Some("1.00"), Some("2.00")), cell("2024-01-02", Some("2.00"), Some("5.00"))]);
        prices.insert(2, vec![cell("2024-01-01", None, Some("3.00")), cell("2024-01-02", None, Some("4.00"))]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        assert!(losers.is_empty());
        assert_eq!(ids(&gainers), vec![1, 2]);
        // Card 1: value_now = 2×2 + 5×3 = $19; value_prev = 1×2 + 2×3 = $8; change = +$11.
        assert_eq!(gainers[0].value_now_cents, 1900);
        assert_eq!(gainers[0].value_prev_cents, 800);
        assert_eq!(gainers[0].change_cents, 1100);
        // Card 2 (foil-only): value_now = 4×2 = $8; value_prev = 3×2 = $6; change = +$2.
        assert_eq!(gainers[1].value_now_cents, 800);
        assert_eq!(gainers[1].value_prev_cents, 600);
        assert_eq!(gainers[1].change_cents, 200);
    }

    #[test]
    fn zero_change_card_is_excluded() {
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", Some("5.00"), None), cell("2024-01-02", Some("5.00"), None)]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        assert!(gainers.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn top_n_cap_keeps_the_biggest() {
        // Seven gainers of increasing magnitude; only the five biggest survive the cap.
        let holdings: Vec<HoldingRow> = (1..=7).map(|k| holding(k, 1, 0)).collect();
        let mut prices = HashMap::new();
        for k in 1..=7 {
            let now = format!("{}.00", 1 + k); // card k gains exactly $k.
            prices.insert(k, vec![cell("2024-01-01", Some("1.00"), None), cell("2024-01-02", Some(&now), None)]);
        }

        let (gainers, _losers) = window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        assert_eq!(gainers.len(), 5);
        // The five biggest, descending: cards 7,6,5,4,3.
        assert_eq!(ids(&gainers), vec![7, 6, 5, 4, 3]);
        assert_eq!(changes(&gainers), vec![700, 600, 500, 400, 300]);
    }

    #[test]
    fn change_pct_carries_sign_and_is_none_when_baseline_is_zero() {
        // Card 1: $10 -> $15, +50%. Card 2: $10 -> $5, -50%. Card 3: $0 -> $3, baseline
        // value 0 so the percentage is undefined (None) even though the card moved.
        let holdings = vec![holding(1, 1, 0), holding(2, 1, 0), holding(3, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(1, vec![cell("2024-01-01", Some("10.00"), None), cell("2024-01-02", Some("15.00"), None)]);
        prices.insert(2, vec![cell("2024-01-01", Some("10.00"), None), cell("2024-01-02", Some("5.00"), None)]);
        prices.insert(3, vec![cell("2024-01-01", Some("0.00"), None), cell("2024-01-02", Some("3.00"), None)]);

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        // Gainers: card 1 (+50%) then card 3 (zero-baseline, None).
        assert_eq!(ids(&gainers), vec![1, 3]);
        assert_eq!(gainers[0].change_pct, Some(50.0));
        assert_eq!(gainers[1].value_prev_cents, 0);
        assert_eq!(gainers[1].change_pct, None);
        // Loser: card 2 at -50%.
        assert_eq!(ids(&losers), vec![2]);
        assert_eq!(losers[0].change_pct, Some(-50.0));
    }

    #[test]
    fn carry_forward_uses_the_latest_snapshot_before_the_baseline() {
        // No snapshot exactly on the baseline (01-06); the 01-05 price ($6) is carried
        // forward, and `latest` (01-10) reads the 01-10 price ($9): change = +$3.
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("4.00"), None),
                cell("2024-01-05", Some("6.00"), None),
                cell("2024-01-10", Some("9.00"), None),
            ],
        );

        // priced_at picks the right cells directly.
        let cells = &prices[&1];
        assert_eq!(priced_at(cells, "2024-01-06").unwrap().usd_cents, Some(600));
        assert_eq!(priced_at(cells, "2024-01-10").unwrap().usd_cents, Some(900));

        let (gainers, losers) = window_movers(&holdings, &prices, "2024-01-10", "2024-01-06", TOP_N);
        assert!(losers.is_empty());
        assert_eq!(ids(&gainers), vec![1]);
        assert_eq!(gainers[0].change_cents, 300);
    }

    #[test]
    fn all_time_uses_each_finish_earliest_non_null_price() {
        // Regular history begins on 01-01 while foil starts on 01-02. All time compares
        // each finish to its own honest first price: regular $1->$4 and foil $3->$5.
        let holdings = vec![holding(1, 1, 1)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("1.00"), None),
                cell("2024-01-02", Some("2.00"), Some("3.00")),
                cell("2024-01-03", Some("4.00"), Some("5.00")),
            ],
        );

        let (gainers, losers) = all_time_movers(&holdings, &prices, "2024-01-03", TOP_N);
        assert!(losers.is_empty());
        assert_eq!(ids(&gainers), vec![1]);
        assert_eq!(gainers[0].value_prev_cents, 400);
        assert_eq!(gainers[0].value_now_cents, 900);
        assert_eq!(gainers[0].change_cents, 500);
    }

    #[test]
    fn format_signed_cents_signs_negatives_including_sub_dollar() {
        assert_eq!(format_signed_cents(1234), "12.34");
        assert_eq!(format_signed_cents(0), "0.00");
        assert_eq!(format_signed_cents(-350), "-3.50");
        // The case bare `format_cents` gets wrong: a sub-dollar loss keeps its sign.
        assert_eq!(format_signed_cents(-50), "-0.50");
        assert_eq!(format_signed_cents(-5), "-0.05");
    }
}
