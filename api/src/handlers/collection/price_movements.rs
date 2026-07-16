//! Collection price movements: the biggest **gain and loss movements** across the cards and
//! sealed products a signed-in user owns, from one day through all captured history.
//!
//! A "movement" is the change in the USD value of one current holding over a window — the
//! item's per-unit price change × quantity, summed over the regular and foil price columns.
//! Cards and sealed products are ranked independently by that value change, then selected in
//! the UI with a Singles / Sealed switch. Both expose seven windows (1d / 7d / 30d / 1y /
//! 2y / 3y / all time), measured back from the latest snapshot for that holding kind. Fixed
//! windows carry the most recent price at or before their baseline; all time
//! uses each finish's own earliest captured non-null price so a newer printing is compared
//! across all of *its* available history rather than being excluded by a global start date.
//!
//! Like [`super::value_history`], this reconstructs everything from the daily
//! card/product price snapshots (keyed by the same internal ids the holdings store)
//! plus the user's *current* counts — there's no per-holding quantity history, so an item's
//! today counts are used at both window anchors. A finish contributes to a window only when
//! both anchors are priced (else the delta would be bogus), and a card counts as a mover
//! only when its total value actually moved. All money math is integer cents; f64 is used
//! only for the reported percentage.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use chrono::{Duration, NaiveDate};
use sea_orm::sea_query::{BinOper, Expr, Func, IntoColumnRef, SimpleExpr};
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect};
use serde::Serialize;

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{
    Card, CardPriceHistory, CollectionItem, CollectionProductItem, Product, ProductPriceHistory,
};
use crate::entities::{
    card, card_price_history, collection_item, collection_product_item, product,
    product_price_history,
};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::CardResponse;
use crate::handlers::shared::product_holdings::{ProductResponse, product_response, set_name_map};
use crate::handlers::shared::require_game;
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::scryfall::format_date;
use crate::state::AppState;

/// How many card ids to bind per `IN (...)` chunk — kept well under SQLite's
/// bound-parameter cap so an arbitrarily large collection still fetches in a handful of
/// queries (mirrors [`super::value_history`]).
const PRICE_ID_CHUNK: usize = 10_000;

/// How many movers to return per direction, per window.
const TOP_N: usize = 5;

/// The biggest gain/loss movements across a user's collection, for each supported window.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionMovers {
    /// The reference ("as of") date the movements are measured to: the most recent
    /// snapshot date across the user's priced card holdings, `"YYYY-MM-DD"`. `None` when no
    /// owned card has any captured price history (all lists then empty).
    pub as_of: Option<String>,
    pub day: CollectionMoverList,
    pub week: CollectionMoverList,
    pub month: CollectionMoverList,
    pub year: CollectionMoverList,
    pub two_year: CollectionMoverList,
    pub three_year: CollectionMoverList,
    pub all_time: CollectionMoverList,
    /// Sealed-product movers kept separate from the backward-compatible card lists above.
    pub sealed: CollectionSealedMovers,
}

/// The ranked movers for one window: the top gainers and the top losers.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionMoverList {
    pub gainers: Vec<CollectionMover>,
    pub losers: Vec<CollectionMover>,
}

/// One card's movement, counts held, and holding-value change.
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

/// The sealed-product mover series, with the same windows as the card series.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSealedMovers {
    pub as_of: Option<String>,
    pub day: CollectionSealedMoverList,
    pub week: CollectionSealedMoverList,
    pub month: CollectionSealedMoverList,
    pub year: CollectionSealedMoverList,
    pub two_year: CollectionSealedMoverList,
    pub three_year: CollectionSealedMoverList,
    pub all_time: CollectionSealedMoverList,
}

/// The ranked sealed-product movers for one window.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSealedMoverList {
    pub gainers: Vec<CollectionSealedMover>,
    pub losers: Vec<CollectionSealedMover>,
}

