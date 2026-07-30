//! Wish-list card-search export: the wish-list browse's mirror of the public catalog's
//! plain-text card export, streamed through the shared engine
//! ([`crate::handlers::shared::card_export`]) over the very query the browse grid runs —
//! the wish-list twin of `collection::export_collection_cards`.

use axum::extract::State;
use axum::response::Response;

use crate::auth::extractor::AuthUser;
use crate::entities::wishlist_item;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    ListParams, narrow_export_statement, render_holdings_export, require_game,
    resolve_holdings_list,
};
use crate::state::AppState;

use super::read::wishlist_query;

/// Export wish-list card search results
///
/// `GET /api/wishlist/{game}/cards/export` -> the whole result set of the signed-in
/// user's wanted-card search as a `.txt` download, honouring the same
/// `q`/`set`/`include_related`/`sort`/`dir` params as `/api/wishlist/{game}` — the
/// wish-list browse's mirror of the catalog's card-search export. Lines carry the real
/// wanted counts, one line per non-empty finish (`4 Sol Ring (LTC) 284`, foil copies on a
/// second ` *F*`-tagged line), so a shopping list pastes straight into the importers.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/cards/export",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the wish-list list"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("sort" = Option<String>, Query, description = "Sort key (`updated`/`quantity`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `N Name (SET) 123` per wanted finish, foil tagged ` *F*`) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A streamed `text/plain` attachment listing every matching wanted card — the whole result set, uncapped.", content_type = "text/plain"),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_wishlist_cards(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    let format = params.export_format()?;
    // The same resolution + query builder as `list_wishlist`, so the file is provably
    // the search the browse grid rendered, never a second implementation.
    let parts = resolve_holdings_list(&state, game_meta, &game, &params).await?;
    let query = wishlist_query(
        user.id,
        &game,
        parts.set_codes.as_deref(),
        parts.search,
        parts.sort,
        parts.dir,
        state.dialect(),
    );
    let statement = narrow_export_statement(
        query,
        wishlist_item::Column::CardId,
        wishlist_item::Column::Quantity,
        wishlist_item::Column::FoilQuantity,
    );
    // Scope-free filename (no set code), matching the collection card export's reasoning.
    render_holdings_export(
        &state,
        statement,
        format,
        &format!("tcglense-{game}-wishlist"),
    )
}
