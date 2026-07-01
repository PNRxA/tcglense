//! Authenticated, per-user card-collection endpoints.
//!
//! A collection records how many copies of each card a signed-in user owns, per
//! game (`/api/collection/{game}/...`). Every route requires a valid access token
//! (via [`AuthUser`]) and is wired into the router's `private` group, so responses
//! are `Cache-Control: no-store` — per-user data must never be shared-cached.
//!
//! Card ids in the path are the provider's **external** id (the same id the public
//! catalog exposes); each is resolved to the internal `cards.id` before storage,
//! so a holding survives a catalog re-import and the stored `card_id` matches
//! `card_price_history`. Ownership is always scoped by `user.id` from the token, so
//! one user can never read or mutate another's collection.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    SelectTwo, Set, SqlErr,
};
use serde::{Deserialize, Serialize};

use crate::auth::extractor::AuthUser;
use crate::catalog::{self, Game};
use crate::collection_import::jobs::{self, JobStatus};
use crate::collection_import::{self, ImportSummary, Provider, ReconcileMode};
use crate::entities::prelude::{Card, CardSet, CollectionItem, CollectionSource};
use crate::entities::{card, card_set, collection_item, collection_source};
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::handlers::catalog::{
    CardResponse, SortDir, SortField, apply_card_sort, group_into_drops, group_set_codes, load_set,
    search_condition,
};
use crate::state::AppState;

const DEFAULT_PAGE_SIZE: u64 = 60;
const MAX_PAGE_SIZE: u64 = 200;
/// The by-drop collection view paginates over *drops* (each a handful of owned cards),
/// so it uses its own smaller bounds than the per-card lists — matching the catalog's
/// by-drop endpoint (`handlers::catalog`).
const DEFAULT_DROP_PAGE_SIZE: u64 = 20;
const MAX_DROP_PAGE_SIZE: u64 = 100;
/// A generous per-card holding cap: far above any real collection, but bounded so a
/// single count can't overflow the valuation arithmetic or be abused to store a
/// pathological value.
const MAX_QUANTITY: i32 = 1_000_000;
/// Cap on how many card ids one batch owned-counts lookup may request. A browse page
/// shows at most a few hundred cards, so this bounds the two `IN (...)` queries well
/// above any real page while staying under SQLite's bound-variable limit and refusing
/// an abusive request.
const MAX_OWNED_IDS: usize = 500;
/// Batch size for `IN (...)` card lookups, kept under SQLite's per-statement
/// bind-variable limit (as few as 999 on old builds) — a collection can hold far more
/// distinct cards than that once imported in bulk.
const CARD_LOOKUP_CHUNK: usize = 900;
/// Hard ceiling on an uploaded collection CSV, enforced as a route body limit (see the
/// router). Sized generously above any real collection *when exported with only the three
/// columns we ask for* (Scryfall ID, Finish, Quantity ≈ 60 bytes/row, so ~16 MB spans far
/// more than [`collection_import`]'s row cap) while bounding the memory a single upload
/// can force us to buffer + parse. A larger, all-columns export can exceed this — the UI
/// tells the user to export only the three needed columns.
pub const MAX_CSV_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

// ---------- Response / request DTOs ----------

/// One owned card: the full public card payload plus how many copies are owned.
#[derive(Debug, Serialize)]
pub struct CollectionEntry {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// One Secret Lair drop with the signed-in user's owned cards in it — the collection
/// mirror of the catalog's `DropGroupResponse`, but each card carries its owned counts.
/// The enclosing [`Page`] paginates over these (so `total` is a drop count, not cards).
#[derive(Debug, Serialize)]
pub struct CollectionDropGroup {
    /// Stable slug for anchors/links; `None` for the catch-all "Other" group of owned
    /// cards the snapshot doesn't place in a drop.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CollectionEntry>,
}

/// Just the owned counts for one card — what the card-detail controls read and write.
#[derive(Debug, Serialize)]
pub struct CollectionQuantities {
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Batch owned-counts response: external card id -> owned counts, for owned cards
/// only. Cards the user doesn't own are simply absent (never a zero entry), so a page
/// with nothing owned serialises to `{ "data": {} }`.
#[derive(Debug, Serialize)]
pub struct OwnedCountsResponse {
    pub data: HashMap<String, CollectionQuantities>,
}

/// Aggregate stats for a user's per-game collection (the collection landing header).
#[derive(Debug, Serialize)]
pub struct CollectionSummary {
    /// Distinct cards owned (one per collection row).
    pub unique_cards: i64,
    /// Total copies owned (regular + foil) across every card.
    pub total_cards: i64,
    /// Estimated USD value: regular copies at the card's `usd`, foil copies at
    /// `usd_foil`, as a 2-dp decimal string. `null` when nothing owned is priced.
    pub total_value_usd: Option<String>,
}

/// One set the user owns cards in, for the collection's per-set landing. Carries the
/// same catalog set metadata a set tile needs (so the SPA can reuse `SetTile`) plus how
/// much of it the user owns.
#[derive(Debug, Serialize, PartialEq)]
pub struct CollectionSet {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i32,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
    pub has_drops: bool,
    /// Distinct cards owned in this set.
    pub owned_cards: i64,
    /// Total copies owned (regular + foil) in this set.
    pub owned_copies: i64,
    /// Estimated USD value of the owned cards in this set (regular copies at `usd`,
    /// foil at `usd_foil`), a 2-dp decimal string. `null` when nothing owned is priced —
    /// same semantics as the summary's `total_value_usd`, scoped to the one set.
    pub owned_value_usd: Option<String>,
}

/// The sets a user owns cards in, newest set first.
#[derive(Debug, Serialize)]
pub struct CollectionSetsResponse {
    pub data: Vec<CollectionSet>,
}

/// A page of results plus the cursor metadata the SPA paginates with (mirrors the
/// catalog's page shape, kept local so the two modules stay decoupled).
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
}

/// Body of `PUT .../cards/{id}`: the desired absolute counts (not a delta). Setting
/// both to zero removes the card from the collection.
#[derive(Debug, Deserialize)]
pub struct SetQuantitiesRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Optional search query — the same Scryfall-style syntax the public catalog
    /// card lists accept (parsed by [`crate::scryfall::search`]); a malformed query
    /// is a 422. Absent/blank means no filter.
    #[serde(default)]
    pub q: Option<String>,
    /// Sort key. `updated` (the default) orders by most-recently-changed; every other
    /// key (`name`/`rarity`/`released`/`cmc`/`price`) reuses the catalog card sorts.
    /// An unknown value is a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`); absent = the sort key's natural direction. An
    /// unknown value is a 422.
    #[serde(default)]
    pub dir: Option<String>,
    /// Optional set-code scope: when present, only cards from that set are returned,
    /// ANDed with any `q`. Powers the per-set collection view. Absent/blank = every set.
    #[serde(default)]
    pub set: Option<String>,
    /// When `true` *and* a `set` scope is present, span the set's whole **group** (its
    /// top-level root plus every related sub-set) instead of just the one set — the
    /// collection mirror of the catalog's `include_related`. Ignored without a `set`.
    #[serde(default)]
    pub include_related: Option<bool>,
}

