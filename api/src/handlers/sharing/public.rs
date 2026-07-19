//! Unauthenticated public collection reads, addressed by handle: `/api/u/{handle}...`.
//! Every read reuses a collection core with the resolved owner's `user_id`; the only
//! new work is `require_public_handle` (resolve + visibility gate → 404). Lives in the
//! router's `public_holdings` group (handle-keyed, CDN-cacheable, ETag'd).

use axum::{Json, extract::State};

use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::collection;
use crate::handlers::shared::product_holdings::{
    ProductHoldingEntry, ProductHoldingListParams, ProductHoldingSet, ProductHoldingSummary,
};
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{
    CollectionDropGroup, CollectionEntry, CollectionSetsResponse, CollectionSubtypeGroup,
    CollectionSummary, DataBody, ListParams, MAX_OWNED_IDS, OwnedCountsRequest,
    OwnedCountsResponse, Page, SetsParams, SummaryParams, dedupe_ids, require_game,
    resolve_set_scope,
};
use crate::state::AppState;

use super::{
    PublicGameSummary, PublicProfile, public_games, public_wishlist_games, require_public_handle,
    require_public_wishlist_handle, resolve_public_user,
};
use crate::handlers::wishlist;

/// Get public profile
///
/// `GET /api/u/{handle}` -> the owner's public identity + a summary per public game
/// (collection and/or wish list). 404 if the handle is unknown or the user has nothing public
/// — no public collection, **no public wish list**, and no public deck (no bare-profile leak).
/// A user who has shared only decks (or only a wish list) still resolves, so their profile
/// page can list them (issues #391/#493).
#[utoipa::path(
    get,
    path = "/api/u/{handle}",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
    ),
    responses(
        (status = 200, description = "The owner's public identity plus a summary per public collection and wish list.", body = PublicProfile),
        (status = 404, description = "Unknown handle, or the user has nothing public."),
    ),
)]
pub async fn public_profile(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> Result<Json<PublicProfile>, AppError> {
    let user = resolve_public_user(&state, &handle).await?;
    let bulk = resolve_bulk_threshold_cents(None);

    let mut games = Vec::new();
    for game in public_games(&state.db, user.id).await? {
        // A visibility row for a game slug no longer in the registry is ignored.
        if crate::catalog::find(&game).is_none() {
            continue;
        }
        let summary = collection::summary(&state.db, user.id, &game, None, bulk).await?;
        games.push(PublicGameSummary { game, summary });
    }

    // The public wish lists (issue #493) — the same per-game summary shape, computed from the
    // wish-list fold instead of the collection's. Independent of the collections above, so a
    // game can appear in one, both, or neither list.
    let mut wishlists = Vec::new();
    for game in public_wishlist_games(&state.db, user.id).await? {
        if crate::catalog::find(&game).is_none() {
            continue;
        }
        let summary = wishlist::summary(&state.db, user.id, &game, None, bulk).await?;
        wishlists.push(PublicGameSummary { game, summary });
    }

    if games.is_empty()
        && wishlists.is_empty()
        && !super::decks::user_has_public_deck(&state.db, user.id).await?
    {
        return Err(AppError::NotFound("collection not found".to_string()));
    }

    Ok(Json(PublicProfile {
        username: user.username.clone().unwrap_or_default(),
        discriminator: user.discriminator.unwrap_or_default(),
        handle: crate::auth::username::handle_of(&user).unwrap_or_default(),
        member_since: user.created_at,
        games,
        wishlists,
    }))
}

/// List public collection
///
/// `GET /api/u/{handle}/{game}` -> a page of the owner's owned cards (mirrors
/// `list_collection`; same `?page/page_size/q/set/include_related/sort/dir`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("sort" = Option<String>, Query, description = "Sort key (`updated`/`quantity`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
    ),
    responses(
        (status = 200, description = "A page of the owner's owned cards.", body = Page<CollectionEntry>),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
        (status = 422, description = "Malformed search query or sort."),
    ),
)]
pub async fn public_list(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_list_page(&state, meta, user_id, &game, &params).await?,
    ))
}

