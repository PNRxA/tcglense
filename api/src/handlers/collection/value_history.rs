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
use chrono::{Datelike, NaiveDate, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter,
    QueryOrder, QuerySelect, Statement, Value,
};
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
use crate::scryfall::format_date;
use crate::state::AppState;

use super::price_movements::{SnapshotSeek, decode_snapshot};

/// How many card ids to bind per `IN (...)` chunk. Kept well under SQLite's 32766
/// bound-parameter cap (each chunk also binds `game` + the optional cutoff), so an
/// arbitrarily large collection still fetches in a handful of queries.
const PRICE_ID_CHUNK: usize = 10_000;

/// One held item's last captured snapshot before an explicit range cutoff. It seeds the
/// carry-forward cursor without exposing an out-of-range day in the response. `snapshot` is
/// `None` for a held item with no captured row before the cutoff (the query is driven from the
/// holdings table, so every held item yields a row).
#[derive(FromQueryResult)]
struct CutoffAnchor {
    item_id: i32,
    snapshot: Option<String>,
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

    // Version-keyed, single-flight response cache (issues #413/#365): between the
    // user's own edits and the daily price capture this response cannot change,
    // and it is the app's most expensive per-user read. `get_or_compute` also
    // coalesces concurrent misses for the same key onto one computation. `None`
    // key = cache degraded, compute as normal.
    let cache_key = state
        .analytics_cache
        .body_key(
            user.id,
            &game,
            "value-history",
            range.map_or("full", PriceRange::token),
        )
        .await;
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, user, game) = (state.clone(), user.clone(), game.clone());
            async move {
                let payload = value_history_payload(state, user, game, range).await?;
                serde_json::to_vec(&payload)
                    .map_err(|err| AppError::Internal(format!("serialize value history: {err}")))
            }
        })
        .await?;
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
    let today = Utc::now().date_naive();
    let cutoff = range.and_then(|r| cutoff_date(today, r));
    // Downsample bucket width for this range (1 = daily, no downsampling). The wide ranges
    // (1y/2y/3y/all, `bucket_days > 1`) fetch one representative snapshot per (item, bucket)
    // below instead of every daily row.
    let bucket_days = range.map_or(1, PriceRange::bucket_days);

    let mut price_rows: Vec<(i32, String, Option<String>, Option<String>)> = Vec::new();

    // For a windowed request, seed each held card's carry-forward cursor with its latest
    // snapshot **strictly before** the cutoff (so the window's first day carries a real price
    // instead of blanking). This is one correlated `LIMIT 1` point-seek per held card, driven
    // from the holdings table and riding the `m…031` covering index — no id chunking needed
    // (it binds only `game` + the cutoff, not the id list). A held card with no pre-cutoff row
    // yields a NULL snapshot, skipped. Each seeded anchor predates every windowed row below, so
    // pushing them all first keeps each card's cells ascending by date for the fold.
    if let Some(cutoff) = &cutoff {
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
        let anchors = CollectionItem::find()
            .select_only()
            .column_as(collection_item::Column::CardId, "item_id")
            .expr_as(seek.before(cutoff.as_str()), "snapshot")
            .filter(collection_item::Column::UserId.eq(user.id))
            .filter(collection_item::Column::Game.eq(game.as_str()))
            .into_model::<CutoffAnchor>()
            .all(&state.db)
            .await?;
        for anchor in anchors {
            let Some(snapshot) = anchor.snapshot else {
                continue;
            };
            let (date, usd, foil) = decode_snapshot(&snapshot)?;
            price_rows.push((anchor.item_id, date, usd, foil));
        }
    }

    // Historic prices for exactly those cards, windowed to the range.
    if bucket_days > 1 {
        // Wide ranges (1y/2y/3y/all): fetch one representative (last) snapshot per (card,
        // downsample bucket) — O(cards × buckets) instead of O(cards × days) — and let the
        // fold + final downsample produce the same series. The buckets align to
        // `downsample_rows`' grid, and the pre-cutoff seed pushed above still carries a real
        // value into the window's first day. On a faithful 18M-row Postgres 16 repro the
        // full-history read dropped from 861k rows fetched to ~15k at similar cold I/O.
        // Window start: the cutoff for a bounded range, else (all) the earliest captured
        // card snapshot — a cheap forward first-row read on m…050's (game, as_of_date, …).
        let earliest = match cutoff {
            Some(_) => None,
            None => CardPriceHistory::find()
                .select_only()
                .column_as(card_price_history::Column::AsOfDate.min(), "m")
                .filter(card_price_history::Column::Game.eq(game.as_str()))
                .into_tuple::<Option<String>>()
                .one(&state.db)
                .await?
                .flatten(),
        };
        if let Some(start) = window_start(cutoff.as_deref(), earliest.as_deref())? {
            let buckets = bucketed_window(start, today, bucket_days, cutoff.as_deref());
            let rows = fetch_bucketed_snapshots(
                &state.db,
                "card_price_history",
                "card_id",
                "collection_items",
                &game,
                user.id,
                &buckets,
            )
            .await?;
            price_rows.extend(rows);
        }
        // The (card, bucket) rows arrive unordered; the fold needs each card's cells
        // ascending by date (the pre-cutoff seeds already sort before the window's rows).
        price_rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    } else {
        // Daily ranges (7d/30d/full): chunk the id list so the `IN (...)` never exceeds the
        // bound-parameter cap; the (game, card_id, as_of_date) covering index serves each
        // chunk (equality on game + card_id, range on as_of_date) index-only, so only the
        // four columns the fold reads cross the wire and the query stays cheap on a cold DB.
        for chunk in card_ids.chunks(PRICE_ID_CHUNK) {
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

    // Sealed products get the same pre-cutoff point-seek seed as cards above, riding `m…020`'s
    // non-covering `idx_product_price_history_game_product_date` (one heap fetch per seed row)
    // rather than the card-side `m…031` covering index.
    if let Some(cutoff) = &cutoff {
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
        let anchors = CollectionProductItem::find()
            .select_only()
            .column_as(collection_product_item::Column::ProductId, "item_id")
            .expr_as(seek.before(cutoff.as_str()), "snapshot")
            .filter(collection_product_item::Column::UserId.eq(user.id))
            .filter(collection_product_item::Column::Game.eq(game.as_str()))
            .into_model::<CutoffAnchor>()
            .all(&state.db)
            .await?;
        for anchor in anchors {
            let Some(snapshot) = anchor.snapshot else {
                continue;
            };
            let (date, usd, foil) = decode_snapshot(&snapshot)?;
            product_price_rows.push((anchor.item_id, date, usd, foil));
        }
    }

    if bucket_days > 1 {
        // Sealed products get the same per-(product, bucket) skip-seek as cards for the wide
        // ranges (product seeks ride `m…020`'s non-covering unique index, so each returned
        // `LIMIT 1` row costs one heap fetch — fine at sealed-product counts).
        let earliest = match cutoff {
            Some(_) => None,
            None => ProductPriceHistory::find()
                .select_only()
                .column_as(product_price_history::Column::AsOfDate.min(), "m")
                .filter(product_price_history::Column::Game.eq(game.as_str()))
                .into_tuple::<Option<String>>()
                .one(&state.db)
                .await?
                .flatten(),
        };
        if let Some(start) = window_start(cutoff.as_deref(), earliest.as_deref())? {
            let buckets = bucketed_window(start, today, bucket_days, cutoff.as_deref());
            let rows = fetch_bucketed_snapshots(
                &state.db,
                "product_price_history",
                "product_id",
                "collection_product_items",
                &game,
                user.id,
                &buckets,
            )
            .await?;
            product_price_rows.extend(rows);
        }
        product_price_rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    } else {
        for chunk in product_ids.chunks(PRICE_ID_CHUNK) {
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
        bucket_days,
        cutoff.as_deref(),
    );
    Ok(DataBody { data: points })
}

/// Parse a stored `"YYYY-MM-DD"` snapshot date. A malformed one is an internal invariant
/// failure (these dates are ours, never client input), so it surfaces as a 500.
fn parse_iso_date(s: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| AppError::Internal(format!("unparseable snapshot date {s:?}: {e}")))
}

/// The first day the wide-range bucket grid must cover: the window `cutoff` for a bounded
/// range, else (all-time) the `earliest` captured snapshot. `None` when neither exists (an
/// all-time request over a game with no captured history — nothing to fetch).
fn window_start(
    cutoff: Option<&str>,
    earliest: Option<&str>,
) -> Result<Option<NaiveDate>, AppError> {
    match cutoff.or(earliest) {
        Some(s) => Ok(Some(parse_iso_date(s)?)),
        None => Ok(None),
    }
}

/// The `[lo, hi)` date-string ranges of every downsample bucket spanning `[start, today]`,
/// aligned to the exact same grid [`downsample_rows`] buckets on (days-since-CE /
/// `bucket_days`) — so fetching each item's last row per bucket and re-downsampling the
/// folded series reproduces the daily-fetch result. The first bucket's lower bound is
/// clamped to `floor` (the window cutoff) so a bounded range never fetches pre-window days
/// the daily path would have excluded; the pre-cutoff seed carries earlier values in.
fn bucketed_window(
    start: NaiveDate,
    today: NaiveDate,
    bucket_days: i64,
    floor: Option<&str>,
) -> Vec<(String, String)> {
    let width = bucket_days.max(1);
    let first = i64::from(start.num_days_from_ce()) / width;
    let last = i64::from(today.num_days_from_ce()) / width;
    let mut buckets: Vec<(String, String)> = (first..=last)
        .filter_map(|k| {
            let lo = NaiveDate::from_num_days_from_ce_opt((k * width) as i32)?;
            let hi = NaiveDate::from_num_days_from_ce_opt(((k + 1) * width) as i32)?;
            Some((format_date(lo), format_date(hi)))
        })
        .collect();
    if let (Some(first), Some(floor)) = (buckets.first_mut(), floor)
        && first.0.as_str() < floor
    {
        first.0 = floor.to_string();
    }
    buckets
}

/// One `(item, bucket)` seek result: the item id and its encoded last-in-bucket snapshot
/// (`None` when the item has no captured row in that bucket).
#[derive(FromQueryResult)]
struct BucketSnapshot {
    item_id: i32,
    snapshot: Option<String>,
}

/// Fetch, per held item, its last captured snapshot within each downsample `bucket` — the
/// `O(items × buckets)` wide-range replacement for the daily `O(items × days)` bulk fetch.
///
/// Drives from the holdings table cross-joined with the bucket ranges, and runs one
/// correlated `LIMIT 1` seek per `(item, bucket)` on the history table's covering index
/// (`game = ? AND id = <holding>.id AND as_of_date` in `[lo, hi)`, newest first) — the same
/// tiny index-descent shape as [`SnapshotSeek`], but a *range* per bucket instead of a
/// single point. Returns `(item_id, date, usd, foil)` rows for the caller to fold exactly
/// like the daily rows.
///
/// Cross-backend raw SQL through the [`crate::db::Dialect`] placeholder seam: the bucket
/// ranges are an inline `SELECT … UNION ALL` derived table (SQLite has no `VALUES`
/// column-alias syntax), and `||` / `COALESCE` share text semantics on both backends
/// (mirroring [`super::price_movements::encoded_snapshot`]). Only the fixed table/column
/// idents are interpolated; every date, `game`, and `user_id` is a bound value.
async fn fetch_bucketed_snapshots(
    db: &DatabaseConnection,
    history_table: &str,
    id_col: &str,
    holdings_table: &str,
    game: &str,
    user_id: i32,
    buckets: &[(String, String)],
) -> Result<Vec<(i32, String, Option<String>, Option<String>)>, AppError> {
    if buckets.is_empty() {
        return Ok(Vec::new());
    }
    let backend = db.get_database_backend();
    let bucket_rows = std::iter::once("SELECT ? AS lo, ? AS hi")
        .chain(std::iter::repeat("SELECT ?, ?").take(buckets.len() - 1))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let template = format!(
        "SELECT h.\"{id_col}\" AS item_id, (\
             SELECT p.\"as_of_date\" || '|' || COALESCE(p.\"price_usd\", '') || '|' \
                    || COALESCE(p.\"price_usd_foil\", '') \
             FROM \"{history_table}\" p \
             WHERE p.\"game\" = ? AND p.\"{id_col}\" = h.\"{id_col}\" \
               AND p.\"as_of_date\" >= bk.lo AND p.\"as_of_date\" < bk.hi \
             ORDER BY p.\"as_of_date\" DESC LIMIT 1\
           ) AS snapshot \
         FROM \"{holdings_table}\" h \
         CROSS JOIN ({bucket_rows}) bk \
         WHERE h.\"user_id\" = ? AND h.\"game\" = ?"
    );
    let sql = crate::db::Dialect::from_backend(backend).placeholders(&template);
    // Bind order follows the textual `?` order: the subselect's `game`, then each bucket's
    // (lo, hi), then the outer `user_id` and `game`.
    let mut values: Vec<Value> = Vec::with_capacity(3 + buckets.len() * 2);
    values.push(game.into());
    for (lo, hi) in buckets {
        values.push(lo.as_str().into());
        values.push(hi.as_str().into());
    }
    values.push(user_id.into());
    values.push(game.into());
    let stmt = Statement::from_sql_and_values(backend, sql, values);

    let mut out = Vec::new();
    for row in BucketSnapshot::find_by_statement(stmt).all(db).await? {
        let Some(snapshot) = row.snapshot else {
            continue;
        };
        let (date, usd, foil) = decode_snapshot(&snapshot)?;
        out.push((row.item_id, date, usd, foil));
    }
    Ok(out)
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
        let holdings = vec![holding(1, 1, 0), holding(2, 1, 0)];
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
        let holdings = vec![holding(1, 1, 0), holding(2, 1, 0)];
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

    /// The wide-range per-(item, bucket) skip-seek must fold to the **same** downsampled
    /// series as the daily bulk fetch it replaces. Seeds a small SQLite catalog with daily
    /// prices (unpriced days, a 12-day gap, and a late-starting card), then for every wide
    /// range asserts the bucketed fetch folds identically to the daily fetch. Guards the raw
    /// cross-backend SQL and the bucket-grid alignment on the CI (SQLite) backend.
    #[tokio::test]
    async fn bucketed_wide_range_fetch_folds_identically_to_daily_fetch() {
        use chrono::Duration;
        use sea_orm::{
            ActiveModelTrait, ActiveValue::Set, ColumnTrait, QueryFilter, QueryOrder, QuerySelect,
        };

        let db = crate::test_support::migrated_memory_db().await;
        // This test seeds only the price-history + holdings rows the fetch reads, not the
        // referenced `cards`/`users` rows, so relax FK enforcement for the connection.
        db.execute_unprepared("PRAGMA foreign_keys = OFF")
            .await
            .expect("disable fk checks");
        let game = crate::scryfall::GAME;
        let user_id = 1;
        let today = Utc::now().date_naive();
        let now = Utc::now();

        async fn put(
            db: &DatabaseConnection,
            game: &str,
            card_id: i32,
            date: &str,
            usd: Option<String>,
            foil: Option<String>,
            now: chrono::DateTime<Utc>,
        ) {
            card_price_history::ActiveModel {
                game: Set(game.to_string()),
                card_id: Set(card_id),
                as_of_date: Set(date.to_string()),
                price_usd: Set(usd),
                price_usd_foil: Set(foil),
                price_eur: Set(None),
                price_tix: Set(None),
                created_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await
            .expect("insert price row");
        }

        // 120 days of history ending today.
        for d in 0..120i64 {
            let date = format_date(today - Duration::days(119 - d));
            // Card 1: priced every day, foil too.
            put(
                &db,
                game,
                1,
                &date,
                Some(format!("{}.50", 10 + d % 7)),
                Some(format!("{}.00", 30 + d % 5)),
                now,
            )
            .await;
            // Card 2: priced every day, no foil, with two unpriced (null-usd) days.
            let usd2 = (d != 33 && d != 71).then(|| format!("{}.25", 5 + d % 3));
            put(&db, game, 2, &date, usd2, None, now).await;
            // Card 3: starts on day 60, with a 12-day gap (days 80..92).
            if d >= 60 && !(80..92).contains(&d) {
                put(
                    &db,
                    game,
                    3,
                    &date,
                    Some(format!("{}.00", 100 + d % 11)),
                    None,
                    now,
                )
                .await;
            }
        }

        for (card_id, qty, foil) in [(1, 2, 1), (2, 3, 0), (3, 1, 0)] {
            collection_item::ActiveModel {
                user_id: Set(user_id),
                game: Set(game.to_string()),
                card_id: Set(card_id),
                quantity: Set(qty),
                foil_quantity: Set(foil),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .expect("insert holding");
        }

        let holdings = vec![holding(1, 2, 1), holding(2, 3, 0), holding(3, 1, 0)];
        let comparable = |pts: &[CollectionValuePoint]| -> Vec<(String, Option<String>)> {
            pts.iter()
                .map(|p| (p.date.clone(), p.value_usd.clone()))
                .collect()
        };
        let fold_rows = |rows: Vec<(i32, String, Option<String>, Option<String>)>,
                         bucket_days: i64,
                         floor: Option<&str>| {
            let mut prices: HashMap<i32, Vec<PriceCell>> = HashMap::new();
            for (id, date, usd, foil) in rows {
                prices.entry(id).or_default().push(PriceCell {
                    date,
                    usd_cents: price_cents(usd.as_deref()),
                    foil_cents: price_cents(foil.as_deref()),
                });
            }
            for cells in prices.values_mut() {
                cells.sort_by(|a, b| a.date.cmp(&b.date));
            }
            fold_collection_value_history(
                &holdings,
                &prices,
                &[],
                &HashMap::new(),
                bucket_days,
                floor,
            )
        };

        for range in [
            PriceRange::Y1,
            PriceRange::Y2,
            PriceRange::Y3,
            PriceRange::All,
        ] {
            let bucket_days = range.bucket_days();
            let cutoff = cutoff_date(today, range);

            // Daily reference: every windowed daily row (the pre-skip-seek fetch).
            let mut q = CardPriceHistory::find()
                .select_only()
                .column(card_price_history::Column::CardId)
                .column(card_price_history::Column::AsOfDate)
                .column(card_price_history::Column::PriceUsd)
                .column(card_price_history::Column::PriceUsdFoil)
                .filter(card_price_history::Column::Game.eq(game));
            if let Some(c) = &cutoff {
                q = q.filter(card_price_history::Column::AsOfDate.gte(c.as_str()));
            }
            let daily = q
                .order_by_asc(card_price_history::Column::CardId)
                .order_by_asc(card_price_history::Column::AsOfDate)
                .into_tuple::<(i32, String, Option<String>, Option<String>)>()
                .all(&db)
                .await
                .unwrap();

            // Bucketed: the new per-(card, bucket) skip-seek path.
            let earliest = match cutoff {
                Some(_) => None,
                None => CardPriceHistory::find()
                    .select_only()
                    .column_as(card_price_history::Column::AsOfDate.min(), "m")
                    .filter(card_price_history::Column::Game.eq(game))
                    .into_tuple::<Option<String>>()
                    .one(&db)
                    .await
                    .unwrap()
                    .flatten(),
            };
            let bucketed = match window_start(cutoff.as_deref(), earliest.as_deref()).unwrap() {
                Some(start) => {
                    let buckets = bucketed_window(start, today, bucket_days, cutoff.as_deref());
                    fetch_bucketed_snapshots(
                        &db,
                        "card_price_history",
                        "card_id",
                        "collection_items",
                        game,
                        user_id,
                        &buckets,
                    )
                    .await
                    .unwrap()
                }
                None => Vec::new(),
            };

            assert_eq!(
                comparable(&fold_rows(daily, bucket_days, cutoff.as_deref())),
                comparable(&fold_rows(bucketed, bucket_days, cutoff.as_deref())),
                "range {range:?}: bucketed fetch must fold identically to the daily fetch",
            );
        }
    }
}