/// Query params for the (optionally set-scoped) collection summary.
#[derive(Debug, Deserialize)]
pub struct SummaryParams {
    /// Optional set-code scope — the summary is computed over just that set's owned
    /// cards. Absent/blank = the whole collection.
    #[serde(default)]
    pub set: Option<String>,
    /// When `true` *and* a `set` scope is present, span the set's whole **group** (root +
    /// related sub-sets) instead of just the one set — the collection mirror of the
    /// catalog's `include_related`, matching the list / ghost views. Ignored without a `set`.
    #[serde(default)]
    pub include_related: Option<bool>,
}

/// How the collection list is ordered: either the collection-specific recency order
/// (the default) or one of the shared catalog card sorts, reused verbatim so the
/// collection grid can sort identically to the browse grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectionSort {
    /// Most-recently added/updated first (by `collection_items.updated_at`).
    Recent,
    /// A card-column sort shared with the catalog card lists.
    Card(SortField),
}

/// Body of `POST .../owned`: the external card ids to look up owned counts for. Sent
/// as a POST body rather than a GET query so a browse page's (potentially few-hundred)
/// id list can't blow the request-line length behind a proxy.
#[derive(Debug, Deserialize)]
pub struct OwnedCountsRequest {
    pub ids: Vec<String>,
}

impl ListParams {
    /// Resolve the requested 1-based page and clamp the page size to `[1, MAX]`.
    fn page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        (page, page_size)
    }

    /// The trimmed search query, or `None` when it's absent or blank.
    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    /// The trimmed set-code scope, or `None` when it's absent or blank.
    fn set(&self) -> Option<&str> {
        self.set.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// Whether to span the scoped set's whole group (the include-related view). Only
    /// meaningful alongside a `set` scope; the handler ignores it otherwise.
    fn include_related(&self) -> bool {
        self.include_related.unwrap_or(false)
    }

    /// Resolve the requested 1-based page and clamp the page size for the by-drop
    /// view, which paginates over drops (not cards) and so has its own smaller bounds.
    fn drop_page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self
            .page_size
            .unwrap_or(DEFAULT_DROP_PAGE_SIZE)
            .clamp(1, MAX_DROP_PAGE_SIZE);
        (page, page_size)
    }

    /// Resolve the `sort`/`dir` params into a validated `(sort, direction)`,
    /// defaulting to most-recently-updated. An unrecognised key/direction is a 422 —
    /// consistent with a malformed `q` — rather than being silently ignored.
    fn sort_spec(&self) -> Result<(CollectionSort, SortDir), AppError> {
        let (sort, default_dir) = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            // The collection's natural default (and its explicit key) is recency.
            None | Some("updated" | "recent") => (CollectionSort::Recent, SortDir::Desc),
            Some(value) => {
                let field = SortField::parse(value)?;
                (CollectionSort::Card(field), field.default_dir())
            }
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => default_dir,
            Some(value) => SortDir::parse(value)?,
        };
        Ok((sort, dir))
    }
}

// ---------- Handlers ----------

/// `GET /api/collection/{game}` -> the signed-in user's owned cards for a game,
/// most-recently-updated first, paginated. Each entry carries the full card payload
/// plus the owned counts.
pub async fn list_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();
    let (sort, dir) = params.sort_spec()?;
    // Parse the optional Scryfall-syntax query up front so a malformed one 422s
    // before we touch the DB (mirrors the catalog card lists).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s))
        .transpose()?;

    // Resolve the (optional) set scope: a single set, or — with `include_related` — the
    // set's whole group (root + related sub-sets), spanning exactly the sets the catalog
    // does. `None` means the whole collection.
    let set_codes =
        resolve_set_scope(&state, &game, params.set(), params.include_related()).await?;

    let paginator = collection_query(user.id, &game, set_codes.as_deref(), search, sort, dir)
        .paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    // `find_also_related` is a LEFT join, so a holding whose card row is gone (e.g.
    // removed by a catalog re-import) comes back with `None` — skip it, exactly as
    // the summary/valuation reads do.
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

/// Build the collection-list query for a user + game: join `cards` (so it can be
/// searched and sorted on card columns), apply the optional already-parsed search
/// condition, and order by the chosen sort. Kept separate from the handler so the
/// join/filter/sort can be unit-tested against a seeded DB without an `AppState`.
///
/// The search condition, the set scope, and the card sort touch only `cards` columns;
/// the `user_id` and `game` filters and the recency sort stay entity-qualified to
/// `collection_items`, so nothing is ambiguous across the join (both tables carry a
/// `game` column).
///
/// `set_codes` scopes to the joined card's `set_code`: `None` = the whole collection,
/// a single code = the per-set view, several codes = the include-related group view.
fn collection_query(
    user_id: i32,
    game: &str,
    set_codes: Option<&[String]>,
    search: Option<Condition>,
    sort: CollectionSort,
    dir: SortDir,
) -> SelectTwo<collection_item::Entity, card::Entity> {
    let mut query = CollectionItem::find()
        .find_also_related(Card)
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game));
    // Scope to one set (per-set view) or several (the include-related group) by the
    // joined card row's `set_code`. An empty slice would match nothing, but the scope
    // resolver never produces one (a group always contains at least the set itself).
    if let Some(codes) = set_codes {
        query = query.filter(card::Column::SetCode.is_in(codes.iter().map(String::as_str)));
    }
    if let Some(condition) = search {
        query = query.filter(condition);
    }
    match sort {
        // Newest change first (or oldest, if reversed), with a stable id tiebreaker
        // for deterministic paging.
        CollectionSort::Recent => query
            .order_by(collection_item::Column::UpdatedAt, dir.order())
            .order_by(collection_item::Column::Id, dir.order()),
        CollectionSort::Card(field) => apply_card_sort(query, field, dir, false),
    }
}

/// Resolve the set-code scope for a collection list: `None` (no scope, the whole
/// collection), a single-code slice (the per-set view), or — with `include_related` —
/// the scoped set's whole group (root + related sub-sets), resolved from the same flat
/// set list the catalog uses ([`group_set_codes`]) so both span identical sets. Only
/// fetches the set list when a group actually needs resolving.
async fn resolve_set_scope(
    state: &AppState,
    game: &str,
    set: Option<&str>,
    include_related: bool,
) -> Result<Option<Vec<String>>, AppError> {
    let Some(code) = set else { return Ok(None) };
    if !include_related {
        return Ok(Some(vec![code.to_string()]));
    }
    let all_sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game))
        .all(&state.db)
        .await?;
    Ok(Some(group_set_codes(&all_sets, code)))
}

