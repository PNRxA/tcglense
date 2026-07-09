//! Catalog price-history endpoint: a card's price-over-time series, optionally windowed
//! and downsampled by `?range`.

use axum::{
    Json,
    extract::State,
};
use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Serialize;

use crate::entities::card_price_history;
use crate::entities::prelude::CardPriceHistory;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    DataBody, PriceParams, PriceRange, cutoff_date, downsample_rows, load_card, require_game,
};
use crate::state::AppState;

/// One day's price snapshot in a card's price-over-time series. Prices are the
/// decimal strings exactly as stored (mirroring the card's
/// [`PricesResponse`](crate::handlers::shared::dto::PricesResponse)); `date` is a
/// `"YYYY-MM-DD"` string.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
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

/// `GET /api/games/{game}/cards/{id}/prices?range=` -> a card's price history,
/// oldest first, for charting. With no `range` the full daily series is returned;
/// an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows the series and
/// **downsamples** it to a coarser resolution the longer the window. `404` if the
/// game or card id is unknown; `422` for an unknown `range`; an empty
/// `{ "data": [] }` when the card has no captured history in the window.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/{id}/prices",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
        ("range" = Option<String>, Query, description = "Window + resolution (`7d`/`30d`/`1y`/`2y`/`3y`/`all`); absent = the full daily series"),
    ),
    responses(
        (status = 200, description = "The card's price history, oldest first.", body = DataBody<Vec<PricePoint>>),
        (status = 404, description = "Unknown game or card."),
        (status = 422, description = "Unknown range."),
    ),
)]
pub async fn card_prices(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<PriceParams>,
) -> Result<Json<DataBody<Vec<PricePoint>>>, AppError> {
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

    let kept = downsample_rows(rows, range.map_or(1, PriceRange::bucket_days), |r| {
        r.as_of_date.as_str()
    });
    let data: Vec<PricePoint> = kept.into_iter().map(PricePoint::from).collect();
    Ok(Json(DataBody { data }))
}
