//! Public, game-agnostic card-catalog endpoints.
//!
//! All routes are unauthenticated reads of card data, namespaced by `game`
//! (`/api/games/{game}/...`) so every supported TCG shares one URL shape and one
//! set of handlers. The image route is a lazy caching proxy (see
//! [`crate::catalog::images`]).

use axum::{
    Json,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, Order, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Select,
    prelude::DateTimeUtc,
    sea_query::{Expr, LikeExpr, NullOrdering, SimpleExpr},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::catalog::{self, Game};
use crate::entities::prelude::{Card, CardPriceHistory, CardSet, IngestState};
use crate::entities::{card, card_price_history, card_set, ingest_state};
use crate::error::AppError;
use crate::scryfall::model::StoredFace;
use crate::scryfall::search::escape_like;
use crate::state::AppState;

const DEFAULT_PAGE_SIZE: u64 = 60;
const MAX_PAGE_SIZE: u64 = 200;
/// Card art for a given id is immutable, so it is safe to cache aggressively.
const IMAGE_CACHE_CONTROL: &str = "public, max-age=2592000, immutable";

// ---------- Response DTOs ----------

#[derive(Debug, Serialize)]
pub struct SetResponse {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i32,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
}

impl From<card_set::Model> for SetResponse {
    fn from(m: card_set::Model) -> Self {
        SetResponse {
            code: m.code,
            name: m.name,
            set_type: m.set_type,
            released_at: m.released_at,
            card_count: m.card_count,
            icon_svg_uri: m.icon_svg_uri,
            parent_set_code: m.parent_set_code,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PricesResponse {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

/// One day's price snapshot in a card's price-over-time series. Prices are the
/// decimal strings exactly as stored (mirroring [`PricesResponse`]); `date` is a
/// `"YYYY-MM-DD"` string.
#[derive(Debug, Serialize)]
pub struct PricePoint {
    pub date: String,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

impl From<card_price_history::Model> for PricePoint {
    fn from(m: card_price_history::Model) -> Self {
        PricePoint {
            date: m.as_of_date,
            usd: m.price_usd,
            usd_foil: m.price_usd_foil,
            eur: m.price_eur,
            tix: m.price_tix,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CardFaceResponse {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CardResponse {
    pub id: String,
    pub name: String,
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: Option<String>,
    pub lang: String,
    pub released_at: Option<String>,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub color_identity: Vec<String>,
    pub colors: Vec<String>,
    pub layout: Option<String>,
    pub prices: PricesResponse,
    /// Whether an image is available through the image proxy for this card.
    pub has_image: bool,
    /// Present for multi-faced cards; request face images via `?face=N`.
    pub faces: Vec<CardFaceResponse>,
}

impl From<card::Model> for CardResponse {
    fn from(m: card::Model) -> Self {
        let stored_faces: Vec<StoredFace> = m
            .card_faces
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        let has_image = m.image_normal.is_some()
            || m.image_small.is_some()
            || m.image_large.is_some()
            || stored_faces
                .iter()
                .any(|f| f.image_normal.is_some() || f.image_small.is_some());

        let faces = stored_faces
            .into_iter()
            .map(|f| CardFaceResponse {
                name: f.name,
                mana_cost: f.mana_cost,
                type_line: f.type_line,
                oracle_text: f.oracle_text,
                power: f.power,
                toughness: f.toughness,
                loyalty: f.loyalty,
            })
            .collect();

        CardResponse {
            id: m.external_id,
            name: m.name,
            set_code: m.set_code,
            set_name: m.set_name,
            collector_number: m.collector_number,
            rarity: m.rarity,
            lang: m.lang,
            released_at: m.released_at,
            mana_cost: m.mana_cost,
            cmc: m.cmc,
            type_line: m.type_line,
            oracle_text: m.oracle_text,
            power: m.power,
            toughness: m.toughness,
            loyalty: m.loyalty,
            color_identity: split_csv(m.color_identity),
            colors: split_csv(m.colors),
            layout: m.layout,
            prices: PricesResponse {
                usd: m.price_usd,
                usd_foil: m.price_usd_foil,
                eur: m.price_eur,
                tix: m.price_tix,
            },
            has_image,
            faces,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub detail: Option<String>,
    pub sets_imported: i32,
    pub cards_imported: i32,
    pub source_updated_at: Option<String>,
    pub finished_at: Option<DateTimeUtc>,
}

/// A page of results plus the cursor metadata the SPA needs to paginate.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
}

// ---------- Query params ----------

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    #[serde(default)]
    pub q: Option<String>,
    /// Set-cards only: when `true`, span the set's whole group (its top-level
    /// root plus every related sub-set) instead of just the one set. Ignored by
    /// the all-cards endpoint.
    #[serde(default)]
    pub include_related: Option<bool>,
    /// Sort key (`number`/`name`/`rarity`/`released`/`cmc`/`price`). Absent =
    /// the endpoint's natural default. Unknown values are a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`). Absent = the sort field's natural
    /// direction. Unknown values are a 422.
    #[serde(default)]
    pub dir: Option<String>,
}

impl ListParams {
    fn page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        (page, page_size)
    }

    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    /// Resolve the `sort`/`dir` params into a validated `(field, direction)`,
    /// falling back to `default` (and the field's natural direction) when a value
    /// is absent. An unrecognised value is a 422 — consistent with a malformed `q`
    /// — rather than being silently ignored.
    fn sort_spec(&self, default: SortField) -> Result<(SortField, SortDir), AppError> {
        let field = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => default,
            Some(value) => SortField::parse(value)?,
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => field.default_dir(),
            Some(value) => SortDir::parse(value)?,
        };
        Ok((field, dir))
    }
}

/// A user-facing card-list sort key. Maps to one or more `card` columns; fields
/// that aren't lexically ordered (rarity, price) sort on a derived expression so
/// the order is meaningful rather than alphabetical/string-wise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortField {
    /// Collector number (numeric run first, then the raw string) — a set
    /// listing's natural order.
    Number,
    Name,
    Rarity,
    Released,
    /// Mana value (converted mana cost).
    Cmc,
    /// USD market price.
    Price,
}

impl SortField {
    fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "number" | "collector" => SortField::Number,
            "name" => SortField::Name,
            "rarity" => SortField::Rarity,
            "released" | "date" => SortField::Released,
            "cmc" | "mv" => SortField::Cmc,
            "price" | "usd" => SortField::Price,
            other => return Err(AppError::Validation(format!("unknown sort '{other}'"))),
        })
    }

    /// The direction to use when a caller names a field but no `dir`. Newest,
    /// priciest and rarest first read more usefully than the lexical-ascending
    /// default for those fields.
    fn default_dir(self) -> SortDir {
        match self {
            SortField::Number | SortField::Name | SortField::Cmc => SortDir::Asc,
            SortField::Rarity | SortField::Released | SortField::Price => SortDir::Desc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            other => Err(AppError::Validation(format!(
                "unknown sort direction '{other}'"
            ))),
        }
    }

    fn order(self) -> Order {
        match self {
            SortDir::Asc => Order::Asc,
            SortDir::Desc => Order::Desc,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ImageParams {
    pub size: Option<String>,
    pub face: Option<usize>,
}

// ---------- Handlers ----------

/// `GET /api/games` -> the list of supported games.
pub async fn list_games() -> Json<serde_json::Value> {
    Json(json!({ "data": catalog::GAMES }))
}

/// `GET /api/games/{game}/status` -> the card-data import status for a game.
pub async fn ingest_status(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<StatusResponse>, AppError> {
    require_game(&game)?;
    let row = IngestState::find()
        .filter(ingest_state::Column::Game.eq(game.as_str()))
        .one(&state.db)
        .await?;
    Ok(Json(match row {
        Some(r) => StatusResponse {
            status: r.status,
            detail: r.detail,
            sets_imported: r.sets_imported,
            cards_imported: r.cards_imported,
            source_updated_at: r.source_updated_at,
            finished_at: r.finished_at,
        },
        None => StatusResponse {
            status: "idle".to_string(),
            detail: None,
            sets_imported: 0,
            cards_imported: 0,
            source_updated_at: None,
            finished_at: None,
        },
    }))
}

/// `GET /api/games/{game}/sets` -> every stored set, newest first.
pub async fn list_sets(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let sets = CardSet::find()
        .filter(card_set::Column::Game.eq(game.as_str()))
        .order_by_desc(card_set::Column::ReleasedAt)
        .order_by_asc(card_set::Column::Name)
        .all(&state.db)
        .await?;
    let data: Vec<SetResponse> = sets.into_iter().map(SetResponse::from).collect();
    Ok(Json(json!({ "data": data })))
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
        // Resolve the group membership from the flat set list (one cheap query),
        // mirroring the frontend grouping so both span exactly the same sets.
        let all_sets = CardSet::find()
            .filter(card_set::Column::Game.eq(game.as_str()))
            .all(&state.db)
            .await?;
        let codes = group_set_codes(&all_sets, &set.code);
        query.filter(card::Column::SetCode.is_in(codes))
    } else {
        query.filter(card::Column::SetCode.eq(set.code.as_str()))
    };
    if let Some(search) = params.search() {
        query = query.filter(search_condition(game_meta, search)?);
    }

    let (sort, dir) = params.sort_spec(SortField::Number)?;
    // For the default collector-number order, the related-sets view keeps each
    // set's cards contiguous (set code first, which spans whole sets unlike the
    // per-card released_at). Any other sort spans the whole group by the chosen
    // field instead — grouping by set there would fight the sort.
    let group_by_set = include_related && sort == SortField::Number;
    let paginator = apply_card_sort(query, sort, dir, group_by_set).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    Ok(Json(build_page(rows, page, page_size, total)))
}

/// `GET /api/games/{game}/cards` -> all cards (optional `q` search), by name.
pub async fn list_cards(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let mut query = Card::find().filter(card::Column::Game.eq(game.as_str()));
    if let Some(search) = params.search() {
        query = query.filter(search_condition(game_meta, search)?);
    }
    let (sort, dir) = params.sort_spec(SortField::Name)?;
    let paginator = apply_card_sort(query, sort, dir, false).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    Ok(Json(build_page(rows, page, page_size, total)))
}

/// `GET /api/games/{game}/cards/{id}` -> one card's full detail.
pub async fn get_card(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CardResponse>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    Ok(Json(CardResponse::from(card)))
}

/// `GET /api/games/{game}/cards/{id}/prices` -> a card's daily price history,
/// oldest first, for charting. `404` if the game or card id is unknown; an empty
/// `{ "data": [] }` when the card exists but has no captured history yet.
pub async fn card_prices(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let rows = CardPriceHistory::find()
        .filter(card_price_history::Column::Game.eq(game.as_str()))
        .filter(card_price_history::Column::CardId.eq(card.id))
        .order_by_asc(card_price_history::Column::AsOfDate)
        .all(&state.db)
        .await?;
    let data: Vec<PricePoint> = rows.into_iter().map(PricePoint::from).collect();
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/games/{game}/cards/{id}/prints` -> this card's **other** printings:
/// every card sharing its gameplay identity (Scryfall `oracle_id`) in the same
/// game, excluding the card itself, newest printing first (capped at `MAX_PAGE_SIZE`).
/// `404` if the game or card id is unknown; an empty `{ "data": [] }` when the card
/// has no other printings (or carries no `oracle_id`, so its siblings can't be
/// identified — e.g. reversible cards, whose `oracle_id` lives only per-face).
pub async fn card_prints(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    // Without an oracle_id there's no key to find sibling printings by, so the
    // card has no listable other printings.
    let data: Vec<CardResponse> = match card.oracle_id.as_deref().filter(|s| !s.is_empty()) {
        None => Vec::new(),
        Some(oracle_id) => prints_query(&game, oracle_id, card.id)
            .all(&state.db)
            .await?
            .into_iter()
            .map(CardResponse::from)
            .collect(),
    };
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/games/{game}/cards/{id}/image?size=normal&face=0`
///
/// Streams the cached image, downloading + persisting it on first request.
pub async fn card_image(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ImageParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let size = normalize_size(params.size.as_deref());

    // Resolve the upstream URL (and a stable cache key) for the requested face.
    let (source_url, cache_key) = match params.face {
        Some(idx) => {
            let face = stored_faces(&card)
                .into_iter()
                .nth(idx)
                .ok_or_else(|| AppError::NotFound(format!("card '{id}' has no face {idx}")))?;
            let url = face_image_url(&face, size)
                .ok_or_else(|| AppError::NotFound("no image for that face/size".to_string()))?;
            (url, format!("{id}-f{idx}"))
        }
        None => {
            let url = card_image_url(&card, size)
                .or_else(|| {
                    // Multi-faced cards have no top-level image; use the front face.
                    stored_faces(&card)
                        .into_iter()
                        .next()
                        .and_then(|f| face_image_url(&f, size))
                })
                .ok_or_else(|| AppError::NotFound("no image for that card/size".to_string()))?;
            (url, id.clone())
        }
    };

    // Defense-in-depth: only ever fetch from the provider CDN, so a bad stored URL
    // can't turn this public proxy into an SSRF. All images are Scryfall today.
    if !is_allowed_image_url(&source_url) {
        tracing::warn!(card = %id, url = %source_url, "refusing to proxy non-allowlisted image URL");
        return Err(AppError::NotFound("no image available".to_string()));
    }

    let image = state
        .images
        .get(&game, size, &cache_key, &source_url)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, card = %id, "failed to cache card image");
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

// ---------- Helpers ----------

fn require_game(game: &str) -> Result<&'static Game, AppError> {
    catalog::find(game).ok_or_else(|| AppError::NotFound(format!("unknown game '{game}'")))
}

async fn load_set(state: &AppState, game: &str, code: &str) -> Result<card_set::Model, AppError> {
    let code = code.to_lowercase();
    CardSet::find()
        .filter(card_set::Column::Game.eq(game))
        .filter(card_set::Column::Code.eq(code.as_str()))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("set '{code}' not found")))
}

/// Resolve every set code in `code`'s group: its top-level root plus all
/// descendants. Mirrors the frontend `groupSets` resolution (walk `parent_set_code`
/// up to a root, guarding missing parents and — defensively — cycles) so the
/// "include related sets" view spans exactly the sets nested under one main set.
/// Falls back to `[code]` if the set somehow isn't in the list.
fn group_set_codes(all_sets: &[card_set::Model], code: &str) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    let by_code: HashMap<&str, &card_set::Model> =
        all_sets.iter().map(|s| (s.code.as_str(), s)).collect();

    let root_of = |start: &str| -> String {
        let mut current = start;
        let mut seen = HashSet::new();
        while let Some(set) = by_code.get(current) {
            let Some(parent) = set.parent_set_code.as_deref() else {
                break;
            };
            // Stop at an orphan (parent not in the catalogue) or a cycle.
            if !by_code.contains_key(parent) || !seen.insert(current) {
                break;
            }
            current = parent;
        }
        current.to_string()
    };

    let root = root_of(code);
    let codes: Vec<String> = all_sets
        .iter()
        .filter(|s| root_of(&s.code) == root)
        .map(|s| s.code.clone())
        .collect();
    if codes.is_empty() {
        vec![code.to_string()]
    } else {
        codes
    }
}

async fn load_card(state: &AppState, game: &str, id: &str) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{id}' not found")))
}

/// Query a card's **other** printings: same game and `oracle_id`, excluding the
/// card itself (`exclude_id`). Ordered newest printing first (released date desc,
/// nulls last), then set code and collector number, with a stable `id` tiebreaker
/// so the order is deterministic. `oracle_id` is the gameplay-identity key shared
/// across all printings of a card.
///
/// Capped at `MAX_PAGE_SIZE` results: a handful of cards (e.g. basic lands) share
/// one `oracle_id` across hundreds-to-thousands of printings, and this is a "see
/// also" aid rather than an exhaustive listing, so it returns at most the newest
/// `MAX_PAGE_SIZE` rather than an unbounded response.
fn prints_query(game: &str, oracle_id: &str, exclude_id: i32) -> Select<card::Entity> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::OracleId.eq(oracle_id))
        .filter(card::Column::Id.ne(exclude_id))
        .order_by_with_nulls(card::Column::ReleasedAt, Order::Desc, NullOrdering::Last)
        .order_by_asc(card::Column::SetCode)
        .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last)
        .order_by_asc(card::Column::CollectorNumber)
        .order_by_asc(card::Column::Id)
        .limit(MAX_PAGE_SIZE)
}