/// `GET /api/collection/{game}/summary` -> aggregate stats (distinct cards, total
/// copies, estimated USD value) for the signed-in user's collection in a game.
/// An optional `?set` scopes the stats to a single set (the per-set collection view);
/// `?include_related=true` with a set spans its whole group (root + related sub-sets),
/// so the header value matches the set / include-related collection browse view.
pub async fn collection_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;

    // A set-scoped summary joins the cards up front (bounded by the scoped sets' owned
    // cards) rather than the whole-collection two-step load below. `resolve_set_scope`
    // returns the one set, or its whole group under include-related, or `None` (no scope)
    // — the same resolution the collection list uses, so the value spans identical sets.
    let set = params.set.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(set_codes) =
        resolve_set_scope(&state, &game, set, params.include_related.unwrap_or(false)).await?
    {
        return Ok(Json(scoped_summary(&state, user.id, &game, &set_codes).await?));
    }

    // A collection is bounded by how many distinct cards a user owns, so we load the
    // rows and their cards and total copies + value in Rust (never trusting the
    // stored decimal price strings to SQL arithmetic).
    let rows = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(CollectionSummary {
            unique_cards: 0,
            total_cards: 0,
            total_value_usd: None,
        }));
    }

    let unique_cards = rows.len() as i64;
    let total_cards: i64 = rows
        .iter()
        .map(|r| i64::from(r.quantity) + i64::from(r.foil_quantity))
        .sum();

    // Load the owned cards' prices, chunked so a large collection (now reachable via
    // import) can't exceed SQLite's per-statement bind-variable limit.
    let card_ids: Vec<i32> = rows.iter().map(|r| r.card_id).collect();
    let mut by_id: HashMap<i32, card::Model> = HashMap::new();
    for chunk in card_ids.chunks(CARD_LOOKUP_CHUNK) {
        let cards = Card::find()
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for c in cards {
            by_id.insert(c.id, c);
        }
    }

    let mut total_cents: i128 = 0;
    let mut any_priced = false;
    for r in &rows {
        let Some(card) = by_id.get(&r.card_id) else {
            continue;
        };
        if let Some(cents) = price_cents(card.price_usd.as_deref()) {
            total_cents += cents * i128::from(r.quantity);
            any_priced = true;
        }
        if let Some(cents) = price_cents(card.price_usd_foil.as_deref()) {
            total_cents += cents * i128::from(r.foil_quantity);
            any_priced = true;
        }
    }

    Ok(Json(CollectionSummary {
        unique_cards,
        total_cards,
        total_value_usd: any_priced.then(|| format_cents(total_cents)),
    }))
}