/// One sealed product's movement, counts held, and holding-value change.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSealedMover {
    pub product: ProductResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
    pub value_now: String,
    pub value_prev: String,
    pub change_usd: String,
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
            sealed: CollectionSealedMovers::empty(),
        }
    }
}

impl CollectionSealedMovers {
    fn empty() -> Self {
        Self {
            as_of: None,
            day: CollectionSealedMoverList::empty(),
            week: CollectionSealedMoverList::empty(),
            month: CollectionSealedMoverList::empty(),
            year: CollectionSealedMoverList::empty(),
            two_year: CollectionSealedMoverList::empty(),
            three_year: CollectionSealedMoverList::empty(),
            all_time: CollectionSealedMoverList::empty(),
        }
    }
}

impl CollectionSealedMoverList {
    fn empty() -> Self {
        Self {
            gainers: Vec::new(),
            losers: Vec::new(),
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
/// an all-empty `{ "as_of": null, ... }` when the user owns nothing or no owned item has
/// captured price history. The existing card lists stay backward-compatible; sealed products
/// use the parallel `sealed` series. No query params — the windows and top-N are fixed.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/movers",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The user's biggest card/sealed-product gain/loss movements over the 1d / 7d / 30d / 1y / 2y / 3y / all-time windows (all-empty when nothing owned or no captured price history).", body = CollectionMovers),
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

    // The user's current card + sealed holdings, reduced to ids/counts. Movements value
    // today's counts at both anchors because there is no per-holding quantity history.
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
        return Ok(Json(CollectionMovers::empty()));
    }

    let card_holdings: Vec<HoldingRow> = card_holdings
        .into_iter()
        .map(|(item_id, quantity, foil_quantity)| HoldingRow {
            item_id,
            quantity,
            foil_quantity,
        })
        .collect();
    let product_holdings: Vec<HoldingRow> = product_holdings
        .into_iter()
        .map(|(item_id, quantity, foil_quantity)| HoldingRow {
            item_id,
            quantity,
            foil_quantity,
        })
        .collect();

    let card_ids: Vec<i32> = card_holdings.iter().map(|h| h.item_id).collect();
    let product_ids: Vec<i32> = product_holdings.iter().map(|h| h.item_id).collect();

    // Find independent reference dates so the existing card series keeps its exact
    // semantics while sealed products can have a newer/older capture cadence.
    let mut card_latest: Option<String> = None;
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
            && card_latest
                .as_ref()
                .map_or(true, |current| candidate.as_str() > current.as_str())
        {
            card_latest = Some(candidate);
        }
    }
    let mut product_latest: Option<String> = None;
    for chunk in product_ids.chunks(PRICE_ID_CHUNK) {
        let chunk_latest = ProductPriceHistory::find()
            .select_only()
            .column_as(product_price_history::Column::AsOfDate.max(), "latest")
            .filter(product_price_history::Column::Game.eq(game.as_str()))
            .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()))
            .into_tuple::<Option<String>>()
            .one(&state.db)
            .await?
            .flatten();
        if let Some(candidate) = chunk_latest
            && product_latest
                .as_ref()
                .map_or(true, |current| candidate.as_str() > current.as_str())
        {
            product_latest = Some(candidate);
        }
    }
    if card_latest.is_none() && product_latest.is_none() {
        return Ok(Json(CollectionMovers::empty()));
    }
    let card_targets = card_latest
        .as_deref()
        .map(WindowTargets::from_latest)
        .transpose()?;
    let product_targets = product_latest
        .as_deref()
        .map(WindowTargets::from_latest)
        .transpose()?;

    // Aggregate the exact snapshots needed per item: the latest row, the last row at or
    // before each fixed baseline, and the first non-null row for each finish. Prefixing each
    // encoded snapshot with its ISO date lets MIN/MAX select the corresponding price row in
    // the same grouped query. The database scans the covering index once per id chunk, but
    // only these compact anchors cross the wire — never the complete daily series, and never
    // one round trip per small group of holdings.
    let mut price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();
    if let Some(targets) = &card_targets {
        for chunk in card_ids.chunks(PRICE_ID_CHUNK) {
            let rows = CardPriceHistory::find()
                .select_only()
                .column_as(card_price_history::Column::CardId, "item_id")
                .column_as(
                    latest_snapshot(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                    ),
                    "latest",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.day,
                    ),
                    "day",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.week,
                    ),
                    "week",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.month,
                    ),
                    "month",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.year,
                    ),
                    "year",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.two_year,
                    ),
                    "two_year",
                )
                .column_as(
                    snapshot_at_or_before(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        &targets.three_year,
                    ),
                    "three_year",
                )
                .column_as(
                    first_priced_snapshot(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        card_price_history::Column::PriceUsd,
                    ),
                    "first_usd",
                )
                .column_as(
                    first_priced_snapshot(
                        card_price_history::Column::AsOfDate,
                        card_price_history::Column::PriceUsd,
                        card_price_history::Column::PriceUsdFoil,
                        card_price_history::Column::PriceUsdFoil,
                    ),
                    "first_foil",
                )
                .filter(card_price_history::Column::Game.eq(game.as_str()))
                .filter(card_price_history::Column::CardId.is_in(chunk.iter().copied()))
                .group_by(card_price_history::Column::CardId)
                .into_model::<PriceAnchorSnapshots>()
                .all(&state.db)
                .await?;
            for anchors in rows {
                price_rows.extend(anchors.into_price_rows()?);
            }
        }
    }

    let mut product_price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();
    if let Some(targets) = &product_targets {
        for chunk in product_ids.chunks(PRICE_ID_CHUNK) {
            let rows = ProductPriceHistory::find()
                .select_only()
                .column_as(product_price_history::Column::ProductId, "item_id")
                .column_as(
                    latest_snapshot(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                    ),
                    "latest",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.day,
                    ),
                    "day",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.week,
                    ),
                    "week",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.month,
                    ),
                    "month",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.year,
                    ),
                    "year",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.two_year,
                    ),
                    "two_year",
                )
                .column_as(
                    snapshot_at_or_before(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        &targets.three_year,
                    ),
                    "three_year",
                )
                .column_as(
                    first_priced_snapshot(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        product_price_history::Column::PriceUsd,
                    ),
                    "first_usd",
                )
                .column_as(
                    first_priced_snapshot(
                        product_price_history::Column::AsOfDate,
                        product_price_history::Column::PriceUsd,
                        product_price_history::Column::PriceUsdFoil,
                        product_price_history::Column::PriceUsdFoil,
                    ),
                    "first_foil",
                )
                .filter(product_price_history::Column::Game.eq(game.as_str()))
                .filter(product_price_history::Column::ProductId.is_in(chunk.iter().copied()))
                .group_by(product_price_history::Column::ProductId)
                .into_model::<PriceAnchorSnapshots>()
                .all(&state.db)
                .await?;
            for anchors in rows {
                product_price_rows.extend(anchors.into_price_rows()?);
            }
        }
    }

    // Group each card's snapshots (already ascending by date) and parse the decimal-string
    // prices to integer cents once, up front.
    let mut card_prices: HashMap<i32, Vec<PriceCell>> = HashMap::new();
    for (card_id, date, usd, foil) in price_rows {
        card_prices.entry(card_id).or_default().push(PriceCell {
            date,
            usd_cents: price_cents(usd.as_deref()),
            foil_cents: price_cents(foil.as_deref()),
        });
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

    let card_windows = RawMoverSeries::rank(
        &card_holdings,
        &card_prices,
        card_latest.as_deref(),
        card_targets.as_ref(),
        TOP_N,
    );
    let product_windows = RawMoverSeries::rank(
        &product_holdings,
        &product_prices,
        product_latest.as_deref(),
        product_targets.as_ref(),
        TOP_N,
    );

    // The union of every item that survived into any final list. An item can rank in several
    // windows (and as a gainer in one, a loser in another), so de-duplicate before fetching.
    let mut needed_cards: HashSet<i32> = HashSet::new();
    let mut needed_products: HashSet<i32> = HashSet::new();
    for (gainers, losers) in card_windows.lists() {
        for raw in gainers.iter().chain(losers) {
            needed_cards.insert(raw.item_id);
        }
    }
    for (gainers, losers) in product_windows.lists() {
        for raw in gainers.iter().chain(losers) {
            needed_products.insert(raw.item_id);
        }
    }

    // Fetch just those cards, keyed by the internal id the raw movers carry (capture the id
    // before `From` consumes the model). A raw mover whose card row is gone (a catalog
    // re-import dropped it) is skipped when shaped below — a slightly short list is correct;
    // we deliberately don't backfill.
    let cards: HashMap<i32, CardResponse> = if needed_cards.is_empty() {
        HashMap::new()
    } else {
        Card::find()
            .filter(card::Column::Game.eq(game.as_str()))
            .filter(card::Column::Id.is_in(needed_cards.iter().copied()))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|model| {
                let id = model.id;
                (id, CardResponse::from(model))
            })
            .collect()
    };

    let products: HashMap<i32, ProductResponse> = if needed_products.is_empty() {
        HashMap::new()
    } else {
        let names = set_name_map(&state, &game).await?;
        Product::find()
            .filter(product::Column::Game.eq(game.as_str()))
            .filter(product::Column::Id.is_in(needed_products.iter().copied()))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|model| {
                let id = model.id;
                (id, product_response(model, &names))
            })
            .collect()
    };

    Ok(Json(CollectionMovers {
        as_of: card_latest,
        day: shape_card_window(card_windows.day, &cards),
        week: shape_card_window(card_windows.week, &cards),
        month: shape_card_window(card_windows.month, &cards),
        year: shape_card_window(card_windows.year, &cards),
        two_year: shape_card_window(card_windows.two_year, &cards),
        three_year: shape_card_window(card_windows.three_year, &cards),
        all_time: shape_card_window(card_windows.all_time, &cards),
        sealed: CollectionSealedMovers {
            as_of: product_latest,
            day: shape_sealed_window(product_windows.day, &products),
            week: shape_sealed_window(product_windows.week, &products),
            month: shape_sealed_window(product_windows.month, &products),
            year: shape_sealed_window(product_windows.year, &products),
            two_year: shape_sealed_window(product_windows.two_year, &products),
            three_year: shape_sealed_window(product_windows.three_year, &products),
            all_time: shape_sealed_window(product_windows.all_time, &products),
        },
    }))
}

