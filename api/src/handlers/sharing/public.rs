//! Unauthenticated public collection reads, addressed by handle: `/api/u/{handle}...`.
//! Every read reuses a collection core with the resolved owner's `user_id`; the only
//! new work is `require_public_handle` (resolve + visibility gate → 404). Lives in the
//! router's `public_holdings` group (handle-keyed, CDN-cacheable, ETag'd).

use axum::{Json, extract::State};

use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::collection;
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{
    CollectionDropGroup, CollectionEntry, CollectionSetsResponse, CollectionSubtypeGroup,
    CollectionSummary, ListParams, MAX_OWNED_IDS, OwnedCountsRequest, OwnedCountsResponse, Page,
    SetsParams, SummaryParams, dedupe_ids, require_game, resolve_set_scope,
};
use crate::state::AppState;

use super::{
    PublicGameSummary, PublicProfile, public_games, require_public_handle, resolve_public_user,
};

/// `GET /api/u/{handle}` -> the owner's public identity + a summary per public game.
/// 404 if the handle is unknown or the user has nothing public — no public game **and** no
/// public deck (no bare-profile leak). A user who has shared only decks still resolves, so
/// their profile page can list those decks (issue #391).
#[utoipa::path(
    get,
    path = "/api/u/{handle}",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
    ),
    responses(
        (status = 200, description = "The owner's public identity plus a summary per public game.", body = PublicProfile),
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
    if games.is_empty() && !super::decks::user_has_public_deck(&state.db, user.id).await? {
        return Err(AppError::NotFound("collection not found".to_string()));
    }

    Ok(Json(PublicProfile {
        username: user.username.clone().unwrap_or_default(),
        discriminator: user.discriminator.unwrap_or_default(),
        handle: crate::auth::username::handle_of(&user).unwrap_or_default(),
        member_since: user.created_at,
        games,
    }))
}

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
    let set = params.set.as_deref().map(str::trim).filter(|s| !s.is_empty());
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