/// Aggregate stats for one set (or a whole related-set group) of the user's collection.
/// Joins the cards up front (bounded by the scoped sets' owned cards) so the price/value
/// pass reads the same rows — no second chunked load. Holdings whose card row is gone (a
/// catalog re-import) are left-joined to `None` and skipped, exactly as the whole-collection
/// summary does. `set_codes` is never empty — `resolve_set_scope` always yields at least the
/// scoped set itself.
async fn scoped_summary(
    state: &AppState,
    user_id: i32,
    game: &str,
    set_codes: &[String],
) -> Result<CollectionSummary, AppError> {
    let rows = CollectionItem::find()
        .find_also_related(Card)
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(card::Column::SetCode.is_in(set_codes.iter().map(String::as_str)))
        .all(&state.db)
        .await?;

    let mut unique_cards: i64 = 0;
    let mut total_cards: i64 = 0;
    let mut total_cents: i128 = 0;
    let mut any_priced = false;
    for (item, card) in &rows {
        let Some(card) = card else { continue };
        unique_cards += 1;
        total_cards += i64::from(item.quantity) + i64::from(item.foil_quantity);
        if let Some(cents) = price_cents(card.price_usd.as_deref()) {
            total_cents += cents * i128::from(item.quantity);
            any_priced = true;
        }
        if let Some(cents) = price_cents(card.price_usd_foil.as_deref()) {
            total_cents += cents * i128::from(item.foil_quantity);
            any_priced = true;
        }
    }

    Ok(CollectionSummary {
        unique_cards,
        total_cards,
        total_value_usd: any_priced.then(|| format_cents(total_cents)),
    })
}

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
    let rows = CollectionItem::find()
        .find_also_related(Card)
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;

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
    let table = crate::scryfall::drops::table(&game, &set.code)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::NotFound(format!("set '{}' has no drops", set.code)))?;

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
    let total = buckets.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let data: Vec<CollectionDropGroup> = buckets
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(|bucket| CollectionDropGroup {
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
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
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
    /// Estimated USD value in integer cents (regular at `usd`, foil at `usd_foil`).
    value_cents: i128,
    /// Whether any owned card in the set had a usable price, so an all-unpriced set
    /// reports `null` value rather than `$0.00` (matching the summary).
    any_priced: bool,
}

/// Aggregate owned holdings into per-set tiles: count distinct owned cards + total
/// copies + estimated value per `set_code`, dress each with the game's set metadata
/// (falling back to the card's own `set_name` when the set row is missing), and order
/// newest set first (undated last), tie-broken by code for deterministic output. Pure so
/// it can be unit-tested without a DB. Holdings whose card row is gone are skipped.
fn build_collection_sets(
    game: &str,
    rows: Vec<(collection_item::Model, Option<card::Model>)>,
    sets: Vec<card_set::Model>,
) -> Vec<CollectionSet> {
    let mut agg: HashMap<String, SetAgg> = HashMap::new();
    for (item, card) in rows {
        let Some(card) = card else { continue };
        // Price each finish before moving the card's set_code/set_name into the map,
        // so the borrow is clean regardless of aggregation order.
        let regular_cents = price_cents(card.price_usd.as_deref());
        let foil_cents = price_cents(card.price_usd_foil.as_deref());
        let entry = agg.entry(card.set_code).or_insert_with(|| SetAgg {
            fallback_name: card.set_name,
            ..SetAgg::default()
        });
        entry.owned_cards += 1;
        entry.owned_copies += i64::from(item.quantity) + i64::from(item.foil_quantity);
        if let Some(cents) = regular_cents {
            entry.value_cents += cents * i128::from(item.quantity);
            entry.any_priced = true;
        }
        if let Some(cents) = foil_cents {
            entry.value_cents += cents * i128::from(item.foil_quantity);
            entry.any_priced = true;
        }
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
                value_cents,
                any_priced,
            } = agg;
            let owned_value_usd = any_priced.then(|| format_cents(value_cents));
            let has_drops = crate::scryfall::drops::has_drops(game, &code);
            match meta.get(&code) {
                Some(m) => CollectionSet {
                    code: m.code.clone(),
                    name: m.name.clone(),
                    set_type: m.set_type.clone(),
                    released_at: m.released_at.clone(),
                    card_count: m.card_count,
                    icon_svg_uri: m.icon_svg_uri.clone(),
                    parent_set_code: m.parent_set_code.clone(),
                    has_drops,
                    owned_cards,
                    owned_copies,
                    owned_value_usd,
                },
                // A set present in a holding but absent from card_sets (e.g. metadata
                // not yet synced): degrade to a bare tile using the card's set name.
                None => CollectionSet {
                    code,
                    name: fallback_name,
                    set_type: None,
                    released_at: None,
                    card_count: 0,
                    icon_svg_uri: None,
                    parent_set_code: None,
                    has_drops,
                    owned_cards,
                    owned_copies,
                    owned_value_usd,
                },
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

/// `GET /api/collection/{game}/cards/{id}` -> how many copies of one card the user
/// owns (zeros when the card isn't in their collection). `id` is the external card
/// id; a `404` means the game or card is unknown.
pub async fn get_collection_entry(
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

/// `POST /api/collection/{game}/owned` -> the owned counts for the subset of the
/// given external card ids that the signed-in user actually owns, keyed by external
/// id. Cards the user doesn't own are absent from the map (so an all-unowned page
/// returns `{ "data": {} }`). This backs the owned-count badges overlaid on the
/// public browse grids without an N+1 of per-card lookups. `422` if more than
/// [`MAX_OWNED_IDS`] ids are requested at once.
pub async fn owned_counts(
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
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::ExternalId.is_in(external_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c.external_id))
        .collect();
    if external_by_internal.is_empty() {
        return Ok(Json(OwnedCountsResponse {
            data: HashMap::new(),
        }));
    }

    // One query for the user's holdings among those cards; a card with no row is
    // simply not owned and contributes nothing to the map.
    let internal_ids: Vec<i32> = external_by_internal.keys().copied().collect();
    let rows = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .filter(collection_item::Column::CardId.is_in(internal_ids))
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

/// `PUT /api/collection/{game}/cards/{id}` -> set the owned counts for one card
/// (absolute values, not a delta). Both zero removes the card from the collection.
/// Returns the resulting counts. `404` for an unknown game/card, `422` for a
/// negative or oversized count.
pub async fn set_collection_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let card = load_card(&state, &game, &id).await?;

    let existing = find_row(&state, user.id, &game, card.id).await?;
    let now = Utc::now();

    // Owning zero of both is "not in the collection": drop the row if present.
    if quantity == 0 && foil_quantity == 0 {
        if let Some(row) = existing {
            CollectionItem::delete_by_id(row.id)
                .exec(&state.db)
                .await?;
        }
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    match existing {
        Some(row) => {
            let mut active: collection_item::ActiveModel = row.into();
            active.quantity = Set(quantity);
            active.foil_quantity = Set(foil_quantity);
            active.updated_at = Set(now);
            active.update(&state.db).await?;
        }
        None => {
            let active = collection_item::ActiveModel {
                user_id: Set(user.id),
                game: Set(game.clone()),
                card_id: Set(card.id),
                quantity: Set(quantity),
                foil_quantity: Set(foil_quantity),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            // The unique (user, game, card) index is the real source of truth: two
            // concurrent first-adds can both see `None`, so a unique violation means
            // we lost the race — fall back to updating the row that won.
            if let Err(err) = active.insert(&state.db).await {
                if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                    if let Some(row) = find_row(&state, user.id, &game, card.id).await? {
                        let mut active: collection_item::ActiveModel = row.into();
                        active.quantity = Set(quantity);
                        active.foil_quantity = Set(foil_quantity);
                        active.updated_at = Set(now);
                        active.update(&state.db).await?;
                    }
                } else {
                    return Err(err.into());
                }
            }
        }
    }

    Ok(Json(CollectionQuantities {
        quantity,
        foil_quantity,
    }))
}

// ---------- Import / sync from an external collection provider ----------

/// Body of `POST .../import`: which provider, the source URL/id, and how to reconcile.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub provider: String,
    pub source: String,
    pub mode: ReconcileMode,
}

/// Body of `PUT .../source`: the collection link to remember (provider + source URL/id),
/// plus whether saved re-syncs should use smart (incremental) sync. `smart` defaults to
/// `false` (full mirror) when omitted.
#[derive(Debug, Deserialize)]
pub struct SaveSourceRequest {
    pub provider: String,
    pub source: String,
    #[serde(default)]
    pub smart: bool,
}

/// A saved external collection link for a game.
#[derive(Debug, Serialize)]
pub struct CollectionSourceResponse {
    pub provider: &'static str,
    pub external_id: String,
    /// A canonical, user-facing URL for the collection on the provider.
    pub url: String,
    /// RFC3339 timestamp of the last successful sync, or null if never synced.
    pub last_synced_at: Option<String>,
    /// Whether a saved re-sync uses smart (incremental) sync rather than a full mirror.
    pub smart: bool,
}

/// The status of a background import/sync job — returned when one is enqueued and each
/// time the client polls. Imports run asynchronously (throttled by the provider rate
/// limit), so the client kicks one off and polls this until `complete`/`error`.
#[derive(Debug, Serialize)]
pub struct ImportJobResponse {
    pub job_id: u64,
    /// `queued` | `running` | `complete` | `error`.
    pub status: &'static str,
    /// The import summary, present only when `status == "complete"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ImportSummary>,
    /// A user-facing message, present only when `status == "error"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ImportJobResponse {
    fn from_status(job_id: u64, status: JobStatus) -> Self {
        let (status, summary, error) = match status {
            JobStatus::Queued => ("queued", None, None),
            JobStatus::Running => ("running", None, None),
            JobStatus::Complete(summary) => ("complete", Some(summary), None),
            JobStatus::Failed(message) => ("error", None, Some(message)),
        };
        Self {
            job_id,
            status,
            summary,
            error,
        }
    }
}

/// `POST /api/collection/{game}/import` -> enqueue a one-off import from a collection
/// provider using the chosen reconcile mode (does not save the link). Validates the
/// request synchronously, then returns `202` with a job id to poll; the fetch +
/// reconcile run in the background, throttled by the provider rate limit.
pub async fn import_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<ImportRequest>,
) -> Result<(StatusCode, Json<ImportJobResponse>), AppError> {
    require_game(&game)?;
    let provider = parse_provider(&payload.provider)?;
    if !provider.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "{} import is not available for '{}'",
            provider.label(),
            game
        )));
    }
    // Resolve the source id up front so a bad URL/id is an immediate 422, not a job that
    // fails later.
    let collection_id = collection_import::parse_source(provider, &payload.source)?;

    let job_id = jobs::spawn_import_job(
        state.db.clone(),
        state.http.clone(),
        state.imports.clone(),
        jobs::ImportRequest {
            user_id: user.id,
            game,
            provider,
            collection_id,
            mode: payload.mode,
            stamp_source_synced: false,
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

/// Query params for a CSV upload. The reconcile `mode` rides in the query string so the
/// request body can be the raw CSV file. Parsed as an optional string (not a typed
/// `ReconcileMode`) so a missing/invalid mode is our JSON `422`, not axum's default
/// text/plain query-rejection.
#[derive(Debug, Deserialize)]
pub struct CsvImportParams {
    pub mode: Option<String>,
}

/// `POST /api/collection/{game}/import/csv?mode=...` -> import a collection from an
/// uploaded Archidekt CSV export. The request body is the raw CSV file (bounded by the
/// route's body limit, [`MAX_CSV_UPLOAD_BYTES`]); the reconcile mode is a query param.
///
/// Unlike the URL import this needs no upstream fetch, so it reconciles **synchronously**
/// and returns the [`ImportSummary`] directly (no rate limiter, no background job): a CSV
/// has no location to re-sync from, so it's inherently one-off. `404` for an unknown game,
/// `422` for a bad mode / unreadable CSV / one missing a required column / an empty upload.
pub async fn import_collection_csv(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<CsvImportParams>,
    body: Bytes,
) -> Result<Json<ImportSummary>, AppError> {
    require_game(&game)?;
    // The CSV shape is Archidekt's, and Archidekt is Magic-only (its card ids are Scryfall
    // ids); gate on the same provider/game support as the URL import.
    if !Provider::Archidekt.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "CSV collection import is not available for '{game}'"
        )));
    }
    let mode = parse_reconcile_mode(params.mode.as_deref())?;
    if body.is_empty() {
        return Err(AppError::Validation("no CSV file was uploaded".to_string()));
    }

    let summary = collection_import::execute_csv_import(&state.db, user.id, &game, mode, &body)
        .await
        .map_err(AppError::from)?;
    Ok(Json(summary))
}

