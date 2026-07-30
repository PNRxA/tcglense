//! Plain-text export of a card search's results.
//!
//! The catalog browse grids page 60 cards at a time, so "the results of my search" is
//! something a visitor can see but can't easily *take with them* — copying 40 pages of
//! tiles by hand is not a workflow. These two endpoints hand back the whole result set
//! as one `.txt` download, using the exact same filtered/sorted query the grid renders
//! (`super::cards::all_cards_query` / `super::sets::set_cards_query`), so the file is
//! provably the same search rather than a second implementation that can drift.
//!
//! The formats (`text` / `names`), the uncapped streaming, and the
//! never-hold-a-connection drain all live in the shared engine —
//! [`crate::handlers::shared::card_export`] — which the collection and wish-list browse
//! exports reuse too. The one catalog-specific rendering fact: every `text` line leads
//! with `1`, a quantity the format requires, not a claim about how many the visitor owns
//! (a catalog printing isn't a holding, so there's no count or foil finish to state).

use axum::extract::State;
use axum::response::Response;

use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{load_set, render_catalog_export, require_game};
use crate::state::AppState;

use super::ListParams;
use super::cards::all_cards_query;
use super::sets::set_cards_query;

/// Export card search results
///
/// `GET /api/games/{game}/cards/export` -> the whole result set of the all-cards search
/// as a `.txt` download, honouring the same `q`/`name`/`sort`/`dir` params as
/// `/api/games/{game}/cards`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/export",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the card list"),
        ("name" = Option<String>, Query, description = "Optional exact-name filter (matched literally)"),
        ("sort" = Option<String>, Query, description = "Sort key (`name`/`number`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `1 Name (SET) 123` per printing) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A streamed `text/plain` attachment listing every matching card — the whole result set, uncapped.", content_type = "text/plain"),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_cards(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    let format = params.export_format()?;
    let query = all_cards_query(&game, game_meta, &params, state.dialect())?;
    render_catalog_export(&state, query, format, &format!("tcglense-{game}"))
}

/// Export set card search results
///
/// `GET /api/games/{game}/sets/{code}/cards/export` -> the whole result set of a set's
/// card search as a `.txt` download, honouring the same `q`/`include_related`/`sort`/`dir`
/// params as `/api/games/{game}/sets/{code}/cards`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}/cards/export",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code, e.g. `neo`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the set card list"),
        ("include_related" = Option<bool>, Query, description = "Span the set's whole group (root + related sub-sets)"),
        ("sort" = Option<String>, Query, description = "Sort key (`number`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `1 Name (SET) 123` per printing) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A streamed `text/plain` attachment listing every matching card in the set — the whole result set, uncapped.", content_type = "text/plain"),
        (status = 404, description = "Unknown game or set."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_set_cards(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    // Resolve the set before the format, so an unknown set is a 404 rather than a 422
    // about a typo'd `?format=` — the same ordering the deck export uses.
    let set = load_set(&state, &game, &code).await?;
    let format = params.export_format()?;
    let query = set_cards_query(&state, &game, game_meta, &set, &params).await?;
    // `set.code` is the catalog's own value (the path segment may differ in case), so
    // the filename can't carry anything the visitor typed.
    render_catalog_export(
        &state,
        query,
        format,
        &format!("tcglense-{game}-{}", set.code),
    )
}
