//! Catalog set endpoints: the set list, one set's metadata, its SVG icon proxy, and a
//! set's cards (flat, include-related, or grouped by Secret Lair drop).

use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{
    ColumnTrait, EntityTrait, Order, PaginatorTrait, QueryFilter, QueryOrder,
    sea_query::NullOrdering,
};
use serde::Serialize;

use crate::entities::prelude::{Card, CardSet};
use crate::entities::{card, card_set};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    CardResponse, DataBody, Page, SortDir, SortField, apply_card_sort, build_page,
    filter_drops_by_title, group_into_drops, group_into_subtypes, load_group_set_codes, load_set,
    paginate_buckets, require_drop_table, require_game,
};
use crate::state::AppState;

use super::image::is_allowed_image_url;
use super::{IMAGE_CACHE_CONTROL, ListParams, apply_search, apply_unique};

/// A set/expansion within a game.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardSet"))]
pub struct SetResponse {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i32,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
    /// Whether this set is browsable broken down by Secret Lair-style "drops"
    /// (the `.../drops` endpoint). Lets the SPA offer a by-drop view only where
    /// there's drop data to show.
    pub has_drops: bool,
    /// Whether this set has cards with special treatments (borderless, showcase, …), so
    /// it can be browsed grouped by sub-type (the `.../subtypes` endpoint). Unlike
    /// `has_drops` this is data-derived, so the `From` impl leaves it `false` — the
    /// handler fills it from a query.
    pub has_subtypes: bool,
}

impl From<card_set::Model> for SetResponse {
    fn from(m: card_set::Model) -> Self {
        let has_drops = crate::scryfall::drops::has_drops(&m.game, &m.code);
        SetResponse {
            code: m.code,
            name: m.name,
            set_type: m.set_type,
            released_at: m.released_at,
            card_count: m.card_count,
            icon_svg_uri: m.icon_svg_uri,
            parent_set_code: m.parent_set_code,
            has_drops,
            // Derived from card data, not the set row — filled by the handler.
            has_subtypes: false,
        }
    }
}

/// One Secret Lair drop with its cards, as returned by the drops endpoint. The
/// enclosing [`Page`] paginates over these (so `total` is a drop count, not a
/// card count).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "DropGroup"))]
pub struct DropGroupResponse {
    /// Stable slug for anchors/links; `None` for the catch-all "Other" group of
    /// cards the snapshot doesn't place in a drop.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CardResponse>,
}

/// One set sub-type (card treatment) with its cards, as returned by the subtypes
/// endpoint. Same shape as [`DropGroupResponse`] — the enclosing [`Page`] paginates over
/// these (`total` is a sub-type count) and the SPA renders both through one section
/// component — but here `slug` is always present (every card classifies, Normal included).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "SubtypeGroup"))]
pub struct SubtypeGroupResponse {
    /// Stable slug (`normal`/`borderless`/`showcase`/…) for anchors/links.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CardResponse>,
}

/// List sets
///
/// `GET /api/games/{game}/sets` -> every stored set, newest first.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets",
    tag = "Cards",
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "Every stored set for the game, newest first.", body = DataBody<Vec<SetResponse>>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_sets(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<SetResponse>>>, AppError> {
    require_game(&game)?;
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        // Explicit NULLS LAST so a NULL release date sorts last under DESC on Postgres
        // too (SQLite's DESC already parks NULL last, so this is a no-op there).
        .order_by_with_nulls(card_set::Column::ReleasedAt, Order::Desc, NullOrdering::Last)
        .order_by_asc(card_set::Column::Name)
        .all(&state.db)
        .await?;
    // One aggregate scan marks which sets have a special-treatment card, so each tile knows
    // whether to offer the by-sub-type view (the set list is CDN-cached, so this runs ~hourly).
    let with_subtypes = crate::scryfall::subtypes::sets_with_subtypes(&state.db, &game).await?;
    let data: Vec<SetResponse> = sets
        .into_iter()
        .map(|m| {
            let mut set = SetResponse::from(m);
            set.has_subtypes = with_subtypes.contains(&set.code);
            set
        })
        .collect();
    Ok(Json(DataBody { data }))
}

/// Get set
///
/// `GET /api/games/{game}/sets/{code}` -> one set's metadata.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code, e.g. `neo`"),
    ),
    responses(
        (status = 200, description = "The set's metadata.", body = SetResponse),
        (status = 404, description = "Unknown game or set."),
    ),
)]
pub async fn get_set(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
) -> Result<Json<SetResponse>, AppError> {
    require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let has_subtypes = crate::scryfall::subtypes::set_has_subtypes(&state.db, &game, &set.code).await?;
    let mut response = SetResponse::from(set);
    response.has_subtypes = has_subtypes;
    Ok(Json(response))
}

