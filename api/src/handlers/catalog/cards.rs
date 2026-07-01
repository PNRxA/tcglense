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
    CardResponse, Page, SortField, apply_card_sort, build_page, load_card, require_game, trim_query,
};
use crate::state::AppState;

use super::{
    ListParams, NameSuggestParams, apply_search, apply_unique, name_suggestions_query, prints_query,
};

/// Default / max number of name suggestions the autocomplete endpoint returns.
const DEFAULT_NAME_SUGGESTIONS: u64 = 10;
const MAX_NAME_SUGGESTIONS: u64 = 25;

/// `GET /api/games/{game}/cards` -> all cards (optional `q` search), by name.
pub async fn list_cards(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let query = Card::find().filter(card::Column::Game.eq(game.as_str()));
    let (mut query, shape) = apply_search(query, game_meta, &params)?;
    // Optional exact-name scope (the quick-add "printings of this name" step): an
    // equality bind, so a name full of punctuation/quotes matches literally and
    // there's no injection surface. ANDed with any `q`.
    if let Some(name) = params.exact_name() {
        query = query.filter(card::Column::Name.eq(name));
    }
    let (sort, dir) = params.sort_spec_with(SortField::Name, shape.order, shape.direction)?;
    let query = apply_unique(query, shape.unique);
    let paginator = apply_card_sort(query, sort, dir, false).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    let data: Vec<CardResponse> = rows.into_iter().map(CardResponse::from).collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/games/{game}/card-names?q=&limit=` -> up to `limit` **distinct** card
/// names in the game whose name contains `q` (case-insensitively), with names that
/// *start* with `q` surfaced first, then alphabetically. Powers the collection
/// quick-add autocomplete (one hint per unique name). A blank/absent `q` returns an
/// empty list — there's nothing to suggest yet.
pub async fn card_names(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<NameSuggestParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let Some(term) = trim_query(params.q.as_deref()) else {
        return Ok(Json(json!({ "data": Vec::<String>::new() })));
    };
    let limit = params
        .limit
        .unwrap_or(DEFAULT_NAME_SUGGESTIONS)
        .clamp(1, MAX_NAME_SUGGESTIONS);

    let names: Vec<String> = name_suggestions_query(&game, term, limit)
        .into_tuple::<String>()
        .all(&state.db)
        .await?;

    Ok(Json(json!({ "data": names })))
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
