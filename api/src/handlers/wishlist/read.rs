//! Wish-list read endpoints: the wanted-card list, the aggregate summary, a single
//! card's wanted counts, and the batch wanted-counts lookup that backs the browse-grid
//! badges and ghost views.

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    SelectTwo,
};

use crate::auth::extractor::AuthUser;
use crate::db::Dialect;
use crate::entities::prelude::{Card, WishlistItem};
use crate::entities::{card, wishlist_item};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::{
    CardResponse, CollectionEntry, CollectionQuantities, CollectionSort, CollectionSummary,
    ListParams, MAX_OWNED_IDS, OwnedCountsRequest, OwnedCountsResponse, Page, SortDir,
    SummaryParams, apply_card_sort, build_page, copies_expr, dedupe_ids, load_card, require_game,
    resolve_set_scope, search_condition, summarize_holdings,
};
use crate::state::AppState;

use super::find_row;

/// List wish list
///
/// `GET /api/wishlist/{game}` -> the signed-in user's wanted cards for a game,
/// most-recently-updated first, paginated. Each entry carries the full card payload
/// plus the wanted counts.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
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
        (status = 200, description = "A page of the signed-in user's wanted cards.", body = Page<CollectionEntry>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query or sort."),
    ),
)]
pub async fn list_wishlist(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();
    let (sort, dir) = params.sort_spec()?;
    let dialect = state.dialect();
    // Parse the optional Scryfall-syntax query up front so a malformed one 422s
    // before we touch the DB (mirrors the catalog card lists).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s, dialect))
        .transpose()?;

    // Resolve the (optional) set scope: a single set, or — with `include_related` — the
    // set's whole group (root + related sub-sets), spanning exactly the sets the catalog
    // does. `None` means the whole wish list.
    let set_codes =
        resolve_set_scope(&state, &game, params.set(), params.include_related()).await?;

    let paginator =
        wishlist_query(user.id, &game, set_codes.as_deref(), search, sort, dir, dialect)
            .paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    // `find_also_related` is a LEFT join, so a row whose card is gone (e.g. removed
    // by a catalog re-import) comes back with `None` — skip it, exactly as the
    // summary/valuation reads do.
    let data: Vec<CollectionEntry> = rows
        .into_iter()
        .filter_map(|(item, card)| {
            card.map(|c| CollectionEntry {
                card: CardResponse::from(c),
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// The per-user wanted-holdings base query: every `wishlist_items` row for one
/// `user_id` + `game`, left-joined to its `cards` row, optionally scoped to a set-code
/// slice. This encodes the wish list's core per-user scoping invariant, so the list,
/// summary, and wanted-sets reads all build on the one join + filter.
///
/// `set_codes` scopes to the joined card's `set_code`: `None` = the whole wish list,
/// a single code = the per-set view, several codes = the include-related group view. An
/// empty slice would match nothing, but the scope resolver never produces one (a group
/// always contains at least the set itself).
pub(super) fn wanted_with_cards(
    user_id: i32,
    game: &str,
    set_codes: Option<&[String]>,
) -> SelectTwo<wishlist_item::Entity, card::Entity> {
    let mut query = WishlistItem::find()
        .find_also_related(Card)
        .filter(wishlist_item::Column::UserId.eq(user_id))
        .filter(wishlist_item::Column::Game.eq(game));
    if let Some(codes) = set_codes {
        query = query.filter(card::Column::SetCode.is_in(codes.iter().map(String::as_str)));
    }
    query
}

/// Build the wish-list query for a user + game: the [`wanted_with_cards`] base
/// (per-user scope + optional set scope), plus the optional already-parsed search
/// condition and the chosen sort. Kept separate from the handler so the join/filter/sort
/// can be unit-tested against a seeded DB without an `AppState`.
///
/// The search condition, the set scope, and the card sort touch only `cards` columns;
/// the `user_id` and `game` filters and the recency sort stay entity-qualified to
/// `wishlist_items`, so nothing is ambiguous across the join (both tables carry a
/// `game` column).
pub(super) fn wishlist_query(
    user_id: i32,
    game: &str,
    set_codes: Option<&[String]>,
    search: Option<Condition>,
    sort: CollectionSort,
    dir: SortDir,
    dialect: Dialect,
) -> SelectTwo<wishlist_item::Entity, card::Entity> {
    let mut query = wanted_with_cards(user_id, game, set_codes);
    if let Some(condition) = search {
        query = query.filter(condition);
    }
    match sort {
        // Newest change first (or oldest, if reversed), with a stable id tiebreaker
        // for deterministic paging.
        CollectionSort::Recent => query
            .order_by(wishlist_item::Column::UpdatedAt, dir.order())
            .order_by(wishlist_item::Column::Id, dir.order()),
        // Total copies wanted (regular + foil), with a stable id tiebreaker for
        // deterministic paging. The copies expression names holdings-only columns, so
        // it stays unambiguous under the card join.
        CollectionSort::Quantity => query
            .order_by(copies_expr(), dir.order())
            .order_by(wishlist_item::Column::Id, dir.order()),
        CollectionSort::Card(field) => apply_card_sort(query, field, dir, false, dialect),
    }
}

/// Get wish list summary
///
/// `GET /api/wishlist/{game}/summary` -> aggregate stats (distinct cards, total
/// copies, estimated USD value) for the signed-in user's wish list in a game.
/// An optional `?set` scopes the stats to a single set (the per-set wish-list view);
/// `?include_related=true` with a set spans its whole group (root + related sub-sets),
/// so the header value matches the set / include-related wish-list browse view.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/summary",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
    ),
    responses(
        (status = 200, description = "Aggregate stats for the user's wish list.", body = CollectionSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn wishlist_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;

    // Resolve the optional scope: one set, or its whole group under include-related, or
    // `None` for the whole wish list — the same resolution the wish-list list uses, so
    // the value spans identical sets. Then aggregate over exactly those rows.
    let set = params.set.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let set_codes =
        resolve_set_scope(&state, &game, set, params.include_related.unwrap_or(false)).await?;
    Ok(Json(
        summary(
            &state.db,
            user.id,
            &game,
            set_codes.as_deref(),
            params.bulk_threshold_cents(),
        )
        .await?,
    ))
}

/// Aggregate stats (distinct cards, total copies, estimated USD value) for a user's
/// wish list in a game, optionally scoped. `set_codes = Some(codes)` scopes to those sets
/// (never empty — `resolve_set_scope` yields at least the scoped set); `None` spans the
/// whole wish list. The fold itself is the shared [`summarize_holdings`] core: each row
/// is left-joined to its card, so a row whose card is gone (a catalog re-import) is
/// skipped for **all three** stats — matching the wish-list list (`list_wishlist`).
pub(super) async fn summary(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
    set_codes: Option<&[String]>,
    bulk_threshold_cents: i128,
) -> Result<CollectionSummary, AppError> {
    let rows = wanted_with_cards(user_id, game, set_codes).all(db).await?;
    Ok(summarize_holdings(&rows, bulk_threshold_cents))
}

/// Get wish list card
///
/// `GET /api/wishlist/{game}/cards/{id}` -> how many copies of one card the user
/// wants (zeros when the card isn't on their wish list). `id` is the external card
/// id; a `404` means the game or card is unknown.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/cards/{id}",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
    ),
    responses(
        (status = 200, description = "How many copies of the card the user wants (zeros if none).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or card."),
    ),
)]
pub async fn get_wishlist_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let row = find_row(&state, user.id, &game, card.id).await?;
    Ok(Json(match row {
        Some(r) => CollectionQuantities {
            quantity: r.quantity,
            foil_quantity: r.foil_quantity,
        },
        None => CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        },
    }))
}

