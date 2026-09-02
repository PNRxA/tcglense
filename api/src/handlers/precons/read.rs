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
    sea_query::NullOrdering,
};

use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, CardSet, PreconDeck, PreconDeckCard, Product};
use crate::entities::{card, card_set, precon_deck, precon_deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{
    CardResponse, DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE,
    MAX_PAGE_SIZE, Page, SearchGroup, build_page, every_word_matches, identity_printing_ids,
    load_card, load_group_set_codes, product_response, require_game, resolve_page, set_name_map,
    starts_with_rank, summarize_holdings, trim_query,
};
use crate::state::AppState;

use super::{
    CardPreconRef, CardPreconsParams, PreconCardEntry, PreconDeckDetail, PreconDeckResponse,
    PreconFaceCard, PreconFacets, PreconGroup, PreconGrouping, PreconListParams, PreconSetRef,
    PreconTypeRef, load_precon, precon_response,
};

/// List preconstructed decks
///
/// `GET /api/games/{game}/precons` -> the published decklists that shipped with the game's
/// sets, newest first: Commander decks, Planeswalker / Challenger / Starter decks, Jumpstart
/// themes, intro packs. Filter by `set`, by `type` (see the facets endpoint for the
/// vocabulary), or by a name substring `q`; `sort=name` orders alphabetically instead, and
/// `sort=price` most valuable first (unpriced decks last).
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
        ("include_related" = Option<bool>, Query, description = "With `set`, span its whole group (root + related sub-sets)"),
        ("type" = Option<String>, Query, description = "Deck type, e.g. `Commander Deck`"),
        ("sort" = Option<String>, Query, description = "`released` (default, newest first), `name`, or `price` (most valuable first; unpriced last)"),
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

    let scope = set_scope(&state, &game, &params).await?;
    let query = sorted_query(filtered_query(&game, &params, scope.as_deref())?, &params);
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

/// The universal search's precon leg (`GET /api/games/{game}/search`, see
/// [`crate::handlers::search`]): up to `limit` precons whose name contains every word of
/// `term`, prefix matches first and then by name, plus whether more matched.
///
/// Built from [`filtered_query`] — the one builder the flat and grouped listings share —
/// with only its `q` set, so the search can't answer a name the browse wouldn't. Dressed
/// exactly as a browse tile is (set name, face card), with the caller lending the set-name
/// map it already loaded for the sealed leg. One row of over-fetch answers `has_more`.
pub(crate) async fn search_precons(
    state: &AppState,
    game: &str,
    term: &str,
    limit: usize,
    set_names: &HashMap<String, String>,
) -> Result<SearchGroup<PreconDeckResponse>, AppError> {
    let params = PreconListParams {
        q: Some(term.to_string()),
        ..PreconListParams::default()
    };
    let rows = filtered_query(game, &params, None)?
        .order_by_asc(starts_with_rank(
            (precon_deck::Entity, precon_deck::Column::Name),
            term,
        ))
        .order_by_asc(precon_deck::Column::Name)
        .order_by_asc(precon_deck::Column::Slug)
        .limit(limit as u64 + 1)
        .all(&state.db)
        .await?;
    let faces = face_cards(state, &rows).await?;
    let data: Vec<PreconDeckResponse> = rows
        .iter()
        .map(|row| {
            precon_response(
                row,
                set_names.get(&row.set_code).cloned(),
                row.face_card_id.and_then(|id| faces.get(&id).cloned()),
            )
        })
        .collect();
    Ok(SearchGroup::from_overfetch(data, limit))
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
        // The same mapping the copy uses, so the page and the copy are judged alike.
        format: super::copy::precon_format(&precon.deck_type),
        deck: precon_response(&precon, names.get(&precon.set_code).cloned(), face_card),
        summary,
        sideboard_summary,
        cards,
        product: product.map(|p| product_response(p, &names)),
    }))
}

/// The set codes a listing runs over: just the `set` filter, or — with `include_related` —
/// its whole catalog group, resolved through the same [`load_group_set_codes`] seam the card
/// listing's own related view uses, so "All decks" on the precon landing spans exactly the
/// sets "View all" spans there. `None` = no set filter (the unscoped browse).
///
/// The code is a *filter*, not a route: an unknown one resolves to itself and yields an empty
/// page rather than a 404 (`group_set_codes`'s own fallback), matching how `set` already behaves.
async fn set_scope(
    state: &AppState,
    game: &str,
    params: &PreconListParams,
) -> Result<Option<Vec<String>>, AppError> {
    let Some(set) = trim_query(params.set.as_deref()) else {
        return Ok(None);
    };
    let code = set.to_lowercase();
    if params.include_related.unwrap_or(false) {
        Ok(Some(load_group_set_codes(state, game, &code).await?))
    } else {
        Ok(Some(vec![code]))
    }
}