/// Calendar baselines measured back from one holding kind's own latest snapshot.
struct WindowTargets {
    day: String,
    week: String,
    month: String,
    year: String,
    two_year: String,
    three_year: String,
}

impl WindowTargets {
    fn from_latest(latest: &str) -> Result<Self, AppError> {
        let latest_date = NaiveDate::parse_from_str(latest, "%Y-%m-%d").map_err(|e| {
            AppError::Internal(format!("unparseable snapshot date {latest:?}: {e}"))
        })?;
        Ok(Self {
            day: format_date(latest_date - Duration::days(1)),
            week: format_date(latest_date - Duration::days(7)),
            month: format_date(latest_date - Duration::days(30)),
            year: format_date(latest_date - Duration::days(365)),
            two_year: format_date(latest_date - Duration::days(730)),
            three_year: format_date(latest_date - Duration::days(1095)),
        })
    }
}

/// Raw ranked lists for every supported window, before catalog payload shaping.
struct RawMoverSeries {
    day: (Vec<RawMover>, Vec<RawMover>),
    week: (Vec<RawMover>, Vec<RawMover>),
    month: (Vec<RawMover>, Vec<RawMover>),
    year: (Vec<RawMover>, Vec<RawMover>),
    two_year: (Vec<RawMover>, Vec<RawMover>),
    three_year: (Vec<RawMover>, Vec<RawMover>),
    all_time: (Vec<RawMover>, Vec<RawMover>),
}