/// `GET /api/collection/{game}/import/jobs/{job_id}` -> the status of a background
/// import/sync job (queued / running / complete / error). `404` for an unknown job or
/// one that isn't the caller's.
pub async fn get_import_job(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, job_id)): Path<(String, u64)>,
) -> Result<Json<ImportJobResponse>, AppError> {
    require_game(&game)?;
    let status = state
        .imports
        .status(job_id, user.id, &game)
        .ok_or_else(|| AppError::NotFound("import job not found".to_string()))?;
    Ok(Json(ImportJobResponse::from_status(job_id, status)))
}

/// `GET /api/collection/{game}/source` -> the saved collection link for this game, or
/// `null` if none is saved.
pub async fn get_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<Option<CollectionSourceResponse>>, AppError> {
    require_game(&game)?;
    let row = find_source(&state, user.id, &game).await?;
    Ok(Json(row.map(source_response)))
}

/// `PUT /api/collection/{game}/source` -> save (upsert) the collection link for this
/// game. Validates that the source resolves to a provider collection id; does not sync.
pub async fn save_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<SaveSourceRequest>,
) -> Result<Json<CollectionSourceResponse>, AppError> {
    require_game(&game)?;
    let provider = parse_provider(&payload.provider)?;
    if !provider.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "{} import is not available for '{}'",
            provider.label(),
            game
        )));
    }
    // Validate + normalise the source to a bare provider collection id.
    let external_id = collection_import::parse_source(provider, &payload.source)?;
    let now = Utc::now();

    // Pointing the link at a different collection resets the sync marker; re-saving the
    // same link preserves it. (Read the current link to decide, then upsert atomically.)
    let existing = find_source(&state, user.id, &game).await?;
    let changed = existing
        .as_ref()
        .is_none_or(|e| e.provider != provider.as_str() || e.external_id != external_id);
    let last_synced_at = if changed {
        None
    } else {
        existing.as_ref().and_then(|e| e.last_synced_at)
    };

    // Upsert on the unique (user, game) index so two concurrent first-time saves can't
    // 500 on a unique violation (matches the collection-item upsert's race-safety).
    let active = collection_source::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        provider: Set(provider.as_str().to_string()),
        external_id: Set(external_id),
        last_synced_at: Set(last_synced_at),
        smart: Set(payload.smart),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    CollectionSource::insert(active)
        .on_conflict(
            OnConflict::columns([
                collection_source::Column::UserId,
                collection_source::Column::Game,
            ])
            .update_columns([
                collection_source::Column::Provider,
                collection_source::Column::ExternalId,
                collection_source::Column::LastSyncedAt,
                collection_source::Column::Smart,
                collection_source::Column::UpdatedAt,
            ])
            .to_owned(),
        )
        .exec(&state.db)
        .await?;

    // Read back the canonical row for the response.
    let saved = find_source(&state, user.id, &game)
        .await?
        .ok_or_else(|| AppError::Internal("saved collection source vanished".to_string()))?;
    Ok(Json(source_response(saved)))
}