/// The precon list's filters, shared by the flat list and the by-set grouping so the two can
/// never disagree about what a query matches — only about how the matches are laid out.
///
/// The set scope arrives pre-resolved (see [`set_scope`]) because resolving a group needs the
/// DB, and this stays a pure query builder.
fn filtered_query(
    game: &str,
    params: &PreconListParams,
    scope: Option<&[String]>,
) -> Result<Select<PreconDeck>, AppError> {
    let mut query = PreconDeck::find().filter(precon_deck::Column::Game.eq(game));
    if let Some(term) = trim_query(params.q.as_deref()) {
        // Every whitespace-separated word as its own order-independent name substring,
        // AND-ed — the sealed product list's rule (issue #273), so a precon and a sealed
        // product answer "commander tarkir" the same way, through the one shared builder
        // (which is also what keeps a long `?q` from overflowing the SQL builder's stack).
        query = query.filter(every_word_matches(
            (precon_deck::Entity, precon_deck::Column::Name),
            term,
        )?);
    }
    if let Some(codes) = scope {
        query = query.filter(precon_deck::Column::SetCode.is_in(codes.iter().cloned()));
    }
    if let Some(deck_type) = trim_query(params.deck_type.as_deref()) {
        query = query.filter(precon_deck::Column::DeckType.eq(deck_type));
    }
    Ok(query)
}

