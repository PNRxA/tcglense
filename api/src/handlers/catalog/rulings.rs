//! Catalog card-rulings endpoint: a card's "Notes and Rules Information" (issue #522) —
//! the official rulings Scryfall records for the card's gameplay identity (`oracle_id`).

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Serialize;

use crate::entities::card_ruling;
use crate::entities::prelude::CardRuling;
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, load_card, require_game};
use crate::state::AppState;

/// One ruling for a card — an official clarification of how it works, as shown in
/// Scryfall's "Notes and Rules Information" section. `source` is who issued it (`"wotc"`
/// or `"scryfall"`); `published_at` is a `"YYYY-MM-DD"` string.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Ruling {
    pub source: String,
    pub published_at: String,
    pub comment: String,
}

impl From<card_ruling::Model> for Ruling {
    fn from(m: card_ruling::Model) -> Self {
        Ruling {
            source: m.source,
            published_at: m.published_at,
            comment: m.comment,
        }
    }
}

/// Get card rulings
///
/// `GET /api/games/{game}/cards/{id}/rulings` -> the card's rulings ("Notes and Rules
/// Information"), oldest first. Rulings are keyed by the card's gameplay identity
/// (`oracle_id`), so every printing of a card returns the same list. `404` if the game or
/// card id is unknown; an empty `{ "data": [] }` when the card has no rulings.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/{id}/rulings",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
    ),
    responses(
        (status = 200, description = "The card's rulings, oldest first.", body = DataBody<Vec<Ruling>>),
        (status = 404, description = "Unknown game or card."),
    ),
)]
pub async fn card_rulings(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<Ruling>>>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;

    // Rulings key on the gameplay identity (oracle_id); a card without one (e.g. a token)
    // has none.
    let data = match card.oracle_id.as_deref() {
        Some(oracle_id) => CardRuling::find()
            .filter(card_ruling::Column::Game.eq(game.as_str()))
            .filter(card_ruling::Column::OracleId.eq(oracle_id))
            .order_by_asc(card_ruling::Column::PublishedAt)
            .order_by_asc(card_ruling::Column::Id)
            .all(&state.db)
            .await?
            .into_iter()
            .map(Ruling::from)
            .collect(),
        None => Vec::new(),
    };
    Ok(Json(DataBody { data }))
}