/// Get public collection summary
///
/// `GET /api/u/{handle}/{game}/summary` (mirrors `collection_summary`;
/// `?set/include_related/bulk_max_cents`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/summary",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Bulk-value threshold (cents) for the estimate split"),
    ),
    responses(
        (status = 200, description = "Aggregate stats for the owner's public collection.", body = CollectionSummary),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
    ),
)]
pub async fn public_summary(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    let set = params
        .set
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let set_codes =
        resolve_set_scope(&state, &game, set, params.include_related.unwrap_or(false)).await?;
    Ok(Json(
        collection::summary(
            &state.db,
            user_id,
            &game,
            set_codes.as_deref(),
            params.bulk_threshold_cents(),
        )
        .await?,
    ))
}

/// List public collection sets
///
/// `GET /api/u/{handle}/{game}/sets` (mirrors `collection_sets`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/sets",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Bulk-value threshold (cents) for the estimate split"),
    ),
    responses(
        (status = 200, description = "The sets the owner owns cards in, each with catalog metadata + owned counts.", body = CollectionSetsResponse),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
    ),
)]
pub async fn public_sets(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<SetsParams>,
) -> Result<Json<CollectionSetsResponse>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_sets(&state, user_id, &game, params.bulk_threshold_cents()).await?,
    ))
}

/// List public sealed products
///
/// `GET /api/u/{handle}/{game}/products` -> a page of the owner's owned sealed products
/// (mirrors `list_collection_products`; same `?page/page_size/set`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/products",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("set" = Option<String>, Query, description = "Restrict to one set code; an unknown/unheld code yields an empty page"),
    ),
    responses(
        (status = 200, description = "A page of the owner's owned sealed products.", body = Page<ProductHoldingEntry>),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
    ),
)]
pub async fn public_products(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<ProductHoldingListParams>,
) -> Result<Json<Page<ProductHoldingEntry>>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_products_page(&state, user_id, &game, params).await?,
    ))
}

/// Get public sealed summary
///
/// `GET /api/u/{handle}/{game}/products/summary` (mirrors `collection_product_summary`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/products/summary",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "Aggregate stats for the owner's public sealed products.", body = ProductHoldingSummary),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
    ),
)]
pub async fn public_product_summary(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
) -> Result<Json<ProductHoldingSummary>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_product_summary(&state, user_id, &game).await?,
    ))
}

/// List public sealed-product sets
///
/// `GET /api/u/{handle}/{game}/products/sets` (mirrors `list_collection_product_sets`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/products/sets",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "Every set the owner owns sealed products in, newest set first, each an aggregate tile.", body = DataBody<Vec<ProductHoldingSet>>),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
    ),
)]
pub async fn public_product_sets(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<ProductHoldingSet>>>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(DataBody {
        data: collection::owned_product_sets(&state, user_id, &game).await?,
    }))
}

/// List public collection set drops
///
/// `GET /api/u/{handle}/{game}/sets/{code}/drops` (mirrors `collection_set_drops`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/sets/{code}/drops",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code (drop-grouped set, e.g. Secret Lair)"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by drop)"),
        ("page_size" = Option<u64>, Query, description = "Drops per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
    ),
    responses(
        (status = 200, description = "A page of the owner's owned cards in the set, grouped by drop.", body = Page<CollectionDropGroup>),
        (status = 404, description = "Unknown/private handle, non-public game, unknown set, or a set that isn't drop-grouped."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn public_set_drops(
    State(state): State<AppState>,
    Path((handle, game, code)): Path<(String, String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionDropGroup>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_drop_page(&state, meta, user_id, &game, &code, &params).await?,
    ))
}

/// List public collection set sub-types
///
/// `GET /api/u/{handle}/{game}/sets/{code}/subtypes` (mirrors `collection_set_subtypes`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/{game}/sets/{code}/subtypes",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by sub-type)"),
        ("page_size" = Option<u64>, Query, description = "Sub-types per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
    ),
    responses(
        (status = 200, description = "A page of the owner's owned cards in the set, grouped by sub-type.", body = Page<CollectionSubtypeGroup>),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown set."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn public_set_subtypes(
    State(state): State<AppState>,
    Path((handle, game, code)): Path<(String, String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionSubtypeGroup>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;
    Ok(Json(
        collection::owned_subtype_page(&state, meta, user_id, &game, &code, &params).await?,
    ))
}

/// Batch public owned counts
///
/// `POST /api/u/{handle}/{game}/owned` -> the owner's owned counts for the subset of the
/// posted external card ids they actually own, keyed by external id (mirrors the authed
/// `owned_counts`). Backs the show-ghosts overlay on the public browse grid: which catalog
/// cards the owner holds. Cards the owner doesn't own are absent from the map. `422` over
/// [`MAX_OWNED_IDS`]; a private/unknown handle is `404`. Served `no-store` (the response
/// varies by the POST body, so it must never be shared-cached).
#[utoipa::path(
    post,
    path = "/api/u/{handle}/{game}/owned",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = OwnedCountsRequest,
    responses(
        (status = 200, description = "The owner's owned counts for the subset of the posted ids they hold, keyed by external id.", body = OwnedCountsResponse),
        (status = 404, description = "Unknown/private handle, non-public game, or unknown game."),
        (status = 422, description = "More than the maximum number of card ids requested at once."),
    ),
)]
pub async fn public_owned_counts(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    require_game(&game)?;
    let user_id = require_public_handle(&state, &handle, &game).await?;

    let external_ids = dedupe_ids(payload.ids);
    if external_ids.len() > MAX_OWNED_IDS {
        return Err(AppError::Validation(format!(
            "at most {MAX_OWNED_IDS} card ids may be looked up at once"
        )));
    }

    Ok(Json(
        collection::owned_counts_map(&state, user_id, &game, external_ids).await?,
    ))
}

