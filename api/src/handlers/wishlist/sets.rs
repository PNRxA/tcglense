//! Wish-list per-set endpoints: the wanted-set landing tiles and the wanted cards of a
//! drop-grouped set, grouped by Secret Lair drop.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::CardSet;
use crate::entities::{card, card_set, wishlist_item};
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, CollectionDropGroup, CollectionEntry, CollectionSetsResponse, CollectionSort,
    ListParams, Page, SortDir, SortField, build_collection_sets, group_into_drops, load_set,
    paginate_buckets, require_drop_table, require_game, search_condition,
};
use crate::state::AppState;

use super::read::{wanted_with_cards, wishlist_query};

/// `GET /api/wishlist/{game}/sets` -> the sets the signed-in user wants cards in,
/// newest set first, each with the catalog set metadata plus wanted counts. Backs the
/// wish list's per-set landing (its `owned_*` fields read as wanted counts there).
pub async fn wishlist_sets(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionSetsResponse>, AppError> {
    require_game(&game)?;

    // Every wanted card (with its joined card row) for the game — bounded by how many
    // distinct cards the user wants.
    let rows = wanted_with_cards(user.id, &game, None).all(&state.db).await?;

    // The game's set metadata, to dress each wanted set as a full catalog tile.
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;

    Ok(Json(CollectionSetsResponse {
        data: build_collection_sets(&game, rows, sets),
    }))
}

/// `GET /api/wishlist/{game}/sets/{code}/drops` -> the signed-in user's wanted cards
/// in a drop-grouped set (e.g. Secret Lair), grouped by Secret Lair drop and
/// **paginated by drop** — the wish-list mirror of the catalog's set-drops endpoint,
/// but scoped to (and carrying the wanted counts of) what the user wants.
///
/// Only wanted cards appear, so a drop the user wants nothing in is simply absent;
/// cards whose collector number isn't in the snapshot fall into a trailing "Other"
/// group. `404` if the set isn't drop-grouped (check `has_drops` first). An optional
/// `q` narrows the wanted cards, dropping now-empty drops.
pub async fn wishlist_set_drops(
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

    // The user's wanted cards in this set, in collector-number order (with their
    // wish-list rows) — bounded by one set, so we group + paginate by drop in memory,
    // keeping every drop complete regardless of where the page boundary falls.
    let scope = [set.code.clone()];
    let rows = wishlist_query(
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

    // A row whose card is gone (a catalog re-import) left-joins to `None` — skip it,
    // exactly as the list/summary reads do.
    let pairs: Vec<(wishlist_item::Model, card::Model)> = rows
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
