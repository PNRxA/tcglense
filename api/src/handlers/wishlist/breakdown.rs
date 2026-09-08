//! The wish-list breakdown (issue #680): the collection breakdown's twin over
//! `wishlist_items` — what buying the list would cost, by rarity, colour identity, card
//! type and finish, plus the ten most valuable wanted lines by wanted value. The fold,
//! DTOs and caching seam live in [`crate::handlers::shared::breakdown`]; this module
//! contributes only the entity query, and its cache rides the wish list's **own**
//! holdings version ([`HoldingsSurface::Wishlist`]) so a wish-list edit orphans it.

use axum::extract::State;
use sea_orm::{
    ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait, sea_query::JoinType,
};

use crate::analytics_cache::{HoldingsSurface, json_body_response};
use crate::auth::extractor::AuthUser;
use crate::entities::prelude::WishlistItem;
use crate::entities::wishlist_item;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::breakdown::{
    BreakdownParams, TOP_HOLDINGS, breakdown_cache_key, finish_breakdown, fold_breakdown,
    narrow_breakdown_rows,
};
use crate::handlers::shared::{HoldingBreakdown, require_game};
use crate::state::AppState;

/// Get wish-list breakdown
///
/// `GET /api/wishlist/{game}/breakdown?bulk_max_cents` -> the collection breakdown's
/// twin over the signed-in user's wish list: wanted copies + their estimated USD cost by
/// rarity, colour identity, card type and finish, plus the ten most valuable wanted
/// lines by wanted value (price × copies). The embedded `summary` is the same fold
/// `/summary` answers over the same rows. Cards only. `404` if the game is unknown.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/breakdown",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Per-unit price cutoff (USD cents) under which a finish counts as bulk in the embedded summary; absent = $1"),
    ),
    responses(
        (status = 200, description = "Wanted copies and cost by rarity, colour, card type and finish, and the top wanted lines by wanted value.", body = HoldingBreakdown),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn wishlist_breakdown(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<BreakdownParams>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;

    let cache_key =
        breakdown_cache_key(&state, HoldingsSurface::Wishlist, user.id, &game, &params).await;
    let bulk_threshold_cents = params.bulk_threshold_cents();
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, game) = (state.clone(), game.clone());
            async move {
                let rows = wanted_breakdown_rows(user.id, &game).all(&state.db).await?;
                let folded = fold_breakdown(&rows, bulk_threshold_cents, TOP_HOLDINGS);
                let payload = finish_breakdown(&state.db, folded).await?;
                serde_json::to_vec(&payload).map_err(|err| {
                    AppError::Internal(format!("serialize wish-list breakdown: {err}"))
                })
            }
        })
        .await?;
    Ok(json_body_response(body))
}

/// The breakdown's rows for a user + game: the same per-user scope and LEFT JOIN as
/// [`super::read::wanted_summary_rows`] (keep the filters in lock-step), projected
/// through the shared breakdown columns.
fn wanted_breakdown_rows(
    user_id: i32,
    game: &str,
) -> sea_orm::Selector<sea_orm::SelectModel<crate::handlers::shared::HoldingBreakdownRow>> {
    let query = WishlistItem::find()
        .join(JoinType::LeftJoin, wishlist_item::Relation::Card.def())
        .filter(wishlist_item::Column::UserId.eq(user_id))
        .filter(wishlist_item::Column::Game.eq(game));
    narrow_breakdown_rows(
        query,
        wishlist_item::Column::Quantity,
        wishlist_item::Column::FoilQuantity,
    )
}