impl RawMoverSeries {
    fn rank(
        holdings: &[HoldingRow],
        prices: &HashMap<i32, Vec<PriceCell>>,
        latest: Option<&str>,
        targets: Option<&WindowTargets>,
        top_n: usize,
    ) -> Self {
        let (Some(latest), Some(targets)) = (latest, targets) else {
            return Self::empty();
        };
        Self {
            day: window_movers(holdings, prices, latest, &targets.day, top_n),
            week: window_movers(holdings, prices, latest, &targets.week, top_n),
            month: window_movers(holdings, prices, latest, &targets.month, top_n),
            year: window_movers(holdings, prices, latest, &targets.year, top_n),
            two_year: window_movers(holdings, prices, latest, &targets.two_year, top_n),
            three_year: window_movers(holdings, prices, latest, &targets.three_year, top_n),
            all_time: all_time_movers(holdings, prices, latest, top_n),
        }
    }

    fn empty() -> Self {
        Self {
            day: (Vec::new(), Vec::new()),
            week: (Vec::new(), Vec::new()),
            month: (Vec::new(), Vec::new()),
            year: (Vec::new(), Vec::new()),
            two_year: (Vec::new(), Vec::new()),
            three_year: (Vec::new(), Vec::new()),
            all_time: (Vec::new(), Vec::new()),
        }
    }

