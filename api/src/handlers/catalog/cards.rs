//! Catalog card endpoints: the all-cards list (search + paginate), one card's full
//! detail, and a card's other printings.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;

use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, Page, SortField, apply_card_sort, build_page, load_card, require_game,
};
use crate::state::AppState;

use super::{ListParams, apply_search, prints_query};

/// `GET /api/games/{game}/cards` -> all cards (optional `q` search), by name.
pub async fn list_cards(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let mut query = Card::find().filter(card::Column::Game.eq(game.as_str()));
    query = apply_search(query, game_meta, &params)?;
    let (sort, dir) = params.sort_spec(SortField::Name)?;
    let paginator = apply_card_sort(query, sort, dir, false).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    let data: Vec<CardResponse> = rows.into_iter().map(CardResponse::from).collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/games/{game}/cards/{id}` -> one card's full detail.
pub async fn get_card(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CardResponse>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    Ok(Json(CardResponse::from(card)))
}

/// `GET /api/games/{game}/cards/{id}/prints` -> this card's **other** printings:
/// every card sharing its gameplay identity (Scryfall `oracle_id`) in the same
/// game, excluding the card itself, newest printing first (capped at `MAX_PAGE_SIZE`).
/// `404` if the game or card id is unknown; an empty `{ "data": [] }` when the card
/// has no other printings (or carries no `oracle_id`, so its siblings can't be
/// identified — e.g. reversible cards, whose `oracle_id` lives only per-face).
pub async fn card_prints(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    // Without an oracle_id there's no key to find sibling printings by, so the
    // card has no listable other printings.
    let data: Vec<CardResponse> = match card.oracle_id.as_deref().filter(|s| !s.is_empty()) {
        None => Vec::new(),
        Some(oracle_id) => prints_query(&game, oracle_id, card.id)
            .all(&state.db)
            .await?
            .into_iter()
            .map(CardResponse::from)
            .collect(),
    };
    Ok(Json(json!({ "data": data })))
}