/// `GET /api/games/{game}/sets/{code}/icon` -> the set's SVG icon.
///
/// Cached on disk on first request (like card images) so the provider is only
/// hit once per icon rather than hotlinked on every page view.
pub async fn set_icon(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let source_url = set
        .icon_svg_uri
        .ok_or_else(|| AppError::NotFound(format!("set '{code}' has no icon")))?;

    if !is_allowed_image_url(&source_url) {
        tracing::warn!(set = %set.code, url = %source_url, "refusing to proxy non-allowlisted icon URL");
        return Err(AppError::NotFound("no icon available".to_string()));
    }

    let image = state
        .images
        .get_svg(&game, "set-icons", &set.code, &source_url)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, set = %set.code, "failed to cache set icon");
            AppError::Internal(format!("image cache error: {err}"))
        })?;

    Ok((
        [
            (header::CONTENT_TYPE, image.content_type),
            (header::CACHE_CONTROL, IMAGE_CACHE_CONTROL),
        ],
        image.bytes,
    )
        .into_response())
}

/// List set cards
///
/// `GET /api/games/{game}/sets/{code}/cards` -> a set's cards (optional `q` name
/// search), by collector number.
///
/// With `?include_related=true` the listing spans the set's whole **group** — its
/// top-level root plus every related sub-set (tokens, promos, Commander decks, …)
/// — so a main expansion and its supplements can be browsed as one, instead of
/// visiting each set individually. Cards are then grouped by set (set-code order),
/// each set's cards kept together in collector-number order.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}/cards",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code, e.g. `neo`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter"),
        ("include_related" = Option<bool>, Query, description = "Span the set's whole group (root + related sub-sets)"),
        ("sort" = Option<String>, Query, description = "Sort key (`number`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
    ),
    responses(
        (status = 200, description = "A page of the set's cards.", body = Page<CardResponse>),
        (status = 404, description = "Unknown game or set."),
        (status = 422, description = "Malformed search query or sort."),
    ),
)]
pub async fn list_set_cards(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let (page, page_size) = params.page_and_size();
    let include_related = params.include_related.unwrap_or(false);
    let dialect = state.dialect();

    let mut query = Card::find().filter(card::Column::Game.eq(game.as_str()));
    query = if include_related {
        // Resolve the group membership from the flat set list (one cheap query) via the
        // shared seam the collection include-related view also uses, so both span the
        // same sets.
        let codes = load_group_set_codes(&state, &game, &set.code).await?;
        query.filter(card::Column::SetCode.is_in(codes))
    } else {
        query.filter(card::Column::SetCode.eq(set.code.as_str()))
    };
    let (query, shape) = apply_search(query, game_meta, &params, dialect)?;

    let (sort, dir) = params.sort_spec_with(SortField::Number, shape.order, shape.direction)?;
    // For the default collector-number order, the related-sets view keeps each
    // set's cards contiguous (set code first, which spans whole sets unlike the
    // per-card released_at). Any other sort spans the whole group by the chosen
    // field instead — grouping by set there would fight the sort.
    let group_by_set = include_related && sort == SortField::Number;
    let query = apply_unique(query, shape.unique, dialect);
    let paginator =
        apply_card_sort(query, sort, dir, group_by_set, dialect).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    let data: Vec<CardResponse> = rows.into_iter().map(CardResponse::from).collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// List set drops
///
/// `GET /api/games/{game}/sets/{code}/drops` -> a set's cards grouped by Secret
/// Lair drop (Scryfall's curated drop titles), **paginated by drop**.
///
/// Only sets that have a drop snapshot (`has_drops`) are grouped this way — any
/// other set is a `404` here (browse it via `.../cards` instead). Drops keep
/// Scryfall's display order; within a drop, cards are in collector-number order.
/// Cards whose collector number isn't in the snapshot (e.g. a drop newer than the
/// snapshot) collect into a trailing "Other" group so nothing is dropped. An
/// optional `q` narrows the cards first; drops with no remaining matches are
/// omitted. An optional `drop` then narrows to the drops whose curated title
/// matches (case-insensitive substring), applied before pagination so it spans the
/// whole set rather than one page.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}/drops",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code (must have a drop snapshot, e.g. `sld`)"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by drop)"),
        ("page_size" = Option<u64>, Query, description = "Drops per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search narrowing the cards within each drop"),
        ("drop" = Option<String>, Query, description = "Optional case-insensitive drop-title filter"),
    ),
    responses(
        (status = 200, description = "A page of the set's cards grouped by Secret Lair drop.", body = Page<DropGroupResponse>),
        (status = 404, description = "Unknown game/set, or the set has no drop snapshot."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn list_set_drops(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<DropGroupResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let table = require_drop_table(&game, &set.code)?;
    let dialect = state.dialect();

    // One set's cards are bounded, so we pull the whole (optionally searched) set
    // and group + paginate by drop in memory — that keeps every drop complete
    // regardless of where the page boundary falls.
    let query = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::SetCode.eq(set.code.as_str()));
    let (query, _shape) = apply_search(query, game_meta, &params, dialect)?;
    let rows = apply_card_sort(query, SortField::Number, SortDir::Asc, false, dialect)
        .all(&state.db)
        .await?;

    let mut buckets = group_into_drops(table, rows, |card| card.collector_number.as_str());
    // Narrow to the drops whose title matches the "filter drops by name" box, before
    // paginating so the filter spans every drop, not just the page on screen.
    if let Some(needle) = params.drop_title_filter() {
        buckets = filter_drops_by_title(buckets, needle);
    }

    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(paginate_buckets(buckets, page, page_size, |b| {
        DropGroupResponse {
            slug: b.slug,
            title: b.title,
            card_count: b.cards.len(),
            cards: b.cards.into_iter().map(CardResponse::from).collect(),
        }
    })))
}

