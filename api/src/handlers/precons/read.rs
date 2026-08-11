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
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Select,
    sea_query::{Expr, Func, LikeExpr, NullOrdering},
};

use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, CardSet, PreconDeck, PreconDeckCard, Product};
use crate::entities::{card, card_set, precon_deck, precon_deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{
    CardResponse, DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE,
    MAX_PAGE_SIZE, Page, build_page, product_response, require_game, resolve_page, set_name_map,
    summarize_holdings, trim_query,
};
use crate::scryfall::search::escape_like;
use crate::state::AppState;

use super::{
    PreconCardEntry, PreconDeckDetail, PreconDeckResponse, PreconFaceCard, PreconFacets,
    PreconListParams, PreconSetGroup, PreconSetRef, PreconTypeRef, load_precon, precon_response,
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

    let query = sorted_query(filtered_query(&game, &params), &params);
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

/// The precon list's filters, shared by the flat list and the by-set grouping so the two can
/// never disagree about what a query matches — only about how the matches are laid out.
fn filtered_query(game: &str, params: &PreconListParams) -> Select<PreconDeck> {
    let mut query = PreconDeck::find().filter(precon_deck::Column::Game.eq(game));
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
    query
}

/// The list's order: newest first by default, `sort=name` alphabetical.
///
/// `released_at` is nullable (upstream doesn't date every deck), and a NULL must sort **last**
/// in both dialects rather than first on one of them — hence the explicit null ordering,
/// matching the product list's. `slug` breaks the final tie so a page boundary is stable.
fn sorted_query(query: Select<PreconDeck>, params: &PreconListParams) -> Select<PreconDeck> {
    match trim_query(params.sort.as_deref()) {
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
    }
}

/// List preconstructed decks grouped by set
///
/// `GET /api/games/{game}/precons/sets` -> the same decks the flat list returns, bucketed into
/// the sets that published them and **paginated by set** (so a set is never split across a page
/// boundary), newest set first. The by-set mirror of the card catalog's by-drop view.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/sets",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number (pages are sets, not decks)"),
        ("page_size" = Option<u64>, Query, description = "Sets per page (default 20, max 100)"),
        ("q" = Option<String>, Query, description = "Name substring; every word must match"),
        ("set" = Option<String>, Query, description = "Set code, e.g. `tmc` — narrows to that one group"),
        ("type" = Option<String>, Query, description = "Deck type, e.g. `Commander Deck`"),
        ("sort" = Option<String>, Query, description = "`released` (default, newest first) or `name`"),
    ),
    responses(
        (status = 200, description = "A page of sets, each with its preconstructed decks.", body = Page<PreconSetGroup>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_precon_sets(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<PreconListParams>,
) -> Result<Json<Page<PreconSetGroup>>, AppError> {
    require_game(&game)?;
    // Pages are *sets* here, each holding a handful of decks, so this uses the by-drop
    // endpoints' smaller page bounds rather than the card list's.
    let (page, page_size) = resolve_page(
        params.page,
        params.page_size,
        DEFAULT_DROP_PAGE_SIZE,
        MAX_DROP_PAGE_SIZE,
    );

    // A game's precons are bounded (~3 000 header rows for MTG, no cards joined), so the whole
    // filtered set is fetched and bucketed in memory — the same trade the by-drop endpoint
    // makes with a set's cards, and what keeps every group complete regardless of where the
    // page boundary falls. Only the groups actually on the page are then shaped into DTOs.
    let rows = sorted_query(filtered_query(&game, &params), &params)
        .all(&state.db)
        .await?;

    let names = set_name_map(&state, &game).await?;
    let set_dates = set_release_map(&state, &game).await?;
    let mut buckets = group_by_set(rows);
    sort_buckets(&mut buckets, &set_dates, &names, &params);

    let total = buckets.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let on_page: Vec<(String, Vec<precon_deck::Model>)> = buckets
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();

    // One face-card lookup for the whole page, not one per group.
    let page_rows: Vec<precon_deck::Model> = on_page
        .iter()
        .flat_map(|(_, decks)| decks.iter().cloned())
        .collect();
    let faces = face_cards(&state, &page_rows).await?;

    let data: Vec<PreconSetGroup> = on_page
        .into_iter()
        .map(|(code, decks)| PreconSetGroup {
            deck_count: decks.len(),
            decks: decks
                .iter()
                .map(|row| {
                    precon_response(
                        row,
                        names.get(&row.set_code).cloned(),
                        row.face_card_id.and_then(|id| faces.get(&id).cloned()),
                    )
                })
                .collect(),
            // The set's own release date when the catalog knows it, else the newest deck in
            // the group — an undated group still sorts and labels sensibly.
            released_at: set_dates.get(&code).cloned().flatten().or_else(|| {
                decks
                    .iter()
                    .filter_map(|deck| deck.released_at.clone())
                    .max()
            }),
            name: names.get(&code).cloned(),
            code,
        })
        .collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// Bucket precon rows by set code, preserving the order the rows arrived in **both** for the
/// groups (first row of a set names its position) and within each group. Every group therefore
/// leads with whatever the list's own sort put first.
fn group_by_set(rows: Vec<precon_deck::Model>) -> Vec<(String, Vec<precon_deck::Model>)> {
    let mut order: Vec<String> = Vec::new();
    let mut by_set: HashMap<String, Vec<precon_deck::Model>> = HashMap::new();
    for row in rows {
        let code = row.set_code.clone();
        by_set
            .entry(code.clone())
            .or_insert_with(|| {
                order.push(code.clone());
                Vec::new()
            })
            .push(row);
    }
    order
        .into_iter()
        .filter_map(|code| by_set.remove(&code).map(|decks| (code, decks)))
        .collect()
}

/// Order the groups: newest **set** first (`sort=name`: by set name, then code).
///
/// A set's date is the catalog's, not its decks' — a Secret Lair deck released years after the
/// `sld` set still belongs to `sld`, and the group should sit where the set does. A set the
/// catalog doesn't know falls back to its newest deck, then sorts last if it has neither.
fn sort_buckets(
    buckets: &mut [(String, Vec<precon_deck::Model>)],
    set_dates: &HashMap<String, Option<String>>,
    names: &HashMap<String, String>,
    params: &PreconListParams,
) {
    let by_name = trim_query(params.sort.as_deref()) == Some("name");
    let date_of = |code: &String, decks: &Vec<precon_deck::Model>| -> String {
        set_dates
            .get(code)
            .cloned()
            .flatten()
            .or_else(|| decks.iter().filter_map(|d| d.released_at.clone()).max())
            .unwrap_or_default()
    };
    buckets.sort_by(|(a_code, a_decks), (b_code, b_decks)| {
        if by_name {
            let a_name = names.get(a_code).unwrap_or(a_code);
            let b_name = names.get(b_code).unwrap_or(b_code);
            return a_name.cmp(b_name).then_with(|| a_code.cmp(b_code));
        }
        // Newest first; an undated set sinks below every dated one rather than leading.
        date_of(b_code, b_decks)
            .cmp(&date_of(a_code, a_decks))
            .then_with(|| a_code.cmp(b_code))
    });
}

/// Every set's release date for a game, keyed by code — what orders the groups above.
async fn set_release_map(
    state: &AppState,
    game: &str,
) -> Result<HashMap<String, Option<String>>, AppError> {
    Ok(CardSet::find()
        .select_only()
        .column(card_set::Column::Code)
        .column(card_set::Column::ReleasedAt)
        .filter(card_set::Column::Game.eq(game))
        .into_tuple::<(String, Option<String>)>()
        .all(&state.db)
        .await?
        .into_iter()
        .collect())
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
