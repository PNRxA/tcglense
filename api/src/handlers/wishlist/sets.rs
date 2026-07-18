//! Wish-list per-set endpoints: the wanted-set landing tiles and the wanted cards of a
//! drop-grouped set, grouped by Secret Lair drop.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::auth::extractor::AuthUser;
use crate::entities::card_set;
use crate::entities::prelude::CardSet;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    CollectionDropGroup, CollectionSetsResponse, CollectionSort, CollectionSubtypeGroup,
    ListParams, Page, SetsParams, SortDir, SortField, build_collection_sets, holding_drop_page,
    holding_subtype_page, load_set, require_drop_table, require_game, search_condition,
};
use crate::state::AppState;

use super::read::{wanted_summary_rows, wishlist_query};

/// List wish-list sets
///
/// `GET /api/wishlist/{game}/sets` -> the sets the signed-in user wants cards in,
/// newest set first, each with the catalog set metadata plus wanted counts. Backs the
/// wish list's per-set landing (its `owned_*` fields read as wanted counts there).
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/sets",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Per-unit bulk price cutoff in USD cents (default $1); splits each set tile's bulk subtotal"),
    ),
    responses(
        (status = 200, description = "The sets the user wants cards in, newest first, each with catalog metadata + wanted counts.", body = CollectionSetsResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn wishlist_sets(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<SetsParams>,
) -> Result<Json<CollectionSetsResponse>, AppError> {
    require_game(&game)?;

    // Every wanted card's fold-relevant columns for the game (never the wide card
    // rows) — bounded by how many distinct cards the user wants.
    let rows = wanted_summary_rows(user.id, &game, None)
        .all(&state.db)
        .await?;

    // The game's set metadata, to dress each wanted set as a full catalog tile.
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;

    Ok(Json(CollectionSetsResponse {
        data: build_collection_sets(&game, rows, sets, params.bulk_threshold_cents()),
    }))
}

/// List wish-list set drops
///
/// `GET /api/wishlist/{game}/sets/{code}/drops` -> the signed-in user's wanted cards
/// in a drop-grouped set (e.g. Secret Lair), grouped by Secret Lair drop and
/// **paginated by drop** — the wish-list mirror of the catalog's set-drops endpoint,
/// but scoped to (and carrying the wanted counts of) what the user wants.
///
/// Only wanted cards appear, so a drop the user wants nothing in is simply absent;
/// cards whose collector number isn't in the snapshot fall into a trailing "Other"
/// group. `404` if the set isn't drop-grouped (check `has_drops` first). An optional
/// `q` narrows the wanted cards, dropping now-empty drops.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/sets/{code}/drops",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by drop)"),
        ("page_size" = Option<u64>, Query, description = "Drops per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter over the wanted cards"),
    ),
    responses(
        (status = 200, description = "A page of the user's wanted cards in the set, grouped by Secret Lair drop.", body = Page<CollectionDropGroup>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the set isn't drop-grouped."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn wishlist_set_drops(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionDropGroup>>, AppError> {
    let game_meta = require_game(&game)?;
    // Canonicalise the set (and 404 an unknown one) exactly as the catalog does.
    let set = load_set(&state, &game, &code).await?;
    let table = require_drop_table(&game, &set.code)?;
    let dialect = state.dialect();

    // Parse the optional Scryfall-syntax query up front so a malformed one 422s before
    // we touch the DB (mirrors the list handler).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s, dialect))
        .transpose()?;

    // The user's wanted cards in this set, in collector-number order (with their
    // wish-list rows) — bounded by one set, so we group + paginate by drop in memory,
    // keeping every drop complete regardless of where the page boundary falls.
    let scope = [set.code.clone()];
    let rows = wishlist_query(
        user.id,
        &game,
        Some(&scope),
        search,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
        dialect,
    )
    .all(&state.db)
    .await?;

    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(holding_drop_page(&table, rows, page, page_size)))
}

/// List wish-list set sub-types
///
/// `GET /api/wishlist/{game}/sets/{code}/subtypes` -> the signed-in user's wanted cards in
/// a set, grouped by card sub-type (treatment) and **paginated by sub-type** — the
/// wish-list mirror of the catalog's set-subtypes endpoint, scoped to (and carrying the
/// wanted counts of) what the user wants.
///
/// Only wanted cards appear, so a sub-type the user wants nothing in is simply absent. Any
/// set works (no drop-table gate); the SPA gates the toggle on the tile's `has_subtypes`.
/// An optional `q` narrows the wanted cards, dropping now-empty sub-types.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/sets/{code}/subtypes",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by sub-type)"),
        ("page_size" = Option<u64>, Query, description = "Sub-types per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter over the wanted cards"),
    ),
    responses(
        (status = 200, description = "A page of the user's wanted cards in the set, grouped by card sub-type.", body = Page<CollectionSubtypeGroup>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or set."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn wishlist_set_subtypes(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionSubtypeGroup>>, AppError> {
    let game_meta = require_game(&game)?;
    // Canonicalise the set (and 404 an unknown one) exactly as the catalog does.
    let set = load_set(&state, &game, &code).await?;
    let dialect = state.dialect();

    // Parse the optional Scryfall-syntax query up front so a malformed one 422s before we
    // touch the DB (mirrors the by-drop handler).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s, dialect))
        .transpose()?;

    // The user's wanted cards in this set, in collector-number order (with their wish-list
    // rows) — bounded by one set, so we group + paginate by sub-type in memory.
    let scope = [set.code.clone()];
    let rows = wishlist_query(
        user.id,
        &game,
        Some(&scope),
        search,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
        dialect,
    )
    .all(&state.db)
    .await?;

    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(holding_subtype_page(rows, page, page_size)))
}