fn build_page(rows: Vec<card::Model>, page: u64, page_size: u64, total: u64) -> Page<CardResponse> {
    let data: Vec<CardResponse> = rows.into_iter().map(CardResponse::from).collect();
    Page {
        data,
        page,
        page_size,
        total,
        has_more: page.saturating_mul(page_size) < total,
    }
}

/// Apply the requested ordering to a card query, ending with a stable `id`
/// tiebreaker so pagination is deterministic across pages even when the chosen
/// field has ties. `group_by_set` keeps each set's cards contiguous (used by the
/// related-sets view in collector-number order); it only makes sense alongside
/// the `Number` field, where a per-set grouping is wanted instead of a single
/// flat run. Rarity and price sort on a derived expression with unknown/missing
/// values pushed last regardless of direction.
fn apply_card_sort(
    query: Select<card::Entity>,
    field: SortField,
    dir: SortDir,
    group_by_set: bool,
) -> Select<card::Entity> {
    let mut query = if group_by_set {
        query.order_by_asc(card::Column::SetCode)
    } else {
        query
    };
    query = match field {
        SortField::Number => query
            .order_by_with_nulls(card::Column::CollectorNumberInt, dir.order(), NullOrdering::Last)
            .order_by(card::Column::CollectorNumber, dir.order()),
        // Preserve the previous all-cards tiebreak (set, then collector number)
        // so the default listing order is unchanged.
        SortField::Name => query
            .order_by(card::Column::Name, dir.order())
            .order_by_asc(card::Column::SetCode)
            .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last),
        SortField::Rarity => query
            .order_by_with_nulls(rarity_rank_expr(), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Released => query
            .order_by_with_nulls(card::Column::ReleasedAt, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Cmc => query
            .order_by_with_nulls(card::Column::Cmc, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        // Fall back to the foil price when there's no regular USD price, so
        // foil-only printings sort by what they actually cost instead of being
        // parked as unpriced (matches the browse tiles' displayed price).
        SortField::Price => query
            .order_by_with_nulls(
                price_real_expr(&["price_usd", "price_usd_foil"]),
                dir.order(),
                NullOrdering::Last,
            )
            .order_by_asc(card::Column::Name),
    };
    query.order_by_asc(card::Column::Id)
}

/// SQL expression mapping `rarity` to its canonical low→high ordinal, reusing the
/// search grammar's rarity ranking (`scryfall::search::RARITIES`) so the sort and
/// the `r>=`/`r<` filters stay in lockstep. Unknown/missing rarities map to NULL
/// so `NULLS LAST` parks them at the end in either direction. The interpolated
/// values are fixed lowercase rarity names and integer ranks — never user input.
fn rarity_rank_expr() -> SimpleExpr {
    let arms: String = crate::scryfall::search::RARITIES
        .iter()
        .enumerate()
        .map(|(rank, name)| format!("WHEN '{name}' THEN {rank}"))
        .collect::<Vec<_>>()
        .join(" ");
    Expr::cust(format!("CASE IFNULL(rarity, '') {arms} ELSE NULL END"))
}

/// SQL expression giving a card's sort price: the first non-empty value among
/// `cols` (in order), cast to a real number. Each column is NULL/empty-guarded
/// (so `''` isn't treated as `0`) and the guarded values are `COALESCE`d, so a
/// card priced only in a later column — e.g. a foil-only printing with no regular
/// `price_usd` — still sorts by that price rather than being treated as unpriced.
/// When every column is NULL/empty the result is NULL, so `NULLS LAST` keeps
/// truly-unpriced cards at the end in either direction. `cols` are fixed column
/// names, never user input.
fn price_real_expr(cols: &[&str]) -> SimpleExpr {
    let arms: Vec<String> = cols
        .iter()
        .map(|col| {
            format!("CASE WHEN {col} IS NULL OR {col} = '' THEN NULL ELSE CAST({col} AS REAL) END")
        })
        .collect();
    // SQLite's COALESCE needs ≥2 arguments; a single column is emitted bare.
    let expr = match arms.as_slice() {
        [single] => single.clone(),
        _ => format!("COALESCE({})", arms.join(", ")),
    };
    Expr::cust(expr)
}

fn stored_faces(card: &card::Model) -> Vec<StoredFace> {
    card.card_faces
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Build the `q` search filter, dispatching to the game's query syntax. MTG
/// (Scryfall) gets the full Scryfall-style grammar (see [`crate::scryfall::search`]);
/// any other game falls back to a plain card-name substring match. A malformed
/// Scryfall query becomes an `AppError::Validation` (HTTP 422).
fn search_condition(game: &Game, search: &str) -> Result<Condition, AppError> {
    match game.id {
        crate::scryfall::GAME => Ok(crate::scryfall::search::parse(search)?),
        _ => Ok(Condition::all().add(name_like(search))),
    }
}

/// A `name LIKE %term%` filter for the fallback (non-Scryfall) game search, with
/// LIKE metacharacters in `search` escaped so they match literally (paired with an
/// explicit `ESCAPE '\'`).
fn name_like(search: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(search));
    Expr::col((card::Entity, card::Column::Name)).like(LikeExpr::new(pattern).escape('\\'))
}

/// Whether the image proxy is allowed to fetch a URL: HTTPS on a provider CDN.
/// Stored image URLs all come from Scryfall ingestion; this guards against a bad
/// value ever turning the proxy into an SSRF.
fn is_allowed_image_url(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(parsed) => {
            parsed.scheme() == "https"
                && parsed
                    .host_str()
                    .is_some_and(|host| host == "scryfall.io" || host.ends_with(".scryfall.io"))
        }
        Err(_) => false,
    }
}

/// Map a requested image size to a stored, allow-listed size (default `normal`).
fn normalize_size(requested: Option<&str>) -> &'static str {
    match requested {
        Some("small") => "small",
        Some("large") => "large",
        Some("png") => "png",
        Some("art_crop") => "art_crop",
        _ => "normal",
    }
}

