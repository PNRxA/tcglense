//! Catalog set endpoints: the set list, one set's metadata, its SVG icon proxy, and a
//! set's cards (flat, include-related, or grouped by Secret Lair drop).

use axum::{
    Json,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::Serialize;

use crate::entities::prelude::{Card, CardSet};
use crate::entities::{card, card_set};
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, DataBody, Page, SortDir, SortField, apply_card_sort, build_page, group_into_drops,
    load_group_set_codes, load_set, paginate_buckets, require_drop_table, require_game,
};
use crate::state::AppState;

use super::image::is_allowed_image_url;
use super::{IMAGE_CACHE_CONTROL, ListParams, apply_search};

#[derive(Debug, Serialize)]
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
        }
    }
}

/// One Secret Lair drop with its cards, as returned by the drops endpoint. The
/// enclosing [`Page`] paginates over these (so `total` is a drop count, not a
/// card count).
#[derive(Debug, Serialize)]
pub struct DropGroupResponse {
    /// Stable slug for anchors/links; `None` for the catch-all "Other" group of
    /// cards the snapshot doesn't place in a drop.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CardResponse>,
}

/// `GET /api/games/{game}/sets` -> every stored set, newest first.
pub async fn list_sets(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<SetResponse>>>, AppError> {
    require_game(&game)?;
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        .order_by_desc(card_set::Column::ReleasedAt)
        .order_by_asc(card_set::Column::Name)
        .all(&state.db)
        .await?;
    let data: Vec<SetResponse> = sets.into_iter().map(SetResponse::from).collect();
    Ok(Json(DataBody { data }))
}

/// `GET /api/games/{game}/sets/{code}` -> one set's metadata.
pub async fn get_set(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
) -> Result<Json<SetResponse>, AppError> {
    require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    Ok(Json(SetResponse::from(set)))
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

/// `GET /api/games/{game}/sets/{code}/cards` -> a set's cards (optional `q` name
/// search), by collector number.
///
/// With `?include_related=true` the listing spans the set's whole **group** — its
/// top-level root plus every related sub-set (tokens, promos, Commander decks, …)
/// — so a main expansion and its supplements can be browsed as one, instead of
/// visiting each set individually. Cards are then grouped by set (set-code order),
/// each set's cards kept together in collector-number order.
pub async fn list_set_cards(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let (page, page_size) = params.page_and_size();
    let include_related = params.include_related.unwrap_or(false);

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
    query = apply_search(query, game_meta, &params)?;

    let (sort, dir) = params.sort_spec(SortField::Number)?;
    // For the default collector-number order, the related-sets view keeps each
    // set's cards contiguous (set code first, which spans whole sets unlike the
    // per-card released_at). Any other sort spans the whole group by the chosen
    // field instead — grouping by set there would fight the sort.
    let group_by_set = include_related && sort == SortField::Number;
    let paginator = apply_card_sort(query, sort, dir, group_by_set).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    let data: Vec<CardResponse> = rows.into_iter().map(CardResponse::from).collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/games/{game}/sets/{code}/drops` -> a set's cards grouped by Secret
/// Lair drop (Scryfall's curated drop titles), **paginated by drop**.
///
/// Only sets that have a drop snapshot (`has_drops`) are grouped this way — any
/// other set is a `404` here (browse it via `.../cards` instead). Drops keep
/// Scryfall's display order; within a drop, cards are in collector-number order.
/// Cards whose collector number isn't in the snapshot (e.g. a drop newer than the
/// snapshot) collect into a trailing "Other" group so nothing is dropped. An
/// optional `q` narrows the cards first; drops with no remaining matches are
/// omitted.
pub async fn list_set_drops(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<DropGroupResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let table = require_drop_table(&game, &set.code)?;

    // One set's cards are bounded, so we pull the whole (optionally searched) set
    // and group + paginate by drop in memory — that keeps every drop complete
    // regardless of where the page boundary falls.
    let mut query = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::SetCode.eq(set.code.as_str()));
    query = apply_search(query, game_meta, &params)?;
    let rows = apply_card_sort(query, SortField::Number, SortDir::Asc, false)
        .all(&state.db)
        .await?;

    let buckets = group_into_drops(table, rows, |card| card.collector_number.as_str());

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