    fn lists(&self) -> [(&Vec<RawMover>, &Vec<RawMover>); 7] {
        [
            (&self.day.0, &self.day.1),
            (&self.week.0, &self.week.1),
            (&self.month.0, &self.month.1),
            (&self.year.0, &self.year.1),
            (&self.two_year.0, &self.two_year.1),
            (&self.three_year.0, &self.three_year.1),
            (&self.all_time.0, &self.all_time.1),
        ]
    }
}

/// The exact history snapshots needed to rank one card across every response window. Fixed
/// baselines use the most recent row on/before their target; `first_*` can differ because a
/// finish may start receiving prices later than its sibling. Each snapshot is encoded as
/// `YYYY-MM-DD|regular|foil` by the aggregate helpers below.
#[derive(FromQueryResult)]
struct PriceAnchorSnapshots {
    item_id: i32,
    latest: String,
    day: Option<String>,
    week: Option<String>,
    month: Option<String>,
    year: Option<String>,
    two_year: Option<String>,
    three_year: Option<String>,
    first_usd: Option<String>,
    first_foil: Option<String>,
}

impl PriceAnchorSnapshots {
    /// Decode the aggregate cells back to the compact row shape consumed by the existing
    /// ranker. Repeated dates are harmless: grouping below keeps them adjacent, and every
    /// encoding for a given card/date contains the same source-row prices.
    fn into_price_rows(
        self,
    ) -> Result<Vec<(i32, String, Option<String>, Option<String>)>, AppError> {
        let snapshots = [
            Some(self.latest),
            self.day,
            self.week,
            self.month,
            self.year,
            self.two_year,
            self.three_year,
            self.first_usd,
            self.first_foil,
        ];
        let mut rows = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots.into_iter().flatten() {
            let (date, usd, foil) = decode_snapshot(&snapshot)?;
            rows.push((self.item_id, date, usd, foil));
        }
        rows.sort_unstable_by(|a, b| a.1.cmp(&b.1));
        rows.dedup();
        Ok(rows)
    }
}

/// A holding reduced to what the ranking needs: item kind/id and current counts.
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

