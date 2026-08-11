//! Public reads for the precon browser: the paginated list, its filter facets, and one
//! deck's full contents.
//!
//! All three are anonymous catalog reads (the router's `public` group), so they must stay
//! cheap enough to serve uncached: the list's tile facets — colours, counts, the face card —
//! were folded once at ingest into columns, and the only per-page work here is one join for
//! the face cards and one lookup of the set names.

use std::collections::HashMap;

use axum::{Json, extract::State};
use sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    sea_query::{Expr, Func, LikeExpr, NullOrdering},
};

use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, CardSet, PreconDeck, PreconDeckCard, Product};
use crate::entities::{card, card_set, precon_deck, precon_deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{
    CardResponse, DEFAULT_PAGE_SIZE, DataBody, MAX_PAGE_SIZE, Page, build_page, product_response,
    require_game, resolve_page, set_name_map, summarize_holdings, trim_query,
};
use crate::scryfall::search::escape_like;
use crate::state::AppState;

use super::{
    PreconCardEntry, PreconDeckDetail, PreconDeckResponse, PreconFaceCard, PreconFacets,
    PreconListParams, PreconSetRef, PreconTypeRef, load_precon, precon_response,
};

/// List preconstructed decks
///
/// `GET /api/games/{game}/precons` -> the published decklists that shipped with the game's
/// sets, newest first: Commander decks, Planeswalker / Challenger / Starter decks, Jumpstart
/// themes, Secret Lair drops. Filter by `set`, by `type` (see the facets endpoint for the
/// vocabulary), or by a name substring `q`; `sort=name` orders alphabetically instead.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (default 60, max 200)"),
        ("q" = Option<String>, Query, description = "Name substring; every word must match"),
        ("set" = Option<String>, Query, description = "Set code, e.g. `tmc`"),
        ("type" = Option<String>, Query, description = "Deck type, e.g. `Commander Deck`"),
        ("sort" = Option<String>, Query, description = "`released` (default, newest first) or `name`"),
    ),
    responses(
        (status = 200, description = "A page of preconstructed decks.", body = Page<PreconDeckResponse>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_precons(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<PreconListParams>,
) -> Result<Json<Page<PreconDeckResponse>>, AppError> {
    require_game(&game)?;
    let (page, page_size) = resolve_page(
        params.page,
        params.page_size,
        DEFAULT_PAGE_SIZE,
        MAX_PAGE_SIZE,
    );

    let mut query = PreconDeck::find().filter(precon_deck::Column::Game.eq(game.as_str()));
    if let Some(term) = trim_query(params.q.as_deref()) {
        // Every whitespace-separated word as its own order-independent name substring,
        // AND-ed — the sealed product list's rule (issue #273), so a precon and a sealed
        // product answer "commander tarkir" the same way. LOWER both sides so the match is
        // case-insensitive on Postgres too.
        for word in term.split_whitespace() {
            let pattern = format!("%{}%", escape_like(word).to_ascii_lowercase());
            query = query.filter(
                Expr::expr(Func::lower(Expr::col((
                    precon_deck::Entity,
                    precon_deck::Column::Name,
                ))))
                .like(LikeExpr::new(pattern).escape('\\')),
            );
        }
    }
    if let Some(set) = trim_query(params.set.as_deref()) {
        query = query.filter(precon_deck::Column::SetCode.eq(set.to_lowercase()));
    }
    if let Some(deck_type) = trim_query(params.deck_type.as_deref()) {
        query = query.filter(precon_deck::Column::DeckType.eq(deck_type));
    }

    // Newest first by default. `released_at` is nullable (upstream doesn't date every deck),
    // and a NULL must sort *last* in both dialects rather than first on one of them — hence
    // the explicit null ordering, matching the product list's.
    let query = match trim_query(params.sort.as_deref()) {
        Some("name") => query
            .order_by_asc(precon_deck::Column::Name)
            .order_by_asc(precon_deck::Column::Slug),
        _ => query
            .order_by_with_nulls(
                precon_deck::Column::ReleasedAt,
                sea_orm::Order::Desc,
                NullOrdering::Last,
            )
            .order_by_asc(precon_deck::Column::Name)
            .order_by_asc(precon_deck::Column::Slug),
    };

    let paginator = query.paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    let names = set_name_map(&state, &game).await?;
    let faces = face_cards(&state, &rows).await?;
    let data: Vec<PreconDeckResponse> = rows
        .iter()
        .map(|row| {
            precon_response(
                row,
                names.get(&row.set_code).cloned(),
                row.face_card_id.and_then(|id| faces.get(&id).cloned()),
            )
        })
        .collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// Precon filter facets
///
/// `GET /api/games/{game}/precons/facets` -> the deck types and sets that actually have
/// preconstructed decks, with counts, so the browse filters need no hardcoded vocabulary
/// (upstream adds categories over time).
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/facets",
    tag = "Preconstructed decks",
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "Deck types + sets that have precons, with counts.", body = DataBody<PreconFacets>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn precon_facets(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<DataBody<PreconFacets>>, AppError> {
    require_game(&game)?;

    let mut types: Vec<PreconTypeRef> = PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::DeckType)
        .column_as(precon_deck::Column::Id.count(), "count")
        .filter(precon_deck::Column::Game.eq(game.as_str()))
        .group_by(precon_deck::Column::DeckType)
        .into_tuple::<(String, i64)>()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|(deck_type, count)| PreconTypeRef { deck_type, count })
        .collect();
    // Biggest categories first, ties alphabetical — a dropdown that opens on "Commander
    // Deck" rather than "Advanced Deck" is the one a player wanted.
    types.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.deck_type.cmp(&b.deck_type))
    });

    let counted: Vec<(String, i64)> = PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::SetCode)
        .column_as(precon_deck::Column::Id.count(), "count")
        .filter(precon_deck::Column::Game.eq(game.as_str()))
        .group_by(precon_deck::Column::SetCode)
        .into_tuple()
        .all(&state.db)
        .await?;
    let total = counted.iter().map(|(_, count)| count).sum();

    // Resolve names + release dates from the catalog's own sets, so the filter reads
    // "Tarkir: Dragonstorm" and orders like every other set list in the app.
    let set_rows: Vec<(String, String, Option<String>)> = CardSet::find()
        .select_only()
        .column(card_set::Column::Code)
        .column(card_set::Column::Name)
        .column(card_set::Column::ReleasedAt)
        .filter(card_set::Column::Game.eq(game.as_str()))
        .into_tuple()
        .all(&state.db)
        .await?;
    let by_code: HashMap<String, (String, Option<String>)> = set_rows
        .into_iter()
        .map(|(code, name, released)| (code, (name, released)))
        .collect();
    let mut sets: Vec<PreconSetRef> = counted
        .into_iter()
        .map(|(code, count)| {
            let known = by_code.get(&code);
            PreconSetRef {
                name: known.map(|(name, _)| name.clone()),
                released_at: known.and_then(|(_, released)| released.clone()),
                code,
                count,
            }
        })
        .collect();
    // Newest set first; an undated set sorts last rather than leading the list.
    sets.sort_by(|a, b| {
        b.released_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.released_at.as_deref().unwrap_or(""))
            .then_with(|| a.code.cmp(&b.code))
    });

    Ok(Json(DataBody {
        data: PreconFacets { types, sets, total },
    }))
}

