//! The collection breakdown (issue #680): value and copies by rarity, colour identity,
//! card type and finish, plus the top holdings by held value — the collection's answer to
//! "where is my money?". The fold, DTOs and caching seam live in
//! [`crate::handlers::shared::breakdown`] (shared with the wish-list twin); this module
//! contributes only the `collection_items` query.

use axum::extract::State;
use sea_orm::{
    ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait, sea_query::JoinType,
};

use crate::analytics_cache::{HoldingsSurface, json_body_response};
use crate::auth::extractor::AuthUser;
use crate::entities::collection_item;
use crate::entities::prelude::CollectionItem;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::breakdown::{
    BreakdownParams, TOP_HOLDINGS, breakdown_cache_key, finish_breakdown, fold_breakdown,
    narrow_breakdown_rows,
};
use crate::handlers::shared::{HoldingBreakdown, require_game};
use crate::state::AppState;

/// Get collection breakdown
///
/// `GET /api/collection/{game}/breakdown?bulk_max_cents` -> where the signed-in user's
/// collection value sits: copies + estimated USD value by **rarity**, by **colour
/// identity** bucket (the five colours, multicolour, colourless), by **card type** (the
/// type line's first card type) and by **finish** (regular vs foil), plus the ten most
/// valuable holdings ranked by *held* value (price × copies — not a single copy's price,
/// which is the list's `sort=price`). The embedded `summary` is the same fold `/summary`
/// answers over the same rows, so every bucket is a slice of that total; a holding whose
/// card row is gone is skipped everywhere. Cards only — sealed products have none of
/// these facets. `404` if the game is unknown.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/breakdown",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Per-unit price cutoff (USD cents) under which a finish counts as bulk in the embedded summary; absent = $1"),
    ),
    responses(
        (status = 200, description = "Value and copies by rarity, colour, card type and finish, and the top holdings by held value.", body = HoldingBreakdown),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn collection_breakdown(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<BreakdownParams>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;

    // Version-keyed, single-flight response cache (like value history and movers):
    // a third whole-collection scan per user, unchanged between the user's own edits
    // and the daily price capture.
    let cache_key =
        breakdown_cache_key(&state, HoldingsSurface::Collection, user.id, &game, &params).await;
    let bulk_threshold_cents = params.bulk_threshold_cents();
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, game) = (state.clone(), game.clone());
            async move {
                let rows = owned_breakdown_rows(user.id, &game).all(&state.db).await?;
                let folded = fold_breakdown(&rows, bulk_threshold_cents, TOP_HOLDINGS);
                let payload = finish_breakdown(&state.db, folded).await?;
                serde_json::to_vec(&payload).map_err(|err| {
                    AppError::Internal(format!("serialize collection breakdown: {err}"))
                })
            }
        })
        .await?;
    Ok(json_body_response(body))
}

/// The breakdown's rows for a user + game: the same per-user scope and LEFT JOIN as
/// [`super::read::owned_summary_rows`] (keep the filters in lock-step), projected
/// through the shared breakdown columns.
fn owned_breakdown_rows(
    user_id: i32,
    game: &str,
) -> sea_orm::Selector<sea_orm::SelectModel<crate::handlers::shared::HoldingBreakdownRow>> {
    let query = CollectionItem::find()
        .join(JoinType::LeftJoin, collection_item::Relation::Card.def())
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game));
    narrow_breakdown_rows(
        query,
        collection_item::Column::Quantity,
        collection_item::Column::FoilQuantity,
    )
}