// ============================================================================================
// Public wish lists (issue #493)
//
// The read-only mirror of the public collection reads above, resolved by handle and gated on
// the independent `wishlist_is_public` flag (`require_public_wishlist_handle`). Every read
// reuses a `wishlist::wanted_*` core with the resolved owner's `user_id` — the only new work
// is the visibility gate. Addressed under a static `wishlist` segment
// (`/api/u/{handle}/wishlist/{game}...`) that wins over the `{game}` capture in axum, mirroring
// the `decks` precedent and the authed `/api/wishlist/{game}` layout.
// ============================================================================================

/// List public wish list
///
/// `GET /api/u/{handle}/wishlist/{game}` -> a page of the owner's wanted cards (mirrors
/// `list_wishlist`; same `?page/page_size/q/set/include_related/sort/dir`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("sort" = Option<String>, Query, description = "Sort key (`updated`/`quantity`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
    ),
    responses(
        (status = 200, description = "A page of the owner's wanted cards.", body = Page<CollectionEntry>),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
        (status = 422, description = "Malformed search query or sort."),
    ),
)]
pub async fn public_wishlist_list(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_list_page(&state, meta, user_id, &game, &params).await?,
    ))
}

/// Get public wish list summary
///
/// `GET /api/u/{handle}/wishlist/{game}/summary` (mirrors `wishlist_summary`;
/// `?set/include_related/bulk_max_cents`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/summary",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Bulk-value threshold (cents) for the estimate split"),
    ),
    responses(
        (status = 200, description = "Aggregate stats for the owner's public wish list.", body = CollectionSummary),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
    ),
)]
pub async fn public_wishlist_summary(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    let set = params
        .set
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let set_codes =
        resolve_set_scope(&state, &game, set, params.include_related.unwrap_or(false)).await?;
    Ok(Json(
        wishlist::summary(
            &state.db,
            user_id,
            &game,
            set_codes.as_deref(),
            params.bulk_threshold_cents(),
        )
        .await?,
    ))
}

/// List public wish list sets
///
/// `GET /api/u/{handle}/wishlist/{game}/sets` (mirrors `wishlist_sets`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/sets",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("bulk_max_cents" = Option<i64>, Query, description = "Bulk-value threshold (cents) for the estimate split"),
    ),
    responses(
        (status = 200, description = "The sets the owner wants cards in, each with catalog metadata + wanted counts.", body = CollectionSetsResponse),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
    ),
)]
pub async fn public_wishlist_sets(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<SetsParams>,
) -> Result<Json<CollectionSetsResponse>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_sets(&state, user_id, &game, params.bulk_threshold_cents()).await?,
    ))
}