/// Get a preconstructed deck
///
/// `GET /api/games/{game}/precons/{slug}` -> one published decklist in full: its header, a
/// value summary, every card in board order (command zone, deck, sideboard), and the sealed
/// product it ships in when the catalog holds one.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/{slug}",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
    ),
    responses(
        (status = 200, description = "The deck's header, summary, cards, and sealed product.", body = PreconDeckDetail),
        (status = 404, description = "Unknown game or precon."),
    ),
)]
pub async fn get_precon(
    State(state): State<AppState>,
    Path((game, slug)): Path<(String, String)>,
) -> Result<Json<PreconDeckDetail>, AppError> {
    require_game(&game)?;
    let precon = load_precon(&state, &game, &slug).await?;

    // Board order (command zone, deck, sideboard) is upstream's reading order, and
    // `position` preserves the order within each — which for a Secret Lair drop is the
    // order the drop itself lists its cards in, not alphabetical.
    let rows: Vec<(precon_deck_card::Model, Option<card::Model>)> = PreconDeckCard::find()
        .find_also_related(Card)
        .filter(precon_deck_card::Column::PreconDeckId.eq(precon.id))
        .order_by_asc(precon_deck_card::Column::Board)
        .order_by_asc(precon_deck_card::Column::Position)
        .order_by_asc(precon_deck_card::Column::Id)
        .all(&state.db)
        .await?;

    // Value aggregates reuse the shared holdings fold, split on the sideboard line the way
    // a deck's detail splits on its maybeboard: `summary` is the deck proper (what
    // `card_count` counts), `sideboard_summary` is what sits beside it.
    let side = PreconBoard::Side.as_str();
    let (side_rows, deck_rows): (Vec<_>, Vec<_>) = rows.iter().partition(|(c, _)| c.board == side);
    let bulk_threshold = resolve_bulk_threshold_cents(None);
    let summary = summarize_holdings(&deck_rows, bulk_threshold);
    let sideboard_summary = summarize_holdings(&side_rows, bulk_threshold);

    // Board order for the wire: the alphabetical `ORDER BY board` above puts `commander`
    // before `main` before `side` by luck of the alphabet, so state it rather than rely on
    // it — renaming a board must not silently reorder the page.
    let board_rank = |board: &str| match board {
        b if b == PreconBoard::Commander.as_str() => 0,
        b if b == PreconBoard::Main.as_str() => 1,
        _ => 2,
    };
    let mut entries: Vec<(u8, i32, i32, PreconCardEntry)> = rows
        .into_iter()
        .filter_map(|(item, card)| {
            let card = card?;
            Some((
                board_rank(&item.board),
                item.position,
                item.id,
                PreconCardEntry {
                    card: CardResponse::from(card),
                    board: item.board,
                    quantity: item.quantity,
                    foil: item.foil,
                },
            ))
        })
        .collect();
    entries.sort_by_key(|(rank, position, id, _)| (*rank, *position, *id));
    let cards: Vec<PreconCardEntry> = entries.into_iter().map(|(_, _, _, entry)| entry).collect();

    // The sealed product it ships in — a link the SPA turns into a price + a buy link. Gone
    // (a re-import dropped it) simply reads as absent.
    let product = match precon.product_id {
        Some(id) => Product::find_by_id(id).one(&state.db).await?,
        None => None,
    };
    let names = set_name_map(&state, &game).await?;
    let face_card = match precon.face_card_id {
        Some(id) => face_cards(&state, std::slice::from_ref(&precon))
            .await?
            .remove(&id),
        None => None,
    };

    Ok(Json(PreconDeckDetail {
        deck: precon_response(&precon, names.get(&precon.set_code).cloned(), face_card),
        summary,
        sideboard_summary,
        cards,
        product: product.map(|p| product_response(p, &names)),
    }))
}

/// Resolve a page of precons' face cards in one query, keyed by internal card id.
///
/// A face card whose catalog row is gone (a re-import) is simply absent, and the tile falls
/// back to its set icon — the same LEFT-join-then-skip tolerance every other card link in
/// the app has.
async fn face_cards(
    state: &AppState,
    rows: &[precon_deck::Model],
) -> Result<HashMap<i32, PreconFaceCard>, AppError> {
    let ids: Vec<i32> = rows
        .iter()
        .filter_map(|row| row.face_card_id)
        .collect::<std::collections::HashSet<i32>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let cards = Card::find()
        .filter(card::Column::Id.is_in(ids))
        .all(&state.db)
        .await?;
    Ok(cards
        .into_iter()
        .map(|card| {
            let id = card.id;
            // `has_image` is whatever the shared card payload would say, so a tile and a card
            // page never disagree about whether an image exists.
            let response = CardResponse::from(card);
            (
                id,
                PreconFaceCard {
                    card_id: response.id,
                    name: response.name,
                    has_image: response.has_image,
                },
            )
        })
        .collect())
}