/// The list's order: newest first by default, `sort=name` alphabetical, `sort=price` most
/// valuable first.
///
/// `released_at` is nullable (upstream doesn't date every deck), and a NULL must sort **last**
/// in both dialects rather than first on one of them — hence the explicit null ordering,
/// matching the product list's. The same rule holds for `price_cents` (`NULL` = "none of its
/// cards are priced", which must sink below every valued deck, never lead the descending
/// order the way a bare `ORDER BY` would put it on Postgres). Being our own derived integer
/// column, the price needs none of the dialect-guarded `CAST` machinery the product list's
/// string prices sort through. `slug` breaks the final tie so a page boundary is stable.
fn sorted_query(query: Select<PreconDeck>, params: &PreconListParams) -> Select<PreconDeck> {
    match trim_query(params.sort.as_deref()) {
        Some("name") => query
            .order_by_asc(precon_deck::Column::Name)
            .order_by_asc(precon_deck::Column::Slug),
        Some("price") => query
            .order_by_with_nulls(
                precon_deck::Column::PriceCents,
                sea_orm::Order::Desc,
                NullOrdering::Last,
            )
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

/// List preconstructed decks grouped
///
/// `GET /api/games/{game}/precons/groups` -> the same decks the flat list returns, bucketed and
/// **paginated by group** (so a group is never split across a page boundary). `group=set` (the
/// default) buckets by the set that published them, newest set first; `group=type` buckets by
/// upstream's deck category — "Commander Deck", "Jumpstart", "Secret Lair Drop" — biggest first,
/// which is what makes a 70-deck set readable instead of a wall of mixed tiles. The by-set /
/// by-type mirror of the card catalog's by-drop view.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/groups",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("group" = Option<String>, Query, description = "`set` (default) or `type`"),
        ("page" = Option<u64>, Query, description = "1-based page number (pages are groups, not decks)"),
        ("page_size" = Option<u64>, Query, description = "Groups per page (default 20, max 100)"),
        ("q" = Option<String>, Query, description = "Name substring; every word must match"),
        ("set" = Option<String>, Query, description = "Set code, e.g. `tmc`"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span its whole group (root + related sub-sets)"),
        ("type" = Option<String>, Query, description = "Deck type, e.g. `Commander Deck`"),
        ("sort" = Option<String>, Query, description = "`released` (default), `name`, or `price` (most valuable first). `name` also orders the groups; `price` orders the decks inside each group, the groups keep their natural order"),
    ),
    responses(
        (status = 200, description = "A page of groups, each with its preconstructed decks.", body = Page<PreconGroup>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_precon_groups(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<PreconListParams>,
) -> Result<Json<Page<PreconGroup>>, AppError> {
    require_game(&game)?;
    // Pages are *groups* here, each holding a handful of decks, so this uses the by-drop
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
    let scope = set_scope(&state, &game, &params).await?;
    let rows = sorted_query(filtered_query(&game, &params, scope.as_deref())?, &params)
        .all(&state.db)
        .await?;

    let names = set_name_map(&state, &game).await?;
    let set_dates = set_release_map(&state, &game).await?;
    let by_set = params.group == PreconGrouping::Set;
    // Bucket on the chosen key, keeping the order the rows arrived in (so every group leads
    // with whatever the list's own sort put first), then order the groups themselves.
    let mut buckets = group_rows(rows, |row| {
        if by_set {
            row.set_code.clone()
        } else {
            row.deck_type.clone()
        }
    });
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

    let data: Vec<PreconGroup> = on_page
        .into_iter()
        .map(|(key, decks)| {
            let shaped: Vec<PreconDeckResponse> = decks
                .iter()
                .map(|row| {
                    precon_response(
                        row,
                        names.get(&row.set_code).cloned(),
                        row.face_card_id.and_then(|id| faces.get(&id).cloned()),
                    )
                })
                .collect();
            PreconGroup {
                deck_count: shaped.len(),
                decks: shaped,
                // A set group is dated and links to its own page; a type group is neither.
                released_at: by_set
                    .then(|| {
                        set_dates.get(&key).cloned().flatten().or_else(|| {
                            decks
                                .iter()
                                .filter_map(|deck| deck.released_at.clone())
                                .max()
                        })
                    })
                    .flatten(),
                title: if by_set {
                    names
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| key.to_uppercase())
                } else {
                    key.clone()
                },
                set_code: by_set.then(|| key.clone()),
                slug: if by_set { key } else { slugify_type(&key) },
            }
        })
        .collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// A deck type as an anchor-safe key (`Commander Deck` -> `commander-deck`). Only ever a
/// fragment/`v-for` key — the wire filter is still the type verbatim, so nothing has to invert
/// this.
fn slugify_type(deck_type: &str) -> String {
    let mut out = String::with_capacity(deck_type.len());
    let mut pending = false;
    for ch in deck_type.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending = true;
        }
    }
    out
}

/// Bucket precon rows by a key, preserving the order the rows arrived in **both** for the
/// groups (the first row of a key names its position) and within each group. Every group
/// therefore leads with whatever the list's own sort put first.
fn group_rows(
    rows: Vec<precon_deck::Model>,
    key_of: impl Fn(&precon_deck::Model) -> String,
) -> Vec<(String, Vec<precon_deck::Model>)> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, Vec<precon_deck::Model>> = HashMap::new();
    for row in rows {
        let key = key_of(&row);
        by_key
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(row);
    }
    order
        .into_iter()
        .filter_map(|key| by_key.remove(&key).map(|decks| (key, decks)))
        .collect()
}

/// Order the groups.
///
/// * **By set:** newest set first. A set's date is the catalog's, not its decks' — a Secret Lair
///   deck released years after the `sld` set still belongs to `sld`, and the group should sit
///   where the set does. A set the catalog doesn't know falls back to its newest deck, then
///   sorts last if it has neither.
/// * **By type:** biggest category first, ties alphabetical — the same order the facets
///   endpoint gives the type dropdown, so the sections and the filter agree on what leads.
///
/// `sort=name` orders either grouping by heading instead, matching what it does to the decks.
/// `sort=price` deliberately does **not** re-order the groups: a group has no price of its
/// own worth claiming (a set's would just be its dearest deck), so the decks inside each
/// group carry the price order (via [`group_rows`]' arrival-order preservation) while the
/// groups keep the natural order above — the same layout-only stance the shared
/// `filtered_query` takes.
fn sort_buckets(
    buckets: &mut [(String, Vec<precon_deck::Model>)],
    set_dates: &HashMap<String, Option<String>>,
    names: &HashMap<String, String>,
    params: &PreconListParams,
) {
    let by_name = trim_query(params.sort.as_deref()) == Some("name");
    let by_set = params.group == PreconGrouping::Set;
    let date_of = |code: &String, decks: &Vec<precon_deck::Model>| -> String {
        set_dates
            .get(code)
            .cloned()
            .flatten()
            .or_else(|| decks.iter().filter_map(|d| d.released_at.clone()).max())
            .unwrap_or_default()
    };
    let title_of = |key: &String| -> String {
        if by_set {
            names
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.to_uppercase())
        } else {
            key.clone()
        }
    };
    buckets.sort_by(|(a_key, a_decks), (b_key, b_decks)| {
        if by_name {
            return title_of(a_key)
                .cmp(&title_of(b_key))
                .then_with(|| a_key.cmp(b_key));
        }
        if !by_set {
            // Biggest category first: a set page should open on its Commander decks, not on
            // whichever category happens to sort first alphabetically.
            return b_decks
                .len()
                .cmp(&a_decks.len())
                .then_with(|| a_key.cmp(b_key));
        }
        // Newest first; an undated set sinks below every dated one rather than leading.
        date_of(b_key, b_decks)
            .cmp(&date_of(a_key, a_decks))
            .then_with(|| a_key.cmp(b_key))
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

/// List preconstructed decks containing a card
///
/// `GET /api/games/{game}/cards/{id}/precons` -> the published preconstructed decks that
/// include this card — **any printing** of it (gameplay identity, as `/prints` resolves
/// siblings), on any board — newest deck first, name then slug as the tiebreak. Each entry
/// carries the precon's browse header plus the copy count, whether the inclusion is
/// foil-only, and whether the card leads the deck from its command zone. Paginated like
/// the precon browse (a format staple is in hundreds of decks); an empty first page when
/// the card is in none. `404` for an unknown game or card.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/{id}/precons",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (default 60, max 200)"),
    ),
    responses(
        (status = 200, description = "A page of the preconstructed decks containing any printing of the card, newest first.", body = Page<CardPreconRef>),
        (status = 404, description = "Unknown game or card."),
    ),
)]
pub async fn card_precons(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<CardPreconsParams>,
) -> Result<Json<Page<CardPreconRef>>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let (page, page_size) = resolve_page(
        params.page,
        params.page_size,
        DEFAULT_PAGE_SIZE,
        MAX_PAGE_SIZE,
    );

    // Every membership row for any printing of the card (hits
    // idx_precon_deck_cards_card_id). No game filter here — a precon card row carries
    // none; the printing ids are already scoped to this game's cards.
    let printing_ids = identity_printing_ids(&state, &card).await?;
    let rows: Vec<(i32, String, i32, bool)> = PreconDeckCard::find()
        .select_only()
        .column(precon_deck_card::Column::PreconDeckId)
        .column(precon_deck_card::Column::Board)
        .column(precon_deck_card::Column::Quantity)
        .column(precon_deck_card::Column::Foil)
        .filter(precon_deck_card::Column::CardId.is_in(printing_ids))
        .into_tuple()
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(build_page(Vec::new(), page, page_size, 0)));
    }

    // Fold to one entry per precon: copies summed across boards/printings/finishes, `foil`
    // ANDed down (foil-only inclusion, the sealed-membership rule), `commander` ORed up.
    struct Membership {
        quantity: i64,
        foil: bool,
        commander: bool,
    }
    let mut memberships: HashMap<i32, Membership> = HashMap::new();
    for (precon_id, board, quantity, foil) in rows {
        let entry = memberships.entry(precon_id).or_insert(Membership {
            quantity: 0,
            foil: true,
            commander: false,
        });
        entry.quantity += i64::from(quantity);
        entry.foil = entry.foil && foil;
        entry.commander = entry.commander || board == PreconBoard::Commander.as_str();
    }

    // Page over the referenced precon rows themselves (a membership row whose deck row
    // vanished mid-reimport simply doesn't come back), ordered exactly as the browse's
    // default sort so the two lists read the same way.
    let precon_ids: Vec<i32> = memberships.keys().copied().collect();
    let query = PreconDeck::find()
        .filter(precon_deck::Column::Game.eq(game.as_str()))
        .filter(precon_deck::Column::Id.is_in(precon_ids))
        .order_by_with_nulls(
            precon_deck::Column::ReleasedAt,
            sea_orm::Order::Desc,
            NullOrdering::Last,
        )
        .order_by_asc(precon_deck::Column::Name)
        .order_by_asc(precon_deck::Column::Slug);
    let paginator = query.paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let precon_rows = paginator.fetch_page(page - 1).await?;

    let names = set_name_map(&state, &game).await?;
    let faces = face_cards(&state, &precon_rows).await?;
    let data: Vec<CardPreconRef> = precon_rows
        .iter()
        .filter_map(|row| {
            memberships.get(&row.id).map(|m| CardPreconRef {
                precon: precon_response(
                    row,
                    names.get(&row.set_code).cloned(),
                    row.face_card_id.and_then(|id| faces.get(&id).cloned()),
                ),
                quantity: m.quantity,
                foil: m.foil,
                commander: m.commander,
            })
        })
        .collect();
    Ok(Json(build_page(data, page, page_size, total)))
}