/// List public wish list set drops
///
/// `GET /api/u/{handle}/wishlist/{game}/sets/{code}/drops` (mirrors `wishlist_set_drops`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/sets/{code}/drops",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code (drop-grouped set, e.g. Secret Lair)"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by drop)"),
        ("page_size" = Option<u64>, Query, description = "Drops per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
    ),
    responses(
        (status = 200, description = "A page of the owner's wanted cards in the set, grouped by drop.", body = Page<CollectionDropGroup>),
        (status = 404, description = "Unknown/private handle, non-public wish list, unknown set, or a set that isn't drop-grouped."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn public_wishlist_set_drops(
    State(state): State<AppState>,
    Path((handle, game, code)): Path<(String, String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionDropGroup>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_drop_page(&state, meta, user_id, &game, &code, &params).await?,
    ))
}

/// List public wish list set sub-types
///
/// `GET /api/u/{handle}/wishlist/{game}/sets/{code}/subtypes` (mirrors `wishlist_set_subtypes`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/sets/{code}/subtypes",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by sub-type)"),
        ("page_size" = Option<u64>, Query, description = "Sub-types per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
    ),
    responses(
        (status = 200, description = "A page of the owner's wanted cards in the set, grouped by sub-type.", body = Page<CollectionSubtypeGroup>),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown set."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn public_wishlist_set_subtypes(
    State(state): State<AppState>,
    Path((handle, game, code)): Path<(String, String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionSubtypeGroup>>, AppError> {
    let meta = require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_subtype_page(&state, meta, user_id, &game, &code, &params).await?,
    ))
}

/// List public wanted sealed products
///
/// `GET /api/u/{handle}/wishlist/{game}/products` -> a page of the owner's wanted sealed
/// products (mirrors `list_wishlist_products`; same `?page/page_size/set`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/products",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("set" = Option<String>, Query, description = "Restrict to one set code; an unknown/unheld code yields an empty page"),
    ),
    responses(
        (status = 200, description = "A page of the owner's wanted sealed products.", body = Page<ProductHoldingEntry>),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
    ),
)]
pub async fn public_wishlist_products(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    Query(params): Query<ProductHoldingListParams>,
) -> Result<Json<Page<ProductHoldingEntry>>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_products_page(&state, user_id, &game, params).await?,
    ))
}

/// Get public wanted sealed summary
///
/// `GET /api/u/{handle}/wishlist/{game}/products/summary` (mirrors `wishlist_product_summary`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/products/summary",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "Aggregate stats for the owner's public wanted sealed products.", body = ProductHoldingSummary),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
    ),
)]
pub async fn public_wishlist_product_summary(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
) -> Result<Json<ProductHoldingSummary>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(
        wishlist::wanted_product_summary(&state, user_id, &game).await?,
    ))
}

/// List public wanted sealed-product sets
///
/// `GET /api/u/{handle}/wishlist/{game}/products/sets` (mirrors `list_wishlist_product_sets`).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/wishlist/{game}/products/sets",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "Every set the owner wants sealed products in, newest set first, each an aggregate tile.", body = DataBody<Vec<ProductHoldingSet>>),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
    ),
)]
pub async fn public_wishlist_product_sets(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<ProductHoldingSet>>>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;
    Ok(Json(DataBody {
        data: wishlist::wanted_product_sets(&state, user_id, &game).await?,
    }))
}

/// Batch public wanted counts
///
/// `POST /api/u/{handle}/wishlist/{game}/owned` -> the owner's wanted counts for the subset of
/// the posted external card ids they actually want, keyed by external id (mirrors the authed
/// `wishlist_counts`). Backs the show-ghosts overlay on the public wish-list browse grid.
/// Cards the owner doesn't want are absent. `422` over [`MAX_OWNED_IDS`]; a private/unknown
/// handle is `404`. Served `no-store` (the response varies by the POST body).
#[utoipa::path(
    post,
    path = "/api/u/{handle}/wishlist/{game}/owned",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = OwnedCountsRequest,
    responses(
        (status = 200, description = "The owner's wanted counts for the subset of the posted ids they want, keyed by external id.", body = OwnedCountsResponse),
        (status = 404, description = "Unknown/private handle, non-public wish list, or unknown game."),
        (status = 422, description = "More than the maximum number of card ids requested at once."),
    ),
)]
pub async fn public_wishlist_owned_counts(
    State(state): State<AppState>,
    Path((handle, game)): Path<(String, String)>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    require_game(&game)?;
    let user_id = require_public_wishlist_handle(&state, &handle, &game).await?;

    let external_ids = dedupe_ids(payload.ids);
    if external_ids.len() > MAX_OWNED_IDS {
        return Err(AppError::Validation(format!(
            "at most {MAX_OWNED_IDS} card ids may be looked up at once"
        )));
    }

    Ok(Json(
        wishlist::wanted_counts_map(&state, user_id, &game, external_ids).await?,
    ))
}
