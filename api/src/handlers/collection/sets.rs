//! Collection per-set endpoints: the owned-set landing tiles and the owned cards of a
//! drop-grouped set, grouped by Secret Lair drop.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::CardSet;
use crate::entities::{card, card_set, collection_item};
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, Page, SortDir, SortField, Valuation, group_into_drops, load_set, paginate_buckets,
    require_drop_table, require_game, search_condition,
};
use crate::state::AppState;

use super::read::{collection_query, owned_with_cards};
use super::{
    CollectionDropGroup, CollectionEntry, CollectionSet, CollectionSetsResponse, CollectionSort,
    ListParams,
};

/// `GET /api/collection/{game}/sets` -> the sets the signed-in user owns cards in,
/// newest set first, each with the catalog set metadata plus owned counts. Backs the
/// collection's per-set landing (mirrors the catalog's game -> sets view).
pub async fn collection_sets(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
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
        data: build_collection_sets(&game, rows, sets),
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

    // Parse the optional Scryfall-syntax query up front so a malformed one 422s before
    // we touch the DB (mirrors the list handler).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s))
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

/// Per-set running totals while aggregating a user's holdings into set tiles.
#[derive(Default)]
struct SetAgg {
    /// The card's own `set_name`, used only if `card_sets` has no row for the set.
    fallback_name: String,
    /// Distinct owned cards (one per holding row).
    owned_cards: i64,
    /// Total owned copies (regular + foil).
    owned_copies: i64,
    /// Estimated USD value of the set's owned cards (regular at `usd`, foil at
    /// `usd_foil`); its `any_priced` flag reports `null` for an all-unpriced set
    /// rather than `$0.00`, matching the summary.
    valuation: Valuation,
}

/// Aggregate owned holdings into per-set tiles: count distinct owned cards + total
/// copies + estimated value per `set_code`, dress each with the game's set metadata
/// (falling back to the card's own `set_name` when the set row is missing), and order
/// newest set first (undated last), tie-broken by code for deterministic output. Pure so
/// it can be unit-tested without a DB. Holdings whose card row is gone are skipped.
pub(super) fn build_collection_sets(
    game: &str,
    rows: Vec<(collection_item::Model, Option<card::Model>)>,
    sets: Vec<card_set::Model>,
) -> Vec<CollectionSet> {
    let mut agg: HashMap<String, SetAgg> = HashMap::new();
    for (item, card) in rows {
        let Some(card) = card else { continue };
        // Read the card's prices before its set_code/set_name move into the map entry,
        // so the borrow is clean regardless of aggregation order.
        let usd = card.price_usd.as_deref();
        let usd_foil = card.price_usd_foil.as_deref();
        let entry = agg.entry(card.set_code).or_insert_with(|| SetAgg {
            fallback_name: card.set_name,
            ..SetAgg::default()
        });
        entry.owned_cards += 1;
        entry.owned_copies += i64::from(item.quantity) + i64::from(item.foil_quantity);
        entry
            .valuation
            .add(usd, item.quantity, usd_foil, item.foil_quantity);
    }

    let meta: HashMap<String, card_set::Model> =
        sets.into_iter().map(|s| (s.code.clone(), s)).collect();

    let mut out: Vec<CollectionSet> = agg
        .into_iter()
        .map(|(code, agg)| {
            let SetAgg {
                fallback_name,
                owned_cards,
                owned_copies,
                valuation,
            } = agg;
            // Dress the tile with the game's set metadata; a set present in a holding but
            // absent from card_sets (e.g. metadata not yet synced) degrades to a bare tile
            // using the card's own set name. The owned stats are identical either way, so
            // both cases build one `CollectionSet` (no duplicated arm).
            let m = meta.get(&code);
            CollectionSet {
                name: m.map_or(fallback_name, |m| m.name.clone()),
                set_type: m.and_then(|m| m.set_type.clone()),
                released_at: m.and_then(|m| m.released_at.clone()),
                card_count: m.map_or(0, |m| m.card_count),
                icon_svg_uri: m.and_then(|m| m.icon_svg_uri.clone()),
                parent_set_code: m.and_then(|m| m.parent_set_code.clone()),
                has_drops: crate::scryfall::drops::has_drops(game, &code),
                owned_cards,
                owned_copies,
                owned_value_usd: valuation.total_usd(),
                code,
            }
        })
        .collect();

    // Newest release first; `None` (undated) sorts last since `None < Some`. Ties by
    // code for a stable, deterministic order.
    out.sort_by(|a, b| {
        b.released_at
            .cmp(&a.released_at)
            .then_with(|| a.code.cmp(&b.code))
    });
    out
}
