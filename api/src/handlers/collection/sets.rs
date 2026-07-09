//! Collection per-set endpoints: the owned-set landing tiles and the owned cards of a
//! drop-grouped set, grouped by Secret Lair drop.

use axum::{
    Json,
    extract::State,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::CardSet;
use crate::entities::{card, card_set, collection_item};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    CardResponse, Page, SetsParams, SortDir, SortField, build_collection_sets, group_into_drops,
    group_into_subtypes, load_set, paginate_buckets, require_drop_table, require_game,
    search_condition,
};
use crate::state::AppState;

use super::read::{collection_query, owned_with_cards};
use super::{
    CollectionDropGroup, CollectionEntry, CollectionSetsResponse, CollectionSort,
    CollectionSubtypeGroup, ListParams,
};

/// `GET /api/collection/{game}/sets` -> the sets the signed-in user owns cards in,
/// newest set first, each with the catalog set metadata plus owned counts. Backs the
/// collection's per-set landing (mirrors the catalog's game -> sets view).
pub async fn collection_sets(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<SetsParams>,
) -> Result<Json<CollectionSetsResponse>, AppError> {
    require_game(&game)?;

    // Every owned card (with its joined card row) for the game — bounded by how many
    // distinct cards the user owns.
    let rows = owned_with_cards(user.id, &game, None).all(&state.db).await?;

    // The game's set metadata, to dress each owned set as a full catalog tile.
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;

    Ok(Json(CollectionSetsResponse {
        data: build_collection_sets(&game, rows, sets, params.bulk_threshold_cents()),
    }))
}

/// `GET /api/collection/{game}/sets/{code}/drops` -> the signed-in user's owned cards
/// in a drop-grouped set (e.g. Secret Lair), grouped by Secret Lair drop and
/// **paginated by drop** — the collection mirror of the catalog's set-drops endpoint,
/// but scoped to (and carrying the owned counts of) what the user owns.
///
/// Only owned cards appear, so a drop the user owns nothing in is simply absent; cards
/// whose collector number isn't in the snapshot fall into a trailing "Other" group.
/// `404` if the set isn't drop-grouped (check `has_drops` first). An optional `q`
/// narrows the owned cards, dropping now-empty drops.
pub async fn collection_set_drops(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionDropGroup>>, AppError> {
    let game_meta = require_game(&game)?;
    // Canonicalise the set (and 404 an unknown one) exactly as the catalog does.
    let set = load_set(&state, &game, &code).await?;
    let table = require_drop_table(&game, &set.code)?;
    let dialect = state.dialect();

    // Parse the optional Scryfall-syntax query up front so a malformed one 422s before
    // we touch the DB (mirrors the list handler).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s, dialect))
        .transpose()?;

    // The user's owned cards in this set, in collector-number order (with their
    // holdings) — bounded by one set, so we group + paginate by drop in memory, keeping
    // every drop complete regardless of where the page boundary falls.
    let scope = [set.code.clone()];
    let rows = collection_query(
        user.id,
        &game,
        Some(&scope),
        search,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
        dialect,
    )
    .all(&state.db)
    .await?;

    // A holding whose card row is gone (a catalog re-import) left-joins to `None` — skip
    // it, exactly as the list/summary reads do.
    let pairs: Vec<(collection_item::Model, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();

    let buckets = group_into_drops(table, pairs, |(_, card)| card.collector_number.as_str());

    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(paginate_buckets(buckets, page, page_size, |bucket| {
        CollectionDropGroup {
            slug: bucket.slug,
            title: bucket.title,
            card_count: bucket.cards.len(),
            cards: bucket
                .cards
                .into_iter()
                .map(|(item, card)| CollectionEntry {
                    card: CardResponse::from(card),
                    quantity: item.quantity,
                    foil_quantity: item.foil_quantity,
                })
                .collect(),
        }
    })))
}

/// `GET /api/collection/{game}/sets/{code}/subtypes` -> the signed-in user's owned cards
/// in a set, grouped by card sub-type (treatment) and **paginated by sub-type** — the
/// collection mirror of the catalog's set-subtypes endpoint, scoped to (and carrying the
/// owned counts of) what the user owns.
///
/// Only owned cards appear, so a sub-type the user owns nothing in is simply absent. Any
/// set works (no drop-table gate); the SPA gates the toggle on the tile's `has_subtypes`.
/// An optional `q` narrows the owned cards, dropping now-empty sub-types.
pub async fn collection_set_subtypes(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionSubtypeGroup>>, AppError> {
    let game_meta = require_game(&game)?;
    // Canonicalise the set (and 404 an unknown one) exactly as the catalog does.
    let set = load_set(&state, &game, &code).await?;
    let dialect = state.dialect();

    // Parse the optional Scryfall-syntax query up front so a malformed one 422s before we
    // touch the DB (mirrors the by-drop handler).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s, dialect))
        .transpose()?;

    // The user's owned cards in this set, in collector-number order (with their holdings)
    // — bounded by one set, so we group + paginate by sub-type in memory.
    let scope = [set.code.clone()];
    let rows = collection_query(
        user.id,
        &game,
        Some(&scope),
        search,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
        dialect,
    )
    .all(&state.db)
    .await?;

    // A holding whose card row is gone (a catalog re-import) left-joins to `None` — skip it.
    let pairs: Vec<(collection_item::Model, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();

    let buckets = group_into_subtypes(pairs, |(_, card)| card);

    let (page, page_size) = params.drop_page_and_size();
    Ok(Json(paginate_buckets(buckets, page, page_size, |bucket| {
        CollectionSubtypeGroup {
            slug: bucket.slug,
            title: bucket.title,
            card_count: bucket.cards.len(),
            cards: bucket
                .cards
                .into_iter()
                .map(|(item, card)| CollectionEntry {
                    card: CardResponse::from(card),
                    quantity: item.quantity,
                    foil_quantity: item.foil_quantity,
                })
                .collect(),
        }
    })))
}
