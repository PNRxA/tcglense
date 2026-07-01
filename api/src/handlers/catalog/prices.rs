//! Catalog price-history endpoint: a card's price-over-time series, optionally windowed
//! and downsampled by `?range`.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Serialize;
use serde_json::json;

use crate::entities::card_price_history;
use crate::entities::prelude::CardPriceHistory;
use crate::error::AppError;
use crate::handlers::shared::{load_card, require_game};
use crate::state::AppState;

use super::PriceParams;

/// One day's price snapshot in a card's price-over-time series. Prices are the
/// decimal strings exactly as stored (mirroring the card's
/// [`PricesResponse`](crate::handlers::shared::dto::PricesResponse)); `date` is a
/// `"YYYY-MM-DD"` string.
#[derive(Debug, Serialize)]
pub struct PricePoint {
    pub date: String,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

impl From<card_price_history::Model> for PricePoint {
    fn from(m: card_price_history::Model) -> Self {
        PricePoint {
            date: m.as_of_date,
            usd: m.price_usd,
            usd_foil: m.price_usd_foil,
            eur: m.price_eur,
            tix: m.price_tix,
        }
    }
}

/// Time window + sampling resolution for a card's price history, selected by the
/// detail-page chart via `?range`. Longer windows are **downsampled** to a coarser
/// resolution so the wire payload (and the plotted line) stays light however much
/// history accrues — the more duration, the lower the resolution. When no `range`
/// is given the endpoint returns the full, un-sampled daily series (the original
/// contract), so this is backward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PriceRange {
    /// Last 7 days, daily.
    D7,
    /// Last 30 days, daily.
    D30,
    /// Last year, weekly.
    Y1,
    /// Last 2 years, fortnightly.
    Y2,
    /// Last 3 years, monthly.
    Y3,
    /// All of history, every ~2 months.
    All,
}

impl PriceRange {
    /// An unrecognised value is a 422 — consistent with a bad `sort`/`q` — rather
    /// than being silently ignored. Blank/absent is handled by the caller (it means
    /// "full series"), so this is only ever called with a non-empty value.
    pub(super) fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "7d" => PriceRange::D7,
            "30d" => PriceRange::D30,
            "1y" => PriceRange::Y1,
            "2y" => PriceRange::Y2,
            "3y" => PriceRange::Y3,
            "all" => PriceRange::All,
            other => return Err(AppError::Validation(format!("unknown range '{other}'"))),
        })
    }

    /// How many days back the window reaches, or `None` for all of history.
    fn window_days(self) -> Option<i64> {
        match self {
            PriceRange::D7 => Some(7),
            PriceRange::D30 => Some(30),
            PriceRange::Y1 => Some(365),
            PriceRange::Y2 => Some(730),
            PriceRange::Y3 => Some(1095),
            PriceRange::All => None,
        }
    }

    /// Width of one downsample bucket in days; one representative day (the most
    /// recent in the bucket) is kept per bucket, so a larger value = coarser chart.
    fn bucket_days(self) -> i64 {
        match self {
            PriceRange::D7 | PriceRange::D30 => 1,
            PriceRange::Y1 => 7,
            PriceRange::Y2 => 14,
            PriceRange::Y3 => 30,
            PriceRange::All => 60,
        }
    }
}

/// The inclusive lower bound (`"YYYY-MM-DD"`) for a range's window relative to
/// `today`, or `None` for [`PriceRange::All`] (no lower bound). Pure so the date
/// arithmetic stays unit-testable; the handler passes `Utc::now().date_naive()`.
pub(super) fn cutoff_date(today: NaiveDate, range: PriceRange) -> Option<String> {
    range
        .window_days()
        .map(|days| crate::scryfall::format_date(today - Duration::days(days)))
}

/// Downsample an **ascending** run of price-history rows to one representative day
/// per `bucket_days`-wide bucket, keeping the *last* (most recent) row in each
/// bucket — so the newest day is always retained. `bucket_days <= 1` is a
/// passthrough (full resolution). Prices are kept as the exact stored decimal
/// strings — never averaged — so every returned point stays a real, internally
/// consistent day (its `usd`/`foil`/`eur`/`tix` all come from the same snapshot).
pub(super) fn downsample(rows: Vec<card_price_history::Model>, bucket_days: i64) -> Vec<PricePoint> {
    if bucket_days <= 1 {
        return rows.into_iter().map(PricePoint::from).collect();
    }
    let mut out: Vec<PricePoint> = Vec::new();
    let mut last_key: Option<i64> = None;
    for row in rows {
        // Bucket on (days-since-CE / width). For our zero-padded `YYYY-MM-DD` rows
        // the keys are monotonic in an ascending series, so equal keys are
        // contiguous. An unparseable date (shouldn't happen) gets a sentinel key
        // that never coalesces, keeping the row rather than dropping it.
        let key = NaiveDate::parse_from_str(&row.as_of_date, "%Y-%m-%d")
            .map(|d| i64::from(d.num_days_from_ce()) / bucket_days)
            .unwrap_or(i64::MIN);
        if last_key == Some(key) && key != i64::MIN {
            *out.last_mut().expect("out is non-empty once last_key is set") = PricePoint::from(row);
        } else {
            out.push(PricePoint::from(row));
            last_key = Some(key);
        }
    }
    out
}

/// `GET /api/games/{game}/cards/{id}/prices?range=` -> a card's price history,
/// oldest first, for charting. With no `range` the full daily series is returned;
/// an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows the series and
/// **downsamples** it to a coarser resolution the longer the window. `404` if the
/// game or card id is unknown; `422` for an unknown `range`; an empty
/// `{ "data": [] }` when the card has no captured history in the window.
pub async fn card_prices(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<PriceParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;

    // Blank/absent range -> the full daily series (original contract); an explicit
    // range windows + downsamples. Unknown values 422, like a bad `sort`.
    let range = match params.range.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(value) => Some(PriceRange::parse(value)?),
    };

    let mut query = CardPriceHistory::find()
        .filter(card_price_history::Column::Game.eq(game.as_str()))
        .filter(card_price_history::Column::CardId.eq(card.id));
    if let Some(cutoff) = range.and_then(|r| cutoff_date(Utc::now().date_naive(), r)) {
        query = query.filter(card_price_history::Column::AsOfDate.gte(cutoff));
    }
    let rows = query
        .order_by_asc(card_price_history::Column::AsOfDate)
        .all(&state.db)
        .await?;

    let data = downsample(rows, range.map_or(1, PriceRange::bucket_days));
    Ok(Json(json!({ "data": data })))
}