/// `DELETE /api/collection/{game}/source` -> forget the saved collection link.
/// Idempotent: deleting when nothing is saved still returns `204`.
pub async fn delete_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    if let Some(existing) = find_source(&state, user.id, &game).await? {
        CollectionSource::delete_by_id(existing.id)
            .exec(&state.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/collection/{game}/sync` -> enqueue a re-sync from the saved collection
/// link; the worker stamps `last_synced_at` on success. Uses smart (incremental) sync
/// when the saved link opted into it, otherwise a full mirror/replace. Returns `202`
/// with a job id to poll. `404` when no link is saved.
pub async fn sync_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<(StatusCode, Json<ImportJobResponse>), AppError> {
    require_game(&game)?;
    let source = find_source(&state, user.id, &game)
        .await?
        .ok_or_else(|| AppError::NotFound("no saved collection link to sync".to_string()))?;
    let provider = Provider::from_id(&source.provider).ok_or_else(|| {
        AppError::Internal(format!(
            "stored collection provider '{}' is unknown",
            source.provider
        ))
    })?;

    // The saved link records how it re-syncs: smart (incremental, only recently-changed
    // cards) or a full mirror (removes cards no longer upstream). Both stamp the source
    // as synced; the UI tailors its confirmation to which one runs.
    let mode = if source.smart {
        ReconcileMode::Smart
    } else {
        ReconcileMode::Replace
    };

    let job_id = jobs::spawn_import_job(
        state.db.clone(),
        state.http.clone(),
        state.imports.clone(),
        jobs::ImportRequest {
            user_id: user.id,
            game,
            provider,
            collection_id: source.external_id,
            mode,
            // Stamp `last_synced_at` on success (this is a re-sync of the saved link).
            stamp_source_synced: true,
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

// ---------- Helpers ----------

fn require_game(game: &str) -> Result<&'static Game, AppError> {
    catalog::find(game).ok_or_else(|| AppError::NotFound(format!("unknown game '{game}'")))
}

/// Parse a collection-provider id from a request, 422 on an unknown provider.
fn parse_provider(s: &str) -> Result<Provider, AppError> {
    Provider::from_id(s)
        .ok_or_else(|| AppError::Validation(format!("unknown collection provider '{s}'")))
}

/// Parse a reconcile mode from a query param, 422 when absent or unrecognised. Used by
/// the CSV upload, where the mode is a query param rather than a typed JSON field (so a
/// bad value returns our JSON error, not axum's default query rejection).
fn parse_reconcile_mode(s: Option<&str>) -> Result<ReconcileMode, AppError> {
    match s.map(str::trim) {
        Some("overwrite") => Ok(ReconcileMode::Overwrite),
        Some("replace") => Ok(ReconcileMode::Replace),
        Some("merge") => Ok(ReconcileMode::Merge),
        _ => Err(AppError::Validation(
            "mode must be one of: overwrite, replace, merge".to_string(),
        )),
    }
}

/// The user's saved collection link for a game, if any.
async fn find_source(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<Option<collection_source::Model>, AppError> {
    Ok(CollectionSource::find()
        .filter(collection_source::Column::UserId.eq(user_id))
        .filter(collection_source::Column::Game.eq(game))
        .one(&state.db)
        .await?)
}

/// Shape a stored source row for the API, resolving its provider to a canonical URL.
fn source_response(row: collection_source::Model) -> CollectionSourceResponse {
    // A stored provider id should always resolve; degrade to a URL-less response rather
    // than failing the read if a future/renamed provider id lingers in an old row.
    let (provider, url) = match Provider::from_id(&row.provider) {
        Some(p) => (p.as_str(), p.collection_url(&row.external_id)),
        None => ("unknown", String::new()),
    };
    CollectionSourceResponse {
        provider,
        external_id: row.external_id,
        url,
        last_synced_at: row.last_synced_at.map(|t| t.to_rfc3339()),
        smart: row.smart,
    }
}

/// Resolve a card by its external (provider) id within a game, 404 if unknown.
async fn load_card(state: &AppState, game: &str, external_id: &str) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(external_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{external_id}' not found")))
}

/// The user's collection row for a card, if any.
async fn find_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    card_id: i32,
) -> Result<Option<collection_item::Model>, AppError> {
    Ok(CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(collection_item::Column::CardId.eq(card_id))
        .one(&state.db)
        .await?)
}

/// Trim, drop blanks, and de-duplicate a batch of requested external card ids,
/// preserving first-seen order (so the `IN (...)` bind list has no repeats and a
/// sloppy client list is tolerated).
fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

fn build_page<T>(data: Vec<T>, page: u64, page_size: u64, total: u64) -> Page<T> {
    Page {
        data,
        page,
        page_size,
        total,
        has_more: page.saturating_mul(page_size) < total,
    }
}

fn validate_quantity(value: i32, field: &str) -> Result<i32, AppError> {
    if value < 0 {
        return Err(AppError::Validation(format!(
            "{field} must not be negative"
        )));
    }
    if value > MAX_QUANTITY {
        return Err(AppError::Validation(format!(
            "{field} must be at most {MAX_QUANTITY}"
        )));
    }
    Ok(value)
}

/// Parse a stored decimal price string (e.g. `"12.34"`) to integer USD cents,
/// rounding to the nearest cent. `None`/empty/unparseable yields `None` so an
/// unpriced card simply doesn't contribute to a valuation.
fn price_cents(price: Option<&str>) -> Option<i128> {
    let value: f64 = price?.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((value * 100.0).round() as i128)
}

/// Format integer USD cents as a 2-dp decimal string (e.g. `1234` -> `"12.34"`).
fn format_cents(cents: i128) -> String {
    let dollars = cents / 100;
    let rem = (cents % 100).abs();
    format!("{dollars}.{rem:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `ListParams` with only paging set — the search/sort tests override
    /// `q`/`sort`/`dir` via struct-update on top of this.
    fn params(page: Option<u64>, page_size: Option<u64>) -> ListParams {
        ListParams {
            page,
            page_size,
            q: None,
            sort: None,
            dir: None,
            set: None,
            include_related: None,
        }
    }

    #[test]
    fn page_and_size_defaults_and_clamps() {
        assert_eq!(params(None, None).page_and_size(), (1, DEFAULT_PAGE_SIZE));
        assert_eq!(params(Some(0), Some(9999)).page_and_size(), (1, MAX_PAGE_SIZE));
        assert_eq!(params(Some(3), Some(20)).page_and_size(), (3, 20));
    }

    #[test]
    fn search_trims_and_blank_filters() {
        assert_eq!(
            ListParams {
                q: Some("  goblin ".into()),
                ..params(None, None)
            }
            .search(),
            Some("goblin")
        );
        assert_eq!(
            ListParams {
                q: Some("   ".into()),
                ..params(None, None)
            }
            .search(),
            None
        );
        assert_eq!(params(None, None).search(), None);
    }

    #[test]
    fn sort_spec_defaults_to_recent_and_reuses_card_sorts() {
        // Absent, and the explicit recency keys, all resolve to newest-first.
        for sort in [None, Some("updated"), Some("recent")] {
            assert_eq!(
                ListParams {
                    sort: sort.map(str::to_string),
                    ..params(None, None)
                }
                .sort_spec()
                .unwrap(),
                (CollectionSort::Recent, SortDir::Desc)
            );
        }
        // A reversed recency (oldest first).
        assert_eq!(
            ListParams {
                sort: Some("updated".into()),
                dir: Some("asc".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Recent, SortDir::Asc)
        );
        // Card sorts borrow the catalog field + its natural direction.
        assert_eq!(
            ListParams {
                sort: Some("name".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Card(SortField::Name), SortDir::Asc)
        );
        assert_eq!(
            ListParams {
                sort: Some("price".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Card(SortField::Price), SortDir::Desc)
        );
    }

    #[test]
    fn sort_spec_rejects_unknown_values() {
        assert!(matches!(
            ListParams {
                sort: Some("nonsense".into()),
                ..params(None, None)
            }
            .sort_spec(),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            ListParams {
                dir: Some("sideways".into()),
                ..params(None, None)
            }
            .sort_spec(),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn build_page_derives_has_more() {
        let page = build_page(vec![1, 2, 3], 1, 3, 10);
        assert!(page.has_more, "more rows remain after page 1");
        let page = build_page(vec![1], 4, 3, 10);
        assert!(!page.has_more, "page 4 of 10 rows is the last");
        let page = build_page(Vec::<i32>::new(), 1, 60, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn dedupe_ids_trims_dedupes_and_drops_blanks() {
        let out = dedupe_ids(vec![
            "  a ".into(),
            "b".into(),
            "a".into(),
            "".into(),
            "   ".into(),
            "b".into(),
            "c".into(),
        ]);
        // First-seen order preserved, blanks gone, no repeats.
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn parse_reconcile_mode_accepts_known_modes_and_rejects_others() {
        assert!(matches!(
            parse_reconcile_mode(Some("overwrite")),
            Ok(ReconcileMode::Overwrite)
        ));
        assert!(matches!(
            parse_reconcile_mode(Some(" replace ")),
            Ok(ReconcileMode::Replace)
        ));
        assert!(matches!(
            parse_reconcile_mode(Some("merge")),
            Ok(ReconcileMode::Merge)
        ));
        // Missing or unrecognised -> our JSON validation error (422), never a silent default.
        assert!(matches!(
            parse_reconcile_mode(None),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            parse_reconcile_mode(Some("wipe")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn validate_quantity_bounds() {
        assert_eq!(validate_quantity(0, "quantity").unwrap(), 0);
        assert_eq!(validate_quantity(5, "quantity").unwrap(), 5);
        assert!(matches!(
            validate_quantity(-1, "quantity"),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_quantity(MAX_QUANTITY + 1, "foil_quantity"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn price_cents_parses_and_rounds() {
        assert_eq!(price_cents(Some("12.34")), Some(1234));
        assert_eq!(price_cents(Some("0.5")), Some(50));
        assert_eq!(price_cents(Some("  1  ")), Some(100));
        assert_eq!(price_cents(Some("0.005")), Some(1)); // rounds to nearest cent
        assert_eq!(price_cents(Some("")), None);
        assert_eq!(price_cents(Some("n/a")), None);
        assert_eq!(price_cents(None), None);
    }

    #[test]
    fn format_cents_renders_two_decimals() {
        assert_eq!(format_cents(1234), "12.34");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(100), "1.00");
        assert_eq!(format_cents(0), "0.00");
    }

    /// A minimal `mtg` card row: only the fields the collection search/sort tests
    /// exercise (name, type line, USD price) are meaningful; the rest are defaulted.
    fn seed_card(id: i32, name: &str, type_line: &str, price_usd: Option<&str>) -> card::Model {
        let ts = "2024-01-01T00:00:00Z"
            .parse::<sea_orm::prelude::DateTimeUtc>()
            .unwrap();
        card::Model {
            id,
            game: "mtg".into(),
            external_id: format!("ext-{id}"),
            oracle_id: None,
            name: name.into(),
            set_code: "tst".into(),
            set_name: "TST".into(),
            collector_number: id.to_string(),
            collector_number_int: Some(id),
            rarity: None,
            lang: "en".into(),
            released_at: None,
            mana_cost: None,
            cmc: None,
            type_line: Some(type_line.into()),
            color_identity: None,
            colors: None,
            layout: None,
            oracle_text: None,
            power: None,
            toughness: None,
            loyalty: None,
            image_small: None,
            image_normal: None,
            image_large: None,
            image_art_crop: None,
            image_png: None,
            card_faces: None,
            price_usd: price_usd.map(str::to_string),
            price_usd_foil: None,
            price_usd_etched: None,
            price_eur: None,
            price_tix: None,
            keywords: None,
            produced_mana: None,
            color_indicator: None,
            watermark: None,
            flavor_text: None,
            illustration_id: None,
            artist: None,
            artist_ids: None,
            border_color: None,
            frame: None,
            frame_effects: None,
            security_stamp: None,
            promo_types: None,
            finishes: None,
            defense: None,
            legalities: None,
            full_art: None,
            textless: None,
            oversized: None,
            promo: None,
            reprint: None,
            variation: None,
            booster: None,
            story_spotlight: None,
            content_warning: None,
            highres_image: None,
            reserved: None,
            game_changer: None,
            edhrec_rank: None,
            penny_rank: None,
            digital: false,
            created_at: ts,
            updated_at: ts,
        }
    }

    /// The joined collection query scopes to the signed-in user, filters with the
    /// shared Scryfall search over card columns, and orders by the chosen sort —
    /// exercising the whole `collection_query` path against a real (in-memory) DB.
    #[tokio::test]
    async fn collection_query_scopes_by_user_and_applies_search_and_sort() {
        use sea_orm::{ActiveModelTrait, IntoActiveModel, prelude::DateTimeUtc};

        let db = crate::test_support::migrated_memory_db().await;
        let mtg = catalog::find("mtg").expect("mtg game");
        let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

        // Two users; user 2 exists only to prove their holdings never leak into
        // user 1's list (the FK on collection_items.user_id needs the rows present).
        for uid in [1, 2] {
            crate::entities::user::ActiveModel {
                id: Set(uid),
                email: Set(format!("u{uid}@example.test")),
                password_hash: Set("x".into()),
                display_name: Set(None),
                created_at: Set(at("2024-01-01T00:00:00Z")),
                updated_at: Set(at("2024-01-01T00:00:00Z")),
            }
            .insert(&db)
            .await
            .expect("insert user");
        }

        for c in [
            seed_card(1, "Goblin Guide", "Creature — Goblin", Some("5.00")),
            seed_card(2, "Forest", "Basic Land — Forest", Some("0.10")),
            seed_card(3, "Goblin King", "Creature — Goblin", Some("2.00")),
            seed_card(4, "Goblin Piker", "Creature — Goblin", Some("1.00")),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }

        // User 1 owns cards 1..=3 (updated at increasing times so recency order is
        // 3, 2, 1); user 2 owns card 4.
        let hold = |id: i32, card_id: i32, user_id: i32, updated: &str| {
            collection_item::ActiveModel {
                id: Set(id),
                user_id: Set(user_id),
                game: Set("mtg".into()),
                card_id: Set(card_id),
                quantity: Set(1),
                foil_quantity: Set(0),
                created_at: Set(at("2024-01-01T00:00:00Z")),
                updated_at: Set(at(updated)),
            }
        };
        for h in [
            hold(1, 1, 1, "2024-01-01T00:00:00Z"),
            hold(2, 2, 1, "2024-02-01T00:00:00Z"),
            hold(3, 3, 1, "2024-03-01T00:00:00Z"),
            hold(4, 4, 2, "2024-04-01T00:00:00Z"),
        ] {
            h.insert(&db).await.expect("insert holding");
        }

        async fn names(
            db: &sea_orm::DatabaseConnection,
            set_codes: Option<&[String]>,
            search: Option<Condition>,
            sort: CollectionSort,
            dir: SortDir,
        ) -> Vec<String> {
            collection_query(1, "mtg", set_codes, search, sort, dir)
                .all(db)
                .await
                .expect("run collection query")
                .into_iter()
                .filter_map(|(_, card)| card.map(|c| c.name))
                .collect()
        }

        // Default recency (updated desc): newest holding first, user 2's card absent.
        assert_eq!(
            names(&db, None, None, CollectionSort::Recent, SortDir::Desc).await,
            ["Goblin King", "Forest", "Goblin Guide"]
        );

        // The shared Scryfall grammar runs over the joined card columns: `t:goblin`
        // keeps only user 1's two Goblins (Forest dropped; user 2's Goblin out of scope).
        let goblins = search_condition(mtg, "t:goblin").unwrap();
        assert_eq!(
            names(&db, None, Some(goblins), CollectionSort::Recent, SortDir::Desc).await,
            ["Goblin King", "Goblin Guide"]
        );

        // Price sort borrows the catalog card sort verbatim: 5.00, 2.00, 0.10.
        assert_eq!(
            names(&db, None, None, CollectionSort::Card(SortField::Price), SortDir::Desc).await,
            ["Goblin Guide", "Goblin King", "Forest"]
        );
    }

    /// The optional set scope filters the joined card's `set_code`, and it ANDs with a
    /// search over the same join — so a set-scoped, `t:goblin`-filtered list only keeps
    /// the goblins in that set.
    #[tokio::test]
    async fn collection_query_scopes_to_a_set() {
        use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

        let db = crate::test_support::migrated_memory_db().await;
        let mtg = catalog::find("mtg").expect("mtg game");
        let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

        crate::entities::user::ActiveModel {
            id: Set(1),
            email: Set("u1@example.test".into()),
            password_hash: Set("x".into()),
            display_name: Set(None),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
        }
        .insert(&db)
        .await
        .expect("insert user");

        // Three sets: "aaa" holds a Goblin + a Land; "bbb" and "ccc" each hold a Goblin.
        // The third set lets the multi-code (group-span) scope prove it *excludes* the
        // sets outside its list, not just that it returns what it's given.
        let card = |id: i32, name: &str, set_code: &str, type_line: &str| {
            let mut c = seed_card(id, name, type_line, Some("1.00"));
            c.set_code = set_code.into();
            c.set_name = set_code.to_uppercase();
            c
        };
        for c in [
            card(1, "Goblin Guide", "aaa", "Creature — Goblin"),
            card(2, "Forest", "aaa", "Basic Land — Forest"),
            card(3, "Goblin King", "bbb", "Creature — Goblin"),
            card(4, "Goblin Piker", "ccc", "Creature — Goblin"),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }
        let hold = |id: i32, card_id: i32| collection_item::ActiveModel {
            id: Set(id),
            user_id: Set(1),
            game: Set("mtg".into()),
            card_id: Set(card_id),
            quantity: Set(1),
            foil_quantity: Set(0),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
        };
        for h in [hold(1, 1), hold(2, 2), hold(3, 3), hold(4, 4)] {
            h.insert(&db).await.expect("insert holding");
        }

        async fn names(
            db: &sea_orm::DatabaseConnection,
            set_codes: Option<&[String]>,
            search: Option<Condition>,
        ) -> Vec<String> {
            let mut out: Vec<String> = collection_query(
                1,
                "mtg",
                set_codes,
                search,
                CollectionSort::Card(SortField::Name),
                SortDir::Asc,
            )
            .all(db)
            .await
            .expect("run query")
            .into_iter()
            .filter_map(|(_, c)| c.map(|c| c.name))
            .collect();
            out.sort();
            out
        }

        let aaa = ["aaa".to_string()];
        // Scoped to set "aaa": only its two cards, not the other sets' Goblins.
        assert_eq!(names(&db, Some(&aaa), None).await, ["Forest", "Goblin Guide"]);
        // Set scope ANDs with the search: goblins in "aaa" only.
        let goblins = search_condition(mtg, "t:goblin").unwrap();
        assert_eq!(names(&db, Some(&aaa), Some(goblins)).await, ["Goblin Guide"]);
        // A multi-code scope (the include-related group view) spans exactly its sets —
        // "aaa" + "bbb", excluding "ccc"'s Goblin Piker.
        let group = ["aaa".to_string(), "bbb".to_string()];
        assert_eq!(
            names(&db, Some(&group), None).await,
            ["Forest", "Goblin Guide", "Goblin King"]
        );
        // No scope: every set's holdings, including "ccc".
        assert_eq!(
            names(&db, None, None).await,
            ["Forest", "Goblin Guide", "Goblin King", "Goblin Piker"]
        );
    }

    /// The drops handler's core: owned cards in a drop-grouped set, joined + ordered by
    /// `collection_query`, group into their Secret Lair drops with their owned counts
    /// intact — and a drop the user owns nothing in never appears.
    #[tokio::test]
    async fn owned_cards_group_into_drops_with_counts() {
        use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

        let db = crate::test_support::migrated_memory_db().await;
        let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

        crate::entities::user::ActiveModel {
            id: Set(1),
            email: Set("d@example.test".into()),
            password_hash: Set("x".into()),
            display_name: Set(None),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
        }
        .insert(&db)
        .await
        .expect("insert user");

        // sld cards at known Secret Lair collector numbers: 2658 -> "Wild in Bloom",
        // 168 -> "Inked", and one number not in the snapshot (which would fall into the
        // trailing "Other" group — but only if owned).
        let sld_card = |id: i32, cn: &str, cn_int: Option<i32>| {
            let mut c = seed_card(id, &format!("SLD {cn}"), "Creature", Some("1.00"));
            c.set_code = "sld".into();
            c.set_name = "Secret Lair Drop".into();
            c.collector_number = cn.into();
            c.collector_number_int = cn_int;
            c
        };
        for c in [
            sld_card(1, "2658", Some(2658)),
            sld_card(2, "168", Some(168)),
            sld_card(3, "999999", Some(999999)),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }
        // Own the first two (2 + 1 foil of #2658; 3 of #168); leave #999999 unowned.
        let hold = |id: i32, card_id: i32, q: i32, f: i32| collection_item::ActiveModel {
            id: Set(id),
            user_id: Set(1),
            game: Set("mtg".into()),
            card_id: Set(card_id),
            quantity: Set(q),
            foil_quantity: Set(f),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
        };
        for h in [hold(1, 1, 2, 1), hold(2, 2, 3, 0)] {
            h.insert(&db).await.expect("insert holding");
        }

        // The same query + grouping pass the drops handler runs.
        let scope = ["sld".to_string()];
        let rows = collection_query(
            1,
            "mtg",
            Some(&scope),
            None,
            CollectionSort::Card(SortField::Number),
            SortDir::Asc,
        )
        .all(&db)
        .await
        .expect("run query");
        let pairs: Vec<(collection_item::Model, card::Model)> = rows
            .into_iter()
            .filter_map(|(item, card)| card.map(|c| (item, c)))
            .collect();
        let table = crate::scryfall::drops::table("mtg", "sld").expect("sld drop table");
        let buckets = group_into_drops(table, pairs, |(_, card)| card.collector_number.as_str());

        // Only the two owned drops appear (the unowned #999999 yields no "Other" group),
        // in Scryfall's drop order (Wild in Bloom before Inked), each carrying its owned
        // holding.
        let titles: Vec<&str> = buckets.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Wild in Bloom", "Inked"]);
        assert_eq!(buckets[0].cards.len(), 1);
        assert_eq!(buckets[0].cards[0].0.quantity, 2);
        assert_eq!(buckets[0].cards[0].0.foil_quantity, 1);
        assert_eq!(buckets[1].cards[0].0.quantity, 3);
    }

    /// `build_collection_sets` counts distinct owned cards + total copies per set,
    /// dresses each with its `card_sets` metadata (falling back to the card's own set
    /// name when the row is missing), orders newest set first (undated last), and skips
    /// holdings whose card row is gone.
    #[test]
    fn build_collection_sets_aggregates_dresses_and_orders() {
        let ts = "2024-01-01T00:00:00Z"
            .parse::<sea_orm::prelude::DateTimeUtc>()
            .unwrap();
        let hold = |id: i32, card_id: i32, quantity: i32, foil_quantity: i32| collection_item::Model {
            id,
            user_id: 1,
            game: "mtg".into(),
            card_id,
            quantity,
            foil_quantity,
            created_at: ts,
            updated_at: ts,
        };
        let carded = |id: i32, set_code: &str, set_name: &str, usd: Option<&str>, foil: Option<&str>| {
            let mut c = seed_card(id, "Card", "Creature", usd);
            c.set_code = set_code.into();
            c.set_name = set_name.into();
            c.price_usd_foil = foil.map(str::to_string);
            c
        };
        let set_meta = |code: &str, name: &str, released: &str| card_set::Model {
            id: 0,
            game: "mtg".into(),
            code: code.into(),
            name: name.into(),
            set_type: Some("expansion".into()),
            released_at: Some(released.into()),
            card_count: 100,
            digital: false,
            icon_svg_uri: Some(format!("https://example.test/{code}.svg")),
            parent_set_code: None,
            external_id: None,
            created_at: ts,
            updated_at: ts,
        };

        let rows = vec![
            // Set "aaa": two distinct cards, 3 total copies (2 + 1 foil, then 1 + 0).
            // Value: 2×$1.00 + 1×$5.00 foil + 1×$2.00 = $9.00.
            (
                hold(1, 1, 2, 1),
                Some(carded(1, "aaa", "Older Set", Some("1.00"), Some("5.00"))),
            ),
            (
                hold(2, 2, 1, 0),
                Some(carded(2, "aaa", "Older Set", Some("2.00"), None)),
            ),
            // Set "bbb": one card, 4 copies — no card_sets metadata (fallback name) and
            // unpriced, so its value is `None` rather than $0.00.
            (
                hold(3, 3, 4, 0),
                Some(carded(3, "bbb", "Newer Set", None, None)),
            ),
            // A holding whose card row is gone — skipped entirely.
            (hold(4, 4, 9, 9), None),
        ];
        // Only "aaa" has metadata; "bbb" must fall back to the card's set_name.
        let sets = vec![set_meta("aaa", "Alpha", "2000-01-01")];

        let out = build_collection_sets("mtg", rows, sets);
        assert_eq!(out.len(), 2);

        // "bbb" (dated? no metadata -> released_at None) sorts after the dated "aaa".
        assert_eq!(out[0].code, "aaa");
        assert_eq!(out[0].name, "Alpha"); // dressed from card_sets, not the card
        assert_eq!(out[0].released_at.as_deref(), Some("2000-01-01"));
        assert_eq!(out[0].owned_cards, 2);
        assert_eq!(out[0].owned_copies, 4); // (2+1) + (1+0)
        assert_eq!(out[0].owned_value_usd.as_deref(), Some("9.00")); // priced holdings summed

        assert_eq!(out[1].code, "bbb");
        assert_eq!(out[1].name, "Newer Set"); // fallback to the card's set_name
        assert_eq!(out[1].released_at, None);
        assert_eq!(out[1].card_count, 0);
        assert_eq!(out[1].owned_cards, 1);
        assert_eq!(out[1].owned_copies, 4);
        assert_eq!(out[1].owned_value_usd, None); // nothing priced -> null, not $0.00
    }
}