/// List set sub-types
///
/// `GET /api/games/{game}/sets/{code}/subtypes` -> a set's cards grouped by sub-type
/// (card treatment: Borderless, Showcase, Extended Art, Full Art, …), **paginated by
/// sub-type**.
///
/// Unlike the by-drop view, every set can be grouped this way — a set with no special
/// treatments is just one "Normal" group (the SPA gates the toggle on `has_subtypes`, so
/// it only offers this where there's something to see). Sub-types keep their fixed order
/// (Normal first, then treatments); within one, cards are in collector-number order. An
/// optional `q` narrows the cards first; sub-types with no remaining matches are omitted.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}/subtypes",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code, e.g. `neo`"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by sub-type)"),
        ("page_size" = Option<u64>, Query, description = "Sub-types per page (clamped)"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search narrowing the cards within each sub-type"),
    ),
    responses(
        (status = 200, description = "A page of the set's cards grouped by sub-type.", body = Page<SubtypeGroupResponse>),
        (status = 404, description = "Unknown game or set."),
        (status = 422, description = "Malformed search query."),
    ),
)]
pub async fn list_set_subtypes(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<SubtypeGroupResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let dialect = state.dialect();

    // One set's cards are bounded, so we pull the whole (optionally searched) set and group
    // + paginate by sub-type in memory — keeping every sub-type complete regardless of where
    // the page boundary falls (matching the by-drop handler).
    let query = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::SetCode.eq(set.code.as_str()));
    let (query, _shape) = apply_search(query, game_meta, &params, dialect)?;
    let rows = apply_card_sort(query, SortField::Number, SortDir::Asc, false, dialect)
        .all(&state.db)
        .await?;

    let buckets = group_into_subtypes(rows, |card| card);
    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(paginate_buckets(buckets, page, page_size, |b| {
        SubtypeGroupResponse {
            slug: b.slug,
            title: b.title,
            card_count: b.cards.len(),
            cards: b.cards.into_iter().map(CardResponse::from).collect(),
        }
    })))
}
