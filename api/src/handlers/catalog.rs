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
    ColumnTrait, EntityTrait, Order, PaginatorTrait, QueryFilter, QueryOrder, prelude::DateTimeUtc,
    sea_query::{Expr, LikeExpr, NullOrdering},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::catalog::{self, Game};
use crate::entities::prelude::{Card, CardSet, IngestState};
use crate::entities::{card, card_set, ingest_state};
use crate::error::AppError;
use crate::scryfall::model::StoredFace;
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

#[derive(Debug, Serialize)]
pub struct CardFaceResponse {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
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
}

impl ListParams {
    fn page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self.page_size.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
        (page, page_size)
    }

    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
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

/// `GET /api/games/{game}/sets/{code}/cards` -> a set's cards, by collector number.
pub async fn list_set_cards(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CardResponse>>, AppError> {
    require_game(&game)?;
    let set = load_set(&state, &game, &code).await?;
    let (page, page_size) = params.page_and_size();

    let paginator = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::SetCode.eq(set.code.as_str()))
        // Numeric collector order; cards without a leading digit (NULL int) sort last.
        .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last)
        .order_by_asc(card::Column::CollectorNumber)
        .paginate(&state.db, page_size);

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
    require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let mut query = Card::find().filter(card::Column::Game.eq(game.as_str()));
    if let Some(search) = params.search() {
        // Escape LIKE metacharacters so a literal `%`/`_` in the query matches
        // literally rather than acting as a wildcard.
        let pattern = format!("%{}%", escape_like(search));
        query = query.filter(
            Expr::col((card::Entity, card::Column::Name)).like(LikeExpr::new(pattern).escape('\\')),
        );
    }
    let paginator = query
        .order_by_asc(card::Column::Name)
        .order_by_asc(card::Column::SetCode)
        .order_by_asc(card::Column::CollectorNumberInt)
        .paginate(&state.db, page_size);

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

async fn load_card(state: &AppState, game: &str, id: &str) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{id}' not found")))
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

/// Escape LIKE wildcards (`%`, `_`) and the escape char so user search input is
/// matched literally. Pair with an explicit `ESCAPE '\'` clause.
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
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
        assert!(is_allowed_image_url("https://cards.scryfall.io/normal/front/0/0/x.jpg"));
        assert!(is_allowed_image_url("https://scryfall.io/x.png"));
        assert!(!is_allowed_image_url("http://cards.scryfall.io/x.jpg")); // not https
        assert!(!is_allowed_image_url("https://evil.example.com/x.jpg")); // wrong host
        assert!(!is_allowed_image_url("https://scryfall.io.evil.com/x.jpg")); // suffix trick
        assert!(!is_allowed_image_url("not a url"));
    }

    #[test]
    fn list_params_clamps_page_size() {
        let p = ListParams { page: Some(0), page_size: Some(9999), q: None };
        assert_eq!(p.page_and_size(), (1, MAX_PAGE_SIZE));
        let d = ListParams { page: None, page_size: None, q: Some("  ".into()) };
        assert_eq!(d.page_and_size(), (1, DEFAULT_PAGE_SIZE));
        assert_eq!(d.search(), None);
    }
}
