//! The wish list's shopping list (issue #292): `GET /api/wishlist/{game}/buy-list`, the
//! JSON rows behind the SPA's "Buy all" buttons (TCGplayer mass entry, MTG Mate's decklist
//! search — the store registry in `web/src/lib/buyLinks.ts`), over the very query the
//! browse grid and the `.txt` export run, through [`crate::handlers::shared::buy_list`].

use axum::{Json, extract::State};

use crate::auth::extractor::AuthUser;
use crate::entities::wishlist_item;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    BUY_LIST_MAX_ROWS, BuyList, ListParams, MAX_PAGE_SIZE, ProductHoldingListParams,
    build_buy_list, load_buy_list_cards, product_rows, require_game, resolve_holdings_list,
};
use crate::state::AppState;

use super::products::wanted_products_page;
use super::read::wishlist_query;

/// Wish-list shopping list
///
/// `GET /api/wishlist/{game}/buy-list` -> the signed-in user's wanted cards as bulk-buy
/// rows — name, set, collector number, the wanted counts, and the printing's TCGplayer
/// product id where TCGplayer lists it — honouring the same
/// `q`/`set`/`include_related`/`sort`/`dir`/`min_copies`/`max_copies`/`finish` params as
/// `/api/wishlist/{game}`, so "buy what's on screen" is the filtered grid. An
/// **unfiltered** request (no `q`, `set` or copy-count filter) is "buy the whole list" and
/// carries the wanted sealed products too, by their TCGplayer ids. Capped at 500 card rows
/// (and one page of products); `truncated` + the totals say what was cut.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/buy-list",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the wish-list list"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("sort" = Option<String>, Query, description = "Row order (`updated`/`quantity`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("min_copies" = Option<i32>, Query, description = "Copy-count floor, as on the wish-list list"),
        ("max_copies" = Option<i32>, Query, description = "Copy-count ceiling, as on the wish-list list"),
        ("finish" = Option<String>, Query, description = "`any`/`regular`/`foil` — the counter the copy bounds read, as on the wish-list list"),
    ),
    responses(
        (status = 200, description = "The shopping list: the matching wanted cards (at most 500 rows) and, for an unfiltered request, the wanted sealed products.", body = BuyList),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query, sort, or copy-count filter."),
    ),
)]
pub async fn wishlist_buy_list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<BuyList>, AppError> {
    let game_meta = require_game(&game)?;
    // The same resolution + query builder as `list_wishlist` and the `.txt` export, so
    // the rows are provably the grid's, never a second implementation of its filters.
    let parts = resolve_holdings_list(&state, game_meta, &game, &params).await?;
    let filtered = params.search().is_some() || params.set().is_some() || parts.copies.is_active();
    let query = wishlist_query(
        user.id,
        &game,
        parts.set_codes.as_deref(),
        parts.search,
        parts.copies,
        parts.sort,
        parts.dir,
        state.dialect(),
    );
    let (cards, total_cards) = load_buy_list_cards(
        &state.db,
        query,
        wishlist_item::Column::Quantity,
        wishlist_item::Column::FoilQuantity,
        BUY_LIST_MAX_ROWS,
    )
    .await?;

    // Sealed products ride only a whole-list request: every card filter is a *card*
    // filter (a set scope, a Scryfall query, a copy count read off card holdings), and
    // "buy these filtered cards" must not drag every wanted booster box along.
    let (products, total_products) = if filtered {
        (Vec::new(), 0)
    } else {
        let page = wanted_products_page(
            &state,
            user.id,
            &game,
            ProductHoldingListParams {
                page: Some(1),
                page_size: Some(MAX_PAGE_SIZE),
                set: None,
            },
        )
        .await?;
        (product_rows(page.data), page.total as u64)
    };

    Ok(Json(build_buy_list(
        cards,
        total_cards,
        products,
        total_products,
    )))
}