/// A ranked movement before it is dressed with the card/product payload: counts held and the
/// window's value change, all in integer cents.
struct RawMover {
    item_id: i32,
    quantity: i32,
    foil_quantity: i32,
    value_now_cents: i128,
    value_prev_cents: i128,
    change_cents: i128,
    change_pct: Option<f64>,
}

/// Encode one history row as `YYYY-MM-DD|regular|foil`. ISO dates sort chronologically, so
/// applying MIN/MAX to this string selects the whole price row for the chosen date. `||` and
/// `COALESCE` have identical text semantics on the supported SQLite and Postgres backends;
/// the query remains entirely parameterized through SeaQuery expressions.
pub(super) fn encoded_snapshot<D, U, F>(date_column: D, usd_column: U, foil_column: F) -> SimpleExpr
where
    D: IntoColumnRef,
    U: IntoColumnRef,
    F: IntoColumnRef,
{
    let concat = BinOper::Custom("||");
    Expr::col(date_column)
        .binary(concat, Expr::val("|"))
        .binary(
            concat,
            Func::coalesce([Expr::col(usd_column).into(), Expr::val("").into()]),
        )
        .binary(concat, Expr::val("|"))
        .binary(
            concat,
            Func::coalesce([Expr::col(foil_column).into(), Expr::val("").into()]),
        )
}

/// The item's most recent captured snapshot.
pub(super) fn latest_snapshot<D, U, F>(date_column: D, usd_column: U, foil_column: F) -> SimpleExpr
where
    D: IntoColumnRef,
    U: IntoColumnRef,
    F: IntoColumnRef,
{
    Func::max(encoded_snapshot(date_column, usd_column, foil_column)).into()
}

/// The item's most recent snapshot at or before a fixed target (carry-forward baseline).
fn snapshot_at_or_before<D, U, F>(
    date_column: D,
    usd_column: U,
    foil_column: F,
    target: &str,
) -> SimpleExpr
where
    D: IntoColumnRef + Clone,
    U: IntoColumnRef,
    F: IntoColumnRef,
{
    Func::max(Expr::case(
        Expr::col(date_column.clone()).lte(target),
        encoded_snapshot(date_column, usd_column, foil_column),
    ))
    .into()
}

/// The earliest snapshot on which `price_column` is non-null.
fn first_priced_snapshot<D, U, F, P>(
    date_column: D,
    usd_column: U,
    foil_column: F,
    price_column: P,
) -> SimpleExpr
where
    D: IntoColumnRef,
    U: IntoColumnRef,
    F: IntoColumnRef,
    P: IntoColumnRef,
{
    Func::min(Expr::case(
        Expr::col(price_column).is_not_null(),
        encoded_snapshot(date_column, usd_column, foil_column),
    ))
    .into()
}

/// Decode a compact aggregate snapshot. Stored prices are decimal strings, so `|` cannot
/// occur in a valid value; a malformed encoding indicates an internal data/query invariant
/// failure rather than bad client input.
pub(super) fn decode_snapshot(
    encoded: &str,
) -> Result<(String, Option<String>, Option<String>), AppError> {
    let mut parts = encoded.split('|');
    let (Some(date), Some(usd), Some(foil), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(AppError::Internal(format!(
            "malformed price snapshot aggregate {encoded:?}"
        )));
    };
    Ok((
        date.to_string(),
        (!usd.is_empty()).then(|| usd.to_string()),
        (!foil.is_empty()).then(|| foil.to_string()),
    ))
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
        let Some(cells) = prices.get(&holding.item_id) else {
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
            item_id: holding.item_id,
            quantity: holding.quantity,
            foil_quantity: holding.foil_quantity,
            value_now_cents,
            value_prev_cents,
            change_cents,
            change_pct,
        });
    }

    partition_and_rank(movers, top_n)
}