/// `POST /api/wishlist/{game}/counts` -> the wanted counts for the subset of the
/// given external card ids that are actually on the signed-in user's wish list, keyed
/// by external id. Cards the user doesn't want are absent from the map (so an
/// all-unwanted page returns `{ "data": {} }`). This backs the wanted-count badges and
/// ghost dimming overlaid on the wish-list browse grids without an N+1 of per-card
/// lookups. `422` if more than [`MAX_OWNED_IDS`] ids are requested at once.
pub async fn wishlist_counts(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    require_game(&game)?;

    let external_ids = dedupe_ids(payload.ids);
    if external_ids.is_empty() {
        return Ok(Json(OwnedCountsResponse {
            data: HashMap::new(),
        }));
    }
    if external_ids.len() > MAX_OWNED_IDS {
        return Err(AppError::Validation(format!(
            "at most {MAX_OWNED_IDS} card ids may be looked up at once"
        )));
    }

    // Resolve external -> internal ids for this game, keeping the reverse map so the
    // response can be keyed by the external id the client sent. Unknown ids just don't
    // appear here (and so never in the result).
    let external_by_internal: HashMap<i32, String> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::ExternalId)
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::ExternalId.is_in(external_ids))
        .into_tuple::<(i32, String)>()
        .all(&state.db)
        .await?
        .into_iter()
        .collect();
    if external_by_internal.is_empty() {
        return Ok(Json(OwnedCountsResponse {
            data: HashMap::new(),
        }));
    }

    // One query for the user's wish-list rows among those cards; a card with no row is
    // simply not wanted and contributes nothing to the map.
    let internal_ids: Vec<i32> = external_by_internal.keys().copied().collect();
    let rows = WishlistItem::find()
        .filter(wishlist_item::Column::UserId.eq(user.id))
        .filter(wishlist_item::Column::Game.eq(game.as_str()))
        .filter(wishlist_item::Column::CardId.is_in(internal_ids))
        .all(&state.db)
        .await?;

    let data: HashMap<String, CollectionQuantities> = rows
        .into_iter()
        .filter_map(|r| {
            external_by_internal.get(&r.card_id).map(|external_id| {
                (
                    external_id.clone(),
                    CollectionQuantities {
                        quantity: r.quantity,
                        foil_quantity: r.foil_quantity,
                    },
                )
            })
        })
        .collect();

    Ok(Json(OwnedCountsResponse { data }))
}
