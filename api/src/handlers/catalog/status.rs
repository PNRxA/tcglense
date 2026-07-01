//! Catalog meta endpoints: the supported-games list and a game's card-data import
//! status.

use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, prelude::DateTimeUtc};
use serde::Serialize;

use crate::catalog;
use crate::entities::ingest_state;
use crate::entities::prelude::IngestState;
use crate::error::AppError;
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub detail: Option<String>,
    pub sets_imported: i32,
    pub cards_imported: i32,
    pub source_updated_at: Option<String>,
    pub finished_at: Option<DateTimeUtc>,
}

/// `GET /api/games` -> the list of supported games.
pub async fn list_games() -> Json<DataBody<&'static [catalog::Game]>> {
    Json(DataBody { data: catalog::GAMES })
}

/// `GET /api/games/{game}/status` -> the card-data import status for a game.
pub async fn ingest_status(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<StatusResponse>, AppError> {
    require_game(&game)?;
    let row = IngestState::find()
        .filter(ingest_state::Column::Game.eq(game.as_str()))
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
