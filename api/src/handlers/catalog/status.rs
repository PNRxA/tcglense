//! Catalog meta endpoints: the supported-games list and a game's card-data import
//! status.

use axum::{
    Json,
    extract::State,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, prelude::DateTimeUtc};
use serde::Serialize;

use crate::catalog;
use crate::entities::ingest_state;
use crate::entities::prelude::IngestState;
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

/// Background card-data import status for a game.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "IngestStatus"))]
pub struct StatusResponse {
    pub status: String,
    pub detail: Option<String>,
    pub sets_imported: i32,
    pub cards_imported: i32,
    pub source_updated_at: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub finished_at: Option<DateTimeUtc>,
}

/// List games
///
/// `GET /api/games` -> the list of supported games.
#[utoipa::path(
    get,
    path = "/api/games",
    tag = "Cards",
    responses(
        (status = 200, description = "Every supported game.", body = DataBody<Vec<catalog::Game>>),
    ),
)]
pub async fn list_games() -> Json<DataBody<&'static [catalog::Game]>> {
    Json(DataBody { data: catalog::GAMES })
}

/// Get import status
///
/// `GET /api/games/{game}/status` -> the card-data import status for a game.
#[utoipa::path(
    get,
    path = "/api/games/{game}/status",
    tag = "Cards",
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "The game's card-data import status (idle defaults when never imported).", body = StatusResponse),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn ingest_status(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<StatusResponse>, AppError> {
    require_game(&game)?;
    // Pin to the card-data dataset: a game can now carry several `ingest_state` rows
    // (the `default_cards` import plus the one-off `tcgcsv_price_backfill`), so a
    // game-only filter would be ambiguous for `.one()`. The status route only ever
    // reports the card-data import.
    let row = IngestState::find()
        .filter(ingest_state::Column::Game.eq(game.as_str()))
        .filter(ingest_state::Column::Dataset.eq(crate::scryfall::DATASET))
        .one(&state.db)
        .await?;
    Ok(Json(match row {
        Some(r) => StatusResponse {
            status: r.status,
            detail: r.detail,
            sets_imported: r.sets_imported,
            cards_imported: r.cards_imported,
            source_updated_at: r.source_updated_at,
            finished_at: r.finished_at,
        },
        None => StatusResponse {
            status: "idle".to_string(),
            detail: None,
            sets_imported: 0,
            cards_imported: 0,
            source_updated_at: None,
            finished_at: None,
        },
    }))
}