/// Split non-zero movements by sign, order them deterministically, and apply the cap.
fn partition_and_rank(movers: Vec<RawMover>, top_n: usize) -> (Vec<RawMover>, Vec<RawMover>) {
    // `movers` holds only non-zero changes, so `> 0` cleanly splits gainers from losers.
    let (gainers, losers): (Vec<RawMover>, Vec<RawMover>) =
        movers.into_iter().partition(|m| m.change_cents > 0);

    rank_partitioned(gainers, losers, top_n)
}

fn rank_partitioned(
    mut gainers: Vec<RawMover>,
    mut losers: Vec<RawMover>,
    top_n: usize,
) -> (Vec<RawMover>, Vec<RawMover>) {
    gainers.sort_by(|a, b| {
        b.change_cents
            .cmp(&a.change_cents)
            .then(b.value_now_cents.cmp(&a.value_now_cents))
            .then(a.item_id.cmp(&b.item_id))
    });
    gainers.truncate(top_n);

    losers.sort_by(|a, b| {
        a.change_cents
            .cmp(&b.change_cents)
            .then(b.value_now_cents.cmp(&a.value_now_cents))
            .then(a.item_id.cmp(&b.item_id))
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

/// Dress card movers with catalog payloads, dropping rows missing after a re-import.
fn shape_card_movers(
    raws: Vec<RawMover>,
    cards: &HashMap<i32, CardResponse>,
) -> Vec<CollectionMover> {
    raws.into_iter()
        .filter_map(|raw| {
            Some(CollectionMover {
                card: cards.get(&raw.item_id)?.clone(),
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

fn shape_card_window(
    (gainers, losers): (Vec<RawMover>, Vec<RawMover>),
    cards: &HashMap<i32, CardResponse>,
) -> CollectionMoverList {
    CollectionMoverList {
        gainers: shape_card_movers(gainers, cards),
        losers: shape_card_movers(losers, cards),
    }
}

/// Dress sealed movers with catalog payloads, keeping their wire shape separate from cards.
fn shape_sealed_movers(
    raws: Vec<RawMover>,
    products: &HashMap<i32, ProductResponse>,
) -> Vec<CollectionSealedMover> {
    raws.into_iter()
        .filter_map(|raw| {
            Some(CollectionSealedMover {
                product: products.get(&raw.item_id)?.clone(),
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

fn shape_sealed_window(
    (gainers, losers): (Vec<RawMover>, Vec<RawMover>),
    products: &HashMap<i32, ProductResponse>,
) -> CollectionSealedMoverList {
    CollectionSealedMoverList {
        gainers: shape_sealed_movers(gainers, products),
        losers: shape_sealed_movers(losers, products),
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

    fn holding(item_id: i32, quantity: i32, foil_quantity: i32) -> HoldingRow {
        HoldingRow {
            item_id,
            quantity,
            foil_quantity,
        }
    }

    fn ids(movers: &[RawMover]) -> Vec<i32> {
        movers.iter().map(|m| m.item_id).collect()
    }

    fn changes(movers: &[RawMover]) -> Vec<i128> {
        movers.iter().map(|m| m.change_cents).collect()
    }

    #[test]
    fn compact_snapshot_decode_preserves_null_finishes() {
        assert_eq!(
            decode_snapshot("2024-01-02|12.34|").unwrap(),
            ("2024-01-02".to_string(), Some("12.34".to_string()), None)
        );
        assert_eq!(
            decode_snapshot("2024-01-02||56.78").unwrap(),
            ("2024-01-02".to_string(), None, Some("56.78".to_string()))
        );
        assert!(decode_snapshot("2024-01-02|12.34").is_err());
    }

    #[test]
    fn compact_snapshot_aggregate_renders_for_both_databases() {
        use sea_orm::sea_query::{PostgresQueryBuilder, Query, SqliteQueryBuilder};

        let query = Query::select()
            .expr(latest_snapshot(
                card_price_history::Column::AsOfDate,
                card_price_history::Column::PriceUsd,
                card_price_history::Column::PriceUsdFoil,
            ))
            .expr(snapshot_at_or_before(
                card_price_history::Column::AsOfDate,
                card_price_history::Column::PriceUsd,
                card_price_history::Column::PriceUsdFoil,
                "2024-01-02",
            ))
            .to_owned();
        for sql in [
            query.to_string(SqliteQueryBuilder),
            query.to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains("MAX("), "{sql}");
            assert!(sql.contains(" || "), "{sql}");
            assert!(sql.contains("COALESCE("), "{sql}");
            assert!(sql.contains("CASE WHEN"), "{sql}");
        }
    }

    #[test]
    fn gainers_desc_losers_asc_partitioned_by_sign() {
        // A single 1d window (latest 01-10, baseline 01-09). Each card holds one regular
        // copy; only the price differs, so the value change is the price change.
        let holdings = vec![
            holding(1, 1, 0),
            holding(2, 1, 0),
            holding(3, 1, 0),
            holding(4, 1, 0),
        ];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-09", Some("10.00"), None),
                cell("2024-01-10", Some("15.00"), None),
            ],
        );
        prices.insert(
            2,
            vec![
                cell("2024-01-09", Some("10.00"), None),
                cell("2024-01-10", Some("12.00"), None),
            ],
        );
        prices.insert(
            3,
            vec![
                cell("2024-01-09", Some("10.00"), None),
                cell("2024-01-10", Some("7.00"), None),
            ],
        );
        prices.insert(
            4,
            vec![
                cell("2024-01-09", Some("10.00"), None),
                cell("2024-01-10", Some("4.00"), None),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-10", "2024-01-09", TOP_N);
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
        prices.insert(
            1,
            vec![
                cell("2024-01-05", Some("5.00"), None),
                cell("2024-01-10", Some("9.00"), None),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-10", "2024-01-03", TOP_N);
        assert!(gainers.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn finish_unpriced_at_one_anchor_is_excluded() {
        // The regular finish is unpriced at the baseline but priced now: no delta can be
        // formed, so the card contributes nothing (and, foil-less, drops out entirely).
        let holdings = vec![holding(1, 1, 0)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", None, None),
                cell("2024-01-02", Some("10.00"), None),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
        assert!(gainers.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn regular_and_foil_contribute_quantity_weighted() {
        // Card 1: 2 regular + 3 foil. Regular $1->$2 (×2 = +$2), foil $2->$5 (×3 = +$9).
        // Card 2: foil-only, 2 copies, $3->$4 (×2 = +$2).
        let holdings = vec![holding(1, 2, 3), holding(2, 0, 2)];
        let mut prices = HashMap::new();
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("1.00"), Some("2.00")),
                cell("2024-01-02", Some("2.00"), Some("5.00")),
            ],
        );
        prices.insert(
            2,
            vec![
                cell("2024-01-01", None, Some("3.00")),
                cell("2024-01-02", None, Some("4.00")),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
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
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("5.00"), None),
                cell("2024-01-02", Some("5.00"), None),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
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
            prices.insert(
                k,
                vec![
                    cell("2024-01-01", Some("1.00"), None),
                    cell("2024-01-02", Some(&now), None),
                ],
            );
        }

        let (gainers, _losers) =
            window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
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
        prices.insert(
            1,
            vec![
                cell("2024-01-01", Some("10.00"), None),
                cell("2024-01-02", Some("15.00"), None),
            ],
        );
        prices.insert(
            2,
            vec![
                cell("2024-01-01", Some("10.00"), None),
                cell("2024-01-02", Some("5.00"), None),
            ],
        );
        prices.insert(
            3,
            vec![
                cell("2024-01-01", Some("0.00"), None),
                cell("2024-01-02", Some("3.00"), None),
            ],
        );

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-02", "2024-01-01", TOP_N);
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

        let (gainers, losers) =
            window_movers(&holdings, &prices, "2024-01-10", "2024-01-06", TOP_N);
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