fn card_image_url(card: &card::Model, size: &str) -> Option<String> {
    match size {
        "small" => card.image_small.clone(),
        "large" => card.image_large.clone(),
        "png" => card.image_png.clone(),
        "art_crop" => card.image_art_crop.clone(),
        _ => card.image_normal.clone(),
    }
}

fn face_image_url(face: &StoredFace, size: &str) -> Option<String> {
    match size {
        "small" => face.image_small.clone(),
        "large" => face.image_large.clone(),
        "png" => face.image_png.clone(),
        "art_crop" => face.image_art_crop.clone(),
        _ => face.image_normal.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_size_allowlists() {
        assert_eq!(normalize_size(Some("png")), "png");
        assert_eq!(normalize_size(Some("art_crop")), "art_crop");
        assert_eq!(normalize_size(Some("../secret")), "normal");
        assert_eq!(normalize_size(None), "normal");
    }

    #[test]
    fn price_point_maps_history_row() {
        let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        let m = card_price_history::Model {
            id: 1,
            game: "mtg".into(),
            card_id: 5,
            as_of_date: "2026-06-30".into(),
            price_usd: Some("1.23".into()),
            price_usd_foil: None,
            price_eur: Some("1.00".into()),
            price_tix: None,
            created_at: ts,
        };
        let p = PricePoint::from(m);
        assert_eq!(p.date, "2026-06-30");
        assert_eq!(p.usd.as_deref(), Some("1.23"));
        assert_eq!(p.usd_foil, None);
        assert_eq!(p.eur.as_deref(), Some("1.00"));
        assert_eq!(p.tix, None);
    }

    #[test]
    fn split_csv_handles_empty() {
        assert_eq!(split_csv(None), Vec::<String>::new());
        assert_eq!(split_csv(Some(String::new())), Vec::<String>::new());
        assert_eq!(split_csv(Some("W,U".to_string())), vec!["W", "U"]);
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("Sol Ring"), "Sol Ring");
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("x\\y"), "x\\\\y");
    }

    #[test]
    fn image_url_allowlist() {
        assert!(is_allowed_image_url(
            "https://cards.scryfall.io/normal/front/0/0/x.jpg"
        ));
        assert!(is_allowed_image_url("https://scryfall.io/x.png"));
        assert!(!is_allowed_image_url("http://cards.scryfall.io/x.jpg")); // not https
        assert!(!is_allowed_image_url("https://evil.example.com/x.jpg")); // wrong host
        assert!(!is_allowed_image_url("https://scryfall.io.evil.com/x.jpg")); // suffix trick
        assert!(!is_allowed_image_url("not a url"));
    }

    fn params(sort: Option<&str>, dir: Option<&str>) -> ListParams {
        ListParams {
            page: None,
            page_size: None,
            q: None,
            include_related: None,
            sort: sort.map(str::to_string),
            dir: dir.map(str::to_string),
        }
    }

    #[test]
    fn list_params_clamps_page_size() {
        let p = ListParams {
            page: Some(0),
            page_size: Some(9999),
            q: None,
            include_related: None,
            sort: None,
            dir: None,
        };
        assert_eq!(p.page_and_size(), (1, MAX_PAGE_SIZE));
        let d = ListParams {
            page: None,
            page_size: None,
            q: Some("  ".into()),
            include_related: None,
            sort: None,
            dir: None,
        };
        assert_eq!(d.page_and_size(), (1, DEFAULT_PAGE_SIZE));
        assert_eq!(d.search(), None);
    }

    #[test]
    fn sort_spec_uses_endpoint_default_when_absent() {
        assert_eq!(
            params(None, None).sort_spec(SortField::Number).unwrap(),
            (SortField::Number, SortDir::Asc),
        );
        assert_eq!(
            params(None, None).sort_spec(SortField::Name).unwrap(),
            (SortField::Name, SortDir::Asc),
        );
        // Blank values are treated as absent (fall back to the default).
        assert_eq!(
            params(Some("  "), Some("")).sort_spec(SortField::Name).unwrap(),
            (SortField::Name, SortDir::Asc),
        );
    }

    #[test]
    fn sort_spec_field_picks_natural_direction() {
        // A field with no explicit dir defaults to its natural direction.
        assert_eq!(
            params(Some("price"), None).sort_spec(SortField::Name).unwrap(),
            (SortField::Price, SortDir::Desc),
        );
        assert_eq!(
            params(Some("released"), None).sort_spec(SortField::Name).unwrap(),
            (SortField::Released, SortDir::Desc),
        );
        assert_eq!(
            params(Some("cmc"), None).sort_spec(SortField::Name).unwrap(),
            (SortField::Cmc, SortDir::Asc),
        );
        // An explicit dir overrides the natural one; aliases resolve.
        assert_eq!(
            params(Some("collector"), Some("desc")).sort_spec(SortField::Name).unwrap(),
            (SortField::Number, SortDir::Desc),
        );
        assert_eq!(
            params(Some("mv"), Some("asc")).sort_spec(SortField::Name).unwrap(),
            (SortField::Cmc, SortDir::Asc),
        );
    }

    #[test]
    fn sort_spec_rejects_unknown_values() {
        assert!(matches!(
            params(Some("color"), None).sort_spec(SortField::Name),
            Err(AppError::Validation(_)),
        ));
        assert!(matches!(
            params(None, Some("sideways")).sort_spec(SortField::Name),
            Err(AppError::Validation(_)),
        ));
    }

    /// A minimal, insertable card row whose only meaningful fields for the sort
    /// tests are its id and the two USD price columns.
    fn test_card(id: i32, usd: Option<&str>, usd_foil: Option<&str>) -> card::Model {
        let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        card::Model {
            id,
            game: "mtg".into(),
            external_id: format!("ext-{id}"),
            oracle_id: None,
            name: format!("Card {id}"),
            set_code: "tst".into(),
            set_name: "TST".into(),
            collector_number: id.to_string(),
            collector_number_int: Some(id),
            rarity: None,
            lang: "en".into(),
            released_at: None,
            mana_cost: None,
            cmc: None,
            type_line: None,
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
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            price_eur: None,
            price_tix: None,
            digital: false,
            created_at: ts,
            updated_at: ts,
        }
    }

    /// The price sort falls back to the foil price when a card has no regular USD
    /// price (some printings are foil-only) but still prefers the regular price
    /// when present, with unpriced cards parked last in either direction.
    #[tokio::test]
    async fn price_sort_falls_back_to_foil_when_no_regular_usd() {
        use sea_orm::{ActiveModelTrait, Database, IntoActiveModel};
        use sea_orm_migration::MigratorTrait;

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        crate::migrator::Migrator::up(&db, None)
            .await
            .expect("run migrations");

        // 1: regular $5 (also a $50 foil — the regular price must win over it).
        // 2: foil-only $20.  3: foil-only $1.  4: fully unpriced.
        // 5: empty-string regular price + $8 foil — the `= ''` guard must treat the
        //    empty regular price as absent and fall through to the foil price.
        for c in [
            test_card(1, Some("5.00"), Some("50.00")),
            test_card(2, None, Some("20.00")),
            test_card(3, None, Some("1.00")),
            test_card(4, None, None),
            test_card(5, Some(""), Some("8.00")),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }

        let ids = |rows: Vec<card::Model>| rows.iter().map(|r| r.id).collect::<Vec<_>>();

        // Effective price = regular USD (when non-empty), else foil. Desc: 2($20),
        // 5($8 via foil), 1($5), 3($1), then the unpriced 4 last. Card 1 sorts on
        // its $5 regular price, not its $50 foil — the fallback only applies when
        // the regular price is missing or empty.
        let desc = apply_card_sort(card::Entity::find(), SortField::Price, SortDir::Desc, false)
            .all(&db)
            .await
            .expect("query desc");
        assert_eq!(ids(desc), vec![2, 5, 1, 3, 4]);

        // Asc mirrors it, with the unpriced card still parked last (NULLS LAST).
        let asc = apply_card_sort(card::Entity::find(), SortField::Price, SortDir::Asc, false)
            .all(&db)
            .await
            .expect("query asc");
        assert_eq!(ids(asc), vec![3, 1, 5, 2, 4]);
    }

    /// A card row whose printing-identity fields (oracle id, set, release date)
    /// are what `prints_query` filters and orders on; everything else reuses the
    /// minimal `test_card` shape.
    fn print_card(id: i32, oracle_id: Option<&str>, set_code: &str, released: Option<&str>) -> card::Model {
        card::Model {
            oracle_id: oracle_id.map(str::to_string),
            set_code: set_code.into(),
            set_name: set_code.to_uppercase(),
            released_at: released.map(str::to_string),
            ..test_card(id, None, None)
        }
    }

    /// `prints_query` returns every other printing sharing the card's oracle id,
    /// newest released first, and excludes the card itself, other oracle ids, and
    /// cards with no oracle id.
    #[tokio::test]
    async fn prints_query_returns_other_printings_newest_first() {
        use sea_orm::{ActiveModelTrait, Database, IntoActiveModel};
        use sea_orm_migration::MigratorTrait;

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        crate::migrator::Migrator::up(&db, None)
            .await
            .expect("run migrations");

        // Three printings of oracle "o-1" across three sets/dates, one unrelated
        // oracle, and one card with no oracle id at all.
        for c in [
            print_card(1, Some("o-1"), "aaa", Some("2020-01-01")),
            print_card(2, Some("o-1"), "bbb", Some("2022-01-01")),
            print_card(3, Some("o-1"), "ccc", Some("2024-01-01")),
            print_card(4, Some("o-2"), "ddd", Some("2024-06-01")),
            print_card(5, None, "eee", Some("2024-06-01")),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }

        let ids = |rows: Vec<card::Model>| rows.iter().map(|r| r.id).collect::<Vec<_>>();

        // Other "o-1" printings of card 2, newest first: ccc(2024) then aaa(2020).
        // The unrelated oracle (4) and the null-oracle card (5) are excluded.
        let rows = prints_query("mtg", "o-1", 2).all(&db).await.expect("query");
        assert_eq!(ids(rows), vec![3, 1]);

        // An oracle with only the one printing (itself) has no other printings.
        let none = prints_query("mtg", "o-2", 4).all(&db).await.expect("query");
        assert!(none.is_empty());
    }

    fn test_set(code: &str, parent: Option<&str>) -> card_set::Model {
        let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        card_set::Model {
            id: 0,
            game: "mtg".into(),
            code: code.into(),
            name: code.to_uppercase(),
            set_type: None,
            released_at: None,
            card_count: 0,
            digital: false,
            icon_svg_uri: None,
            parent_set_code: parent.map(str::to_string),
            external_id: None,
            created_at: ts,
            updated_at: ts,
        }
    }

    fn sorted(mut codes: Vec<String>) -> Vec<String> {
        codes.sort();
        codes
    }

    #[test]
    fn group_codes_standalone_set_is_alone() {
        let sets = vec![test_set("a", None), test_set("b", None)];
        assert_eq!(group_set_codes(&sets, "a"), vec!["a".to_string()]);
    }

    #[test]
    fn group_codes_span_root_and_descendants_from_any_member() {
        // tblc -> blc -> blb: a two-level chain flattened into one group.
        let sets = vec![
            test_set("blb", None),
            test_set("blc", Some("blb")),
            test_set("tblc", Some("blc")),
            test_set("other", None),
        ];
        let expected = vec!["blb".to_string(), "blc".to_string(), "tblc".to_string()];
        // Asking from the root, a middle set, or a leaf all yield the same group.
        assert_eq!(sorted(group_set_codes(&sets, "blb")), expected);
        assert_eq!(sorted(group_set_codes(&sets, "blc")), expected);
        assert_eq!(sorted(group_set_codes(&sets, "tblc")), expected);
    }

    #[test]
    fn group_codes_span_all_siblings_from_one_child() {
        // The common MTG shape: a root with two direct children (e.g. a Commander
        // deck + a token set). Querying any member returns the whole group, and an
        // unrelated multi-member group is excluded.
        let sets = vec![
            test_set("blb", None),
            test_set("blc", Some("blb")),
            test_set("tblb", Some("blb")),
            test_set("dft", None),
            test_set("tdft", Some("dft")),
        ];
        let blb_group = vec!["blb".to_string(), "blc".to_string(), "tblb".to_string()];
        assert_eq!(sorted(group_set_codes(&sets, "tblb")), blb_group);
        assert_eq!(sorted(group_set_codes(&sets, "blc")), blb_group);
        assert_eq!(sorted(group_set_codes(&sets, "blb")), blb_group);
        assert_eq!(
            sorted(group_set_codes(&sets, "dft")),
            vec!["dft".to_string(), "tdft".to_string()],
        );
    }

    #[test]
    fn group_codes_orphan_parent_is_its_own_group() {
        // Parent 'past' isn't in the catalogue, so 'pmic' is its own root.
        let sets = vec![test_set("pmic", Some("past"))];
        assert_eq!(group_set_codes(&sets, "pmic"), vec!["pmic".to_string()]);
    }

    #[test]
    fn group_codes_unknown_set_falls_back_to_itself() {
        let sets = vec![test_set("a", None)];
        assert_eq!(group_set_codes(&sets, "zzz"), vec!["zzz".to_string()]);
    }

    #[test]
    fn group_codes_survive_a_cyclic_reference() {
        // Degenerate data: a <-> b. Each resolves to itself rather than hanging.
        let sets = vec![test_set("a", Some("b")), test_set("b", Some("a"))];
        assert_eq!(group_set_codes(&sets, "a"), vec!["a".to_string()]);
        assert_eq!(group_set_codes(&sets, "b"), vec!["b".to_string()]);
    }
}
