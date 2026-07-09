//! Public catalog endpoints for **sealed products** (booster boxes, bundles, decks, …)
//! sourced from TCGCSV: the paginated list (with name / set / type filters + sorting),
//! one product's detail, its price history, its image proxy, and the filter facets.
//!
//! Products aren't cards, so the product *list* deliberately does **not** wire the
//! Scryfall search compiler — its `q` matches each whitespace-separated word as an
//! order-independent, case-insensitive name substring (all words required), mirroring
//! the card search's bare-word handling (issue #273). The "cards in this product"
//! endpoints are the exception: those rows *are* cards, so their
//! optional `q` reuses the card catalog's compiler (issue #222). Set names are resolved
//! against `card_sets` (falling back to `None` when a product's group has no matching
//! set), mirroring how the collection set builder degrades gracefully.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    Select,
    sea_query::{Expr, Func, LikeExpr, NullOrdering, SimpleExpr},
};
use serde::{Deserialize, Serialize};

use crate::catalog::Game;
use crate::db::Dialect;
use crate::entities::prelude::{
    Card, CardSet, Product, ProductPriceHistory, SealedComponent, SealedContent,
};
use crate::entities::sealed_content::Membership;
use crate::entities::{
    card, card_set, product, product_price_history, sealed_component, sealed_content,
};
use crate::extract::{Path, Query};
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, DEFAULT_PAGE_SIZE, DataBody, MAX_PAGE_SIZE, Page, PriceRange, SortDir, SortField,
    apply_card_sort, build_page, cutoff_date, downsample_rows, load_card, require_game,
    resolve_page, trim_query,
};
use crate::scryfall::search::escape_like;
use crate::state::AppState;
use crate::tcgcsv::classify::booster_family;

use super::image::is_allowed_image_url;
use super::{IMAGE_CACHE_CONTROL, image_error_response};

// ---------- Wire DTOs ----------

/// A sealed product's market prices (USD only — TCGCSV carries no eur/tix).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductPrices"))]
pub(crate) struct ProductPricesResponse {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

/// A sealed product, as the SPA sees it. Mirrors the `Card` DTO idioms: the provider
/// id is exposed as a string `id`, prices are nested, and images are fetched through
/// the proxy (`has_image` says whether one is available).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "Product"))]
pub(crate) struct ProductResponse {
    pub id: String,
    pub name: String,
    pub set_code: String,
    /// The set's display name (resolved via `card_sets`), or `None` when the
    /// product's group has no matching catalog set.
    pub set_name: Option<String>,
    pub product_type: String,
    /// The tcgplayer.com product page URL (for buy-links).
    pub url: Option<String>,
    /// Whether an image is available through the product image proxy.
    pub has_image: bool,
    pub prices: ProductPricesResponse,
    pub released_at: Option<String>,
}

/// One day's price snapshot in a product's price-over-time series (USD only).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductPricePoint"))]
pub struct ProductPricePoint {
    pub date: String,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

impl From<product_price_history::Model> for ProductPricePoint {
    fn from(m: product_price_history::Model) -> Self {
        ProductPricePoint {
            date: m.as_of_date,
            usd: m.price_usd,
            usd_foil: m.price_usd_foil,
        }
    }
}

/// A set that actually has products, for building filter dropdowns.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductSetRef"))]
pub(crate) struct ProductSetRef {
    pub code: String,
    pub name: Option<String>,
}

/// The distinct filter values that actually occur among a game's products, so the SPA
/// can build the type + set dropdowns without hardcoding them.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductFacets"))]
pub(crate) struct ProductFacets {
    /// Distinct `product_type` values, alphabetical.
    pub types: Vec<String>,
    /// Distinct sets that have products (code + resolved name), name-then-code order.
    pub sets: Vec<ProductSetRef>,
}

/// A sealed product a card is found in — or can be pulled from — plus how it relates.
/// Wraps the shared [`ProductResponse`] (so the SPA reuses the product tile/grid) with
/// the membership bucket and a foil flag (the "found in / can be in / may be in" split).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "SealedProductRef"))]
pub(crate) struct SealedProductRef {
    pub product: ProductResponse,
    /// `"contains"` (definitely in), `"booster"` (can be pulled from a booster), or
    /// `"variable"` (may be in a randomized product) — see
    /// [`crate::entities::sealed_content::Membership`].
    pub membership: String,
    /// Whether the card appears **only** as a foil in this product (a foil-only
    /// inclusion, e.g. a foil Secret Lair printing).
    pub foil: bool,
}

/// One line item of a sealed product's **composition** — "what's in the box". Carries the
/// component's kind, display name, and quantity, plus an optional link to the sub-product it
/// *is* (a `sealed` pack/box that resolves to a catalog [`ProductResponse`], so the SPA
/// renders a linked tile) or the card it *is* (a `card` promo that resolves to a
/// [`CardResponse`]). `deck` / `other` (and unresolved links) are textual — both link
/// fields `None`.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductComponent"))]
pub(crate) struct ProductComponent {
    /// `"sealed"` (a nested pack/box), `"deck"` (a precon deck), `"card"` (a fixed promo),
    /// or `"other"` (a physical extra) — see
    /// [`crate::entities::sealed_component::ComponentKind`].
    pub kind: String,
    /// Display label: the linked child's catalog name when a link resolves, else the name
    /// MTGJSON gave the component.
    pub name: String,
    /// How many of this component the product contains (`>= 1`).
    pub quantity: u32,
    /// The sub-product this component links to (a `sealed` component resolving to a catalog
    /// product), for a linked tile. `None` for textual line items.
    pub product: Option<ProductResponse>,
    /// The card this component links to (a `card` component resolving to a catalog card).
    /// `None` otherwise.
    pub card: Option<CardResponse>,
}

/// A card found in — or pullable from — a sealed product, plus how it relates. The
/// **reverse** of [`SealedProductRef`]: wraps the shared [`CardResponse`] (so the SPA
/// reuses the card tile/grid) with the membership bucket and a foil flag, so the
/// sealed-product page can render the "in the box / can be pulled from / may be in"
/// groups over the product's cards.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductCardEntry"))]
pub(crate) struct ProductCardEntry {
    pub card: CardResponse,
    /// `"contains"` (definitely in), `"booster"` (can be pulled from a booster), or
    /// `"variable"` (may be in a randomized product) — see
    /// [`crate::entities::sealed_content::Membership`]. A card that both is contained
    /// in and can be pulled from the same product reports its **strongest** membership
    /// (lowest [`Membership::rank`]), so it shows once, in the "found in" group.
    pub membership: String,
    /// Whether the card appears **only** as a foil in this product (a foil-only
    /// inclusion), at the reported membership.
    pub foil: bool,
    /// Whether this card is **exclusive** to the product's booster family — a `booster`
    /// card pullable from this product's booster line but from no *other* booster family
    /// in the set (e.g. a collector-booster-only borderless printing not on the play /
    /// draft / set sheets). Always `false` for a non-`booster` membership, for a product
    /// that isn't a booster, and for a set with no other booster family to compare against.
    /// Exclusive cards are ordered ahead of the shared booster pool so they lead the list.
    pub exclusive: bool,
}

/// One non-empty display section of a sealed product's cards: the section key and its card
/// count. The `product_cards` list splits into these buckets (`contains` → `exclusive` →
/// `booster` → `variable`, the [`CardSection`] display order); this manifest lets the SPA
/// render one **independently paginated** block per section (issue #224) — knowing which
/// sections exist and how big each is — without fetching every card first.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductCardSection"))]
pub(crate) struct ProductCardSection {
    /// The section key: `contains`, `exclusive`, `booster`, or `variable` — the value the
    /// SPA passes back as `?section=` to page that section (see [`CardSection`]).
    pub key: String,
    /// How many cards fall in this section.
    pub total: u64,
}

// ---------- Query params ----------

/// A sealed-product sort key. Maps to a product column (price via a numeric cast so it
/// orders meaningfully rather than lexically).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductSort {
    Name,
    Price,
    Released,
}

impl ProductSort {
    fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "name" => ProductSort::Name,
            "price" | "usd" => ProductSort::Price,
            "released" | "date" => ProductSort::Released,
            other => return Err(AppError::Validation(format!("unknown sort '{other}'"))),
        })
    }

    /// Natural direction when a field is named without a `dir` (priciest / newest
    /// first read better than ascending for those).
    fn default_dir(self) -> SortDir {
        match self {
            ProductSort::Name => SortDir::Asc,
            ProductSort::Price | ProductSort::Released => SortDir::Desc,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ProductListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Case-insensitive product-name filter: matches each whitespace-separated word as
    /// a name substring, AND-ed together (not Scryfall syntax).
    #[serde(default)]
    pub q: Option<String>,
    /// Filter to one set code (matched case-insensitively).
    #[serde(default)]
    pub set: Option<String>,
    /// Filter to one product type (see [`crate::tcgcsv::classify`]).
    #[serde(default, rename = "type")]
    pub type_filter: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
}

impl ProductListParams {
    fn page_and_size(&self) -> (u64, u64) {
        resolve_page(self.page, self.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE)
    }

    /// Resolve `(field, direction)` from the URL params, defaulting to name-ascending.
    /// Unknown values are a 422 (consistent with the card lists).
    fn sort_spec(&self) -> Result<(ProductSort, SortDir), AppError> {
        let field = match trim_query(self.sort.as_deref()) {
            Some(value) => ProductSort::parse(value)?,
            None => ProductSort::Name,
        };
        let dir = match trim_query(self.dir.as_deref()) {
            Some(value) => SortDir::parse(value)?,
            None => field.default_dir(),
        };
        Ok((field, dir))
    }
}

#[derive(Debug, Deserialize)]
pub struct ProductImageParams {
    pub size: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProductPriceParams {
    #[serde(default)]
    pub range: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProductCardsParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Restrict the page to one display section (`contains` / `exclusive` / `booster` /
    /// `variable`). Absent = the whole ordered list across every section (the original,
    /// back-compatible behaviour). An unknown value is a 422.
    #[serde(default)]
    pub section: Option<String>,
    /// Optional card search (the same Scryfall-style grammar the card catalog accepts —
    /// name substrings plus `c:r`, `t:goblin`, `r:mythic`, …) restricting this page to the
    /// product's cards that match, on top of any `section` filter. `total`/`has_more` then
    /// describe the filtered result. A malformed query is a 422 (issue #222).
    #[serde(default)]
    pub q: Option<String>,
    /// Optional card-list sort (`name`/`rarity`/`cmc`/`price`/…, the shared card-list
    /// vocabulary the catalog browse accepts). Absent = the product's natural membership /
    /// exclusive / set-number order. A sort re-orders the cards **within** each display
    /// section — the section split itself is unchanged. An unknown value is a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`) for `sort`; absent = the field's natural direction.
    /// Ignored without a `sort`. An unknown value is a 422.
    #[serde(default)]
    pub dir: Option<String>,
}

impl ProductCardsParams {
    /// The requested `(field, direction)` card sort, or `None` for the product's natural
    /// order. A `sort` value maps to the shared card-list vocabulary; `dir` overrides its
    /// natural direction. Unknown values are a 422, mirroring the section filter + card lists.
    fn sort_spec(&self) -> Result<Option<(SortField, SortDir)>, AppError> {
        let Some(field) = trim_query(self.sort.as_deref()) else {
            return Ok(None);
        };
        let field = SortField::parse(field)?;
        let dir = match trim_query(self.dir.as_deref()) {
            Some(value) => SortDir::parse(value)?,
            None => field.default_dir(),
        };
        Ok(Some((field, dir)))
    }
}

#[derive(Debug, Deserialize)]
pub struct ProductCardSectionsParams {
    /// Optional card search (see [`ProductCardsParams::q`]) that filters the manifest to the
    /// sections — and per-section counts — whose cards match, so the section list, its
    /// counts, and each section's paged cards all agree under the same `q` (issue #222). A
    /// malformed query is a 422.
    #[serde(default)]
    pub q: Option<String>,
}

/// A display section of a sealed product's cards — its membership bucket, with the booster
/// pool split into the **family-exclusive** printings and the **shared** pool. The SPA
/// paginates each section independently (issue #224). The variant order is the display
/// order, and matches the ordering [`build_product_card_index`] sorts the cards into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardSection {
    /// Guaranteed cards (`contains` membership) — "In the box".
    Contains,
    /// `booster` cards exclusive to this product's booster family (a collector booster's
    /// special printings that no other booster in the set can pull).
    Exclusive,
    /// `booster` cards shared with the set's other booster families (the wider pull pool).
    Booster,
    /// `variable` cards (a randomized / either-or configuration) — "May be included".
    Variable,
}

impl CardSection {
    /// The wire key (also the `?section=` filter value).
    fn key(self) -> &'static str {
        match self {
            CardSection::Contains => "contains",
            CardSection::Exclusive => "exclusive",
            CardSection::Booster => "booster",
            CardSection::Variable => "variable",
        }
    }

    /// Parse a `?section=` value; an unknown key is a 422 (as with the list sort/dir params).
    fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "contains" => CardSection::Contains,
            "exclusive" => CardSection::Exclusive,
            "booster" => CardSection::Booster,
            "variable" => CardSection::Variable,
            other => return Err(AppError::Validation(format!("unknown section '{other}'"))),
        })
    }

    /// The section a card falls in, from its strongest membership and whether it's a
    /// family-exclusive booster card: the `booster` membership splits into
    /// [`Exclusive`](CardSection::Exclusive) / [`Booster`](CardSection::Booster) (shared),
    /// the others map 1:1. An unrecognised membership (there are only the three known ones)
    /// falls back to the weakest [`Variable`](CardSection::Variable) bucket.
    fn classify(membership: &str, exclusive: bool) -> Self {
        if membership == Membership::Contains.as_str() {
            CardSection::Contains
        } else if membership == Membership::Booster.as_str() {
            if exclusive {
                CardSection::Exclusive
            } else {
                CardSection::Booster
            }
        } else {
            CardSection::Variable
        }
    }
}

// ---------- Handlers ----------

/// `GET /api/games/{game}/products` -> a page of sealed products, filtered by
/// `q`/`set`/`type` and ordered by `sort`/`dir` (default name-ascending).
pub async fn list_products(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ProductListParams>,
) -> Result<Json<Page<ProductResponse>>, AppError> {
    require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let mut query = Product::find().filter(product::Column::Game.eq(game.as_str()));
    if let Some(term) = trim_query(params.q.as_deref()) {
        // Match every whitespace-separated word as its own order-independent name
        // substring, AND-ed together, so "final fantasy bundle" finds "Final Fantasy -
        // Gift Bundle" and "FINAL FANTASY - Chocobo Bundle" (issue #273). This mirrors
        // the Scryfall card search's bare-word handling, giving cards and sealed the
        // same "all words present" match. LOWER both sides (ASCII fold) so each match is
        // case-insensitive on Postgres too; `to_ascii_lowercase` matches SQLite's
        // ASCII-only `LOWER()`, so the SQLite result set stays byte-identical.
        // `trim_query` dropped a blank query, so there is always at least one word.
        for word in term.split_whitespace() {
            let pattern = format!("%{}%", escape_like(word).to_ascii_lowercase());
            query = query.filter(
                Expr::expr(Func::lower(Expr::col((product::Entity, product::Column::Name))))
                    .like(LikeExpr::new(pattern).escape('\\')),
            );
        }
    }
    if let Some(set) = trim_query(params.set.as_deref()) {
        query = query.filter(product::Column::SetCode.eq(set.to_lowercase()));
    }
    if let Some(ptype) = trim_query(params.type_filter.as_deref()) {
        query = query.filter(product::Column::ProductType.eq(ptype));
    }

    let (sort, dir) = params.sort_spec()?;
    let paginator =
        apply_product_sort(query, sort, dir, state.dialect()).paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    let names = set_name_map(&state, &game).await?;
    let data: Vec<ProductResponse> = rows
        .into_iter()
        .map(|p| into_response(p, &names))
        .collect();
    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/games/{game}/products/{id}` -> one product's detail.
pub async fn get_product(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<ProductResponse>, AppError> {
    require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;
    let names = set_name_map(&state, &game).await?;
    Ok(Json(into_response(product, &names)))
}

/// `GET /api/games/{game}/products/{id}/prices?range=` -> a product's price history,
/// oldest first, reusing the exact windowing/downsampling of the card price endpoint.
pub async fn product_prices(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ProductPriceParams>,
) -> Result<Json<DataBody<Vec<ProductPricePoint>>>, AppError> {
    require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;

    let range = match trim_query(params.range.as_deref()) {
        None => None,
        Some(value) => Some(PriceRange::parse(value)?),
    };

    let mut query = ProductPriceHistory::find()
        .filter(product_price_history::Column::Game.eq(game.as_str()))
        .filter(product_price_history::Column::ProductId.eq(product.id));
    if let Some(cutoff) = range.and_then(|r| cutoff_date(Utc::now().date_naive(), r)) {
        query = query.filter(product_price_history::Column::AsOfDate.gte(cutoff));
    }
    let rows = query
        .order_by_asc(product_price_history::Column::AsOfDate)
        .all(&state.db)
        .await?;

    let kept = downsample_rows(rows, range.map_or(1, PriceRange::bucket_days), |r| {
        r.as_of_date.as_str()
    });
    let data: Vec<ProductPricePoint> = kept.into_iter().map(ProductPricePoint::from).collect();
    Ok(Json(DataBody { data }))
}

/// `GET /api/games/{game}/products/{id}/image?size=` -> the product image, proxied +
/// cached from the TCGplayer CDN. `size` ∈ `normal` (1000×1000) / `small` (200w).
pub async fn product_image(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ProductImageParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    // 404 an unknown product (its id also validates the CDN key we build below).
    let product = load_product(&state, &game, &id).await?;
    let size = normalize_product_size(params.size.as_deref());
    let source_url = product_cdn_url(&product.external_id, size);

    if !is_allowed_image_url(&source_url) {
        tracing::warn!(product = %id, url = %source_url, "refusing to proxy non-allowlisted product image");
        return Err(AppError::NotFound("no image available".to_string()));
    }

    let image = state
        .images
        .get("products", size, &product.external_id, &source_url)
        .await
        .map_err(|err| image_error_response(err, "product", &id))?;

    Ok((
        [
            (header::CONTENT_TYPE, image.content_type),
            (header::CACHE_CONTROL, IMAGE_CACHE_CONTROL),
        ],
        image.bytes,
    )
        .into_response())
}

/// `GET /api/games/{game}/products/facets` -> the distinct product types + the sets
/// that actually have products, so the SPA can build filter dropdowns.
pub async fn product_facets(
    State(state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<DataBody<ProductFacets>>, AppError> {
    require_game(&game)?;

    let mut types: Vec<String> = Product::find()
        .select_only()
        .column(product::Column::ProductType)
        .distinct()
        .filter(product::Column::Game.eq(game.as_str()))
        .into_tuple()
        .all(&state.db)
        .await?;
    types.sort();

    let mut codes: Vec<String> = Product::find()
        .select_only()
        .column(product::Column::SetCode)
        .distinct()
        .filter(product::Column::Game.eq(game.as_str()))
        // A blank set_code (a group with no abbreviation) isn't a usable filter value.
        .filter(product::Column::SetCode.ne(""))
        .into_tuple()
        .all(&state.db)
        .await?;

    let names = set_name_map(&state, &game).await?;
    codes.sort_by(|a, b| {
        // Sort by resolved name (code as fallback), then code, so the dropdown reads
        // in set-name order.
        let an = names.get(a).map_or(a.as_str(), String::as_str);
        let bn = names.get(b).map_or(b.as_str(), String::as_str);
        an.cmp(bn).then_with(|| a.cmp(b))
    });
    let sets: Vec<ProductSetRef> = codes
        .into_iter()
        .map(|code| {
            let name = names.get(&code).cloned();
            ProductSetRef { code, name }
        })
        .collect();

    Ok(Json(DataBody {
        data: ProductFacets { types, sets },
    }))
}

/// `GET /api/games/{game}/cards/{id}/sealed` -> the sealed products this card is found
/// in (or can be pulled from). Ordered `contains` → `booster` → `variable`, then by
/// product name, so the SPA can render the three "found in / can be in / may be in"
/// groups in place. Empty `{ "data": [] }` when the card is in no ingested product (or
/// no contents have been ingested at all). `404` for an unknown game/card.
pub async fn card_sealed(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<SealedProductRef>>>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;

    // Every membership row for this card (hits idx_sealed_contents_game_card).
    let rows = SealedContent::find()
        .filter(sealed_content::Column::Game.eq(game.as_str()))
        .filter(sealed_content::Column::CardId.eq(card.id))
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }

    // Collapse to one entry per (product, membership): a product holding both a foil and
    // a non-foil printing in the same bucket shows once, flagged `foil` only when *every*
    // contributing row is foil (a foil-only inclusion). `foil_only` starts true and is
    // ANDed down as soon as any non-foil row is seen.
    let mut groups: HashMap<(i32, String), bool> = HashMap::new();
    for row in &rows {
        let foil_only = groups
            .entry((row.product_id, row.membership.clone()))
            .or_insert(true);
        *foil_only = *foil_only && row.foil;
    }

    // Load the referenced products in one query (a card is in a bounded number of
    // products), then dress each with its set name like the other product responses.
    let product_ids: Vec<i32> = {
        let mut ids: Vec<i32> = groups.keys().map(|(pid, _)| *pid).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let products: HashMap<i32, product::Model> = Product::find()
        .filter(product::Column::Game.eq(game.as_str()))
        .filter(product::Column::Id.is_in(product_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();

    let names = set_name_map(&state, &game).await?;
    let mut data: Vec<SealedProductRef> = groups
        .into_iter()
        .filter_map(|((product_id, membership), foil)| {
            // A membership row whose product row vanished (e.g. mid-reimport) is skipped.
            products.get(&product_id).map(|p| SealedProductRef {
                product: into_response(p.clone(), &names),
                membership: membership.to_string(),
                foil,
            })
        })
        .collect();

    // Definitely-in first, then boosters, then maybe; product name as the tiebreak so
    // the order is stable across requests.
    data.sort_by(|a, b| {
        Membership::rank(&a.membership)
            .cmp(&Membership::rank(&b.membership))
            .then_with(|| a.product.name.cmp(&b.product.name))
    });

    Ok(Json(DataBody { data }))
}

/// `GET /api/games/{game}/products/{id}/contents` -> the sealed product's structural
/// composition — "what's in the box" — in display order (nested packs/boxes, then precon
/// decks, then fixed promo cards, then physical extras). A `sealed` component that resolves
/// to a catalog product carries the linked `product` (so the SPA can render "the products
/// this box contains"); a `card` component carries the linked `card`; the rest are textual.
/// `{ "data": [] }` when the product has no ingested composition; `404` for an unknown
/// game/product.
pub async fn product_contents(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<ProductComponent>>>, AppError> {
    require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;

    // The product's components, already ordered by `position` (hits the
    // (game, product_id, position) unique index).
    let rows = SealedComponent::find()
        .filter(sealed_component::Column::Game.eq(game.as_str()))
        .filter(sealed_component::Column::ProductId.eq(product.id))
        .order_by_asc(sealed_component::Column::Position)
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }

    // Load the linked sub-products + promo cards in one query each (a composition has a
    // bounded number of components), then dress each line item.
    let child_product_ids: Vec<i32> = rows.iter().filter_map(|r| r.child_product_id).collect();
    let child_card_ids: Vec<i32> = rows.iter().filter_map(|r| r.child_card_id).collect();

    let names = set_name_map(&state, &game).await?;
    let child_products: HashMap<i32, product::Model> = if child_product_ids.is_empty() {
        HashMap::new()
    } else {
        Product::find()
            .filter(product::Column::Game.eq(game.as_str()))
            .filter(product::Column::Id.is_in(child_product_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect()
    };
    let child_cards: HashMap<i32, card::Model> = if child_card_ids.is_empty() {
        HashMap::new()
    } else {
        Card::find()
            .filter(card::Column::Game.eq(game.as_str()))
            .filter(card::Column::Id.is_in(child_card_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect()
    };

    let data: Vec<ProductComponent> = rows
        .into_iter()
        .map(|row| {
            // A link whose child row vanished (e.g. mid-reimport) degrades to a textual item.
            let linked_product = row
                .child_product_id
                .and_then(|cid| child_products.get(&cid))
                .map(|p| into_response(p.clone(), &names));
            let linked_card: Option<CardResponse> = row
                .child_card_id
                .and_then(|cid| child_cards.get(&cid))
                .map(|c| c.clone().into());
            // Prefer a linked child's catalog name for the label (product or card); else
            // the name MTGJSON gave the component.
            let name = linked_product
                .as_ref()
                .map(|p| p.name.clone())
                .or_else(|| linked_card.as_ref().map(|c| c.name.clone()))
                .unwrap_or(row.name);
            ProductComponent {
                kind: row.kind,
                name,
                quantity: row.quantity.max(0) as u32,
                product: linked_product,
                card: linked_card,
            }
        })
        .collect();

    Ok(Json(DataBody { data }))
}

/// SQLite caps host parameters per statement (as few as 999 on old builds), so the
/// by-card-id lookups are chunked — a huge product (Secret Lair "festival" bundles
/// reference thousands of cards) can't blow the bind limit.
const PRODUCT_CARDS_IN_CHUNK: usize = 900;

/// `GET /api/games/{game}/products/{id}/cards?page=&page_size=&section=` -> a page of the
/// cards this sealed product is found to contain (or can be pulled from), the **reverse** of
/// `cards/{id}/sealed`. Ordered by membership (`contains` → `booster` → `variable`, so
/// the guaranteed cards lead and the wider booster pool follows) and, within the booster
/// pool, **family-exclusive cards first** (a collector booster's borderless/extended-art
/// printings that no other booster in the set can pull — each flagged `exclusive`), then
/// by set code and collector number. Each card is deduped to its strongest membership and
/// carries a `foil`-only flag.
///
/// The optional `?section=` param restricts the page to one display section (`contains` /
/// `exclusive` / `booster` / `variable`) so the SPA can paginate each section on its own
/// (issue #224); omit it for the whole ordered list. `total`/`has_more` then describe the
/// selected section (or the whole list). Empty page when the product has no ingested
/// contents (or the section is empty); `404` for an unknown game/product, `422` for an
/// unknown section.
///
/// The optional `?sort=`/`?dir=` params re-order the cards **within** each display section
/// by the shared card-list vocabulary (`name`/`rarity`/`cmc`/`price`/…), so a product's cards
/// sort like the same cards on the catalog browse; the section split (and its display order)
/// is untouched — a sort only changes the order *inside* a section. Absent = the product's
/// natural membership / exclusive / set-number order. An unknown sort/dir is a `422`.
pub async fn product_cards(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ProductCardsParams>,
) -> Result<Json<Page<ProductCardEntry>>, AppError> {
    let game_meta = require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;
    let (page, page_size) =
        resolve_page(params.page, params.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE);
    // Parse the optional section + sort filters up front so a bad value 422s before any DB work.
    let section = match trim_query(params.section.as_deref()) {
        Some(value) => Some(CardSection::parse(value)?),
        None => None,
    };
    let sort = params.sort_spec()?;

    // The product's cards, deduped + fully ordered, plus the membership/exclusivity lookups.
    let index = build_product_card_index(&state, &game, &product).await?;

    // The base ordering the sections draw from: the product's natural membership / exclusive /
    // set-number order (the default), or — when a `sort` is requested — every one of this
    // product's cards in the chosen card-list order. Either way the section split below is
    // unchanged; a sort only re-orders the cards *within* each section.
    let base_order: Vec<i32> = match sort {
        None => index.ordered.clone(),
        Some((field, dir)) => sorted_product_card_ids(&state, &game, &product, field, dir).await?,
    };

    // The ids this request pages over. With `?section=`, just that section's cards (in
    // `base_order`, so a sort carries through). Without it, the whole list — re-grouped by
    // section (display order) when a sort is active so the section grouping survives the sort;
    // the default `base_order` is already section-grouped, so it passes through untouched.
    let selected: Vec<i32> = match (section, sort) {
        (Some(section), _) => base_order
            .into_iter()
            .filter(|&cid| index.section_of(cid) == Some(section))
            .collect(),
        (None, None) => base_order,
        (None, Some(_)) => group_by_section(&index, &base_order),
    };
    // Narrow to the cards matching the optional `q` search (issue #222), still in order;
    // a malformed query 422s here before the page is loaded.
    let selected = filter_ordered_by_search(&state, game_meta, params.q.as_deref(), &selected)
        .await?;

    let total = selected.len() as u64;
    let start = (page - 1).saturating_mul(page_size) as usize;
    let page_ids: Vec<i32> = selected
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();

    // Only the page's cards are loaded in full + mapped to the (heavier) card DTO.
    let mut models: HashMap<i32, card::Model> = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::Id.is_in(page_ids.iter().copied()))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|m| (m.id, m))
        .collect();

    let data: Vec<ProductCardEntry> = page_ids
        .into_iter()
        .filter_map(|cid| {
            let model = models.remove(&cid)?;
            let (_, membership, foil) = index.best.get(&cid)?;
            Some(ProductCardEntry {
                card: model.into(),
                membership: membership.clone(),
                foil: *foil,
                exclusive: index.exclusive.contains(&cid),
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/games/{game}/products/{id}/cards/sections` -> the non-empty display sections of
/// this product's cards, in display order (`contains` → `exclusive` → `booster` →
/// `variable`), each with its card count. The reverse-companion of `product_cards`: the SPA
/// reads this first to know which sections exist and how big each is, then renders one
/// independently-paginated block per section, pulling each with `?section=` (issue #224).
/// An optional `?q=` filters the manifest to the sections (and counts) whose cards match
/// that search, so it agrees with the filtered `product_cards` pages (issue #222).
/// `{ "data": [] }` when the product has no ingested contents (or nothing matches `q`);
/// `404` for an unknown game/product, `422` for a malformed `q`.
pub async fn product_card_sections(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ProductCardSectionsParams>,
) -> Result<Json<DataBody<Vec<ProductCardSection>>>, AppError> {
    let game_meta = require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;
    let index = build_product_card_index(&state, &game, &product).await?;
    // Restrict the counted ids to those matching the optional `q` search, still in display
    // order (issue #222) — so a section with no matches drops out of the manifest.
    let ordered = filter_ordered_by_search(&state, game_meta, params.q.as_deref(), &index.ordered)
        .await?;

    // Count per section by walking the already-ordered ids: first appearance fixes the
    // section's slot, so the manifest comes out in the same display order the list uses. A
    // Vec (≤ 4 sections) keeps that order — a HashMap wouldn't — and the linear find is trivial.
    let mut sections: Vec<ProductCardSection> = Vec::new();
    for &cid in &ordered {
        let Some(section) = index.section_of(cid) else {
            continue;
        };
        let key = section.key();
        match sections.iter_mut().find(|s| s.key == key) {
            Some(existing) => existing.total += 1,
            None => sections.push(ProductCardSection {
                key: key.to_string(),
                total: 1,
            }),
        }
    }

    Ok(Json(DataBody { data: sections }))
}

// ---------- Helpers ----------

/// A sealed product's cards, deduped and fully ordered, plus the lookups needed to dress or
/// bucket them. Built once by [`build_product_card_index`] and shared by the paged
/// `product_cards` read and the `product_card_sections` count so both agree on each card's
/// membership, exclusivity, section, and position.
struct ProductCardIndex {
    /// `card_id -> (membership_rank, membership, foil)` at the card's strongest membership.
    best: HashMap<i32, (u8, String, bool)>,
    /// The subset of booster cards exclusive to this product's booster family.
    exclusive: HashSet<i32>,
    /// Every distinct card id, in final display order (membership, then family-exclusive
    /// booster cards ahead of the shared pool, then set code + collector number).
    ordered: Vec<i32>,
}

impl ProductCardIndex {
    /// The display section a card falls in (its membership, with the booster pool split into
    /// family-exclusive vs shared), or `None` if the id isn't in the index.
    fn section_of(&self, card_id: i32) -> Option<CardSection> {
        let (_, membership, _) = self.best.get(&card_id)?;
        Some(CardSection::classify(membership, self.exclusive.contains(&card_id)))
    }
}

/// Dedupe, order, and index a product's cards: fetch its membership rows, collapse each card
/// to its strongest membership, flag the family-exclusive booster cards, and sort the whole
/// list into display order. The heavy per-page `card::Model` load stays out here — only the
/// id ordering + the membership/exclusivity lookups — so both the paged read and the section
/// count can share it cheaply. An empty (all-zero) index when the product has no contents.
async fn build_product_card_index(
    state: &AppState,
    game: &str,
    product: &product::Model,
) -> Result<ProductCardIndex, AppError> {
    // Every membership row for this product (hits the (game, product_id) prefix of
    // idx_sealed_contents_unique), selecting only the three fields the dedup folds —
    // a giant product's contents run to thousands of rows, so the timestamps + game
    // column of the full model aren't worth deserializing.
    let rows: Vec<(i32, String, bool)> = SealedContent::find()
        .select_only()
        .column(sealed_content::Column::CardId)
        .column(sealed_content::Column::Membership)
        .column(sealed_content::Column::Foil)
        .filter(sealed_content::Column::Game.eq(game))
        .filter(sealed_content::Column::ProductId.eq(product.id))
        .into_tuple()
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(ProductCardIndex {
            best: HashMap::new(),
            exclusive: HashSet::new(),
            ordered: Vec::new(),
        });
    }

    // Collapse to one entry per card at its strongest (lowest-rank) membership, foil
    // ANDed among that membership's rows (foil-only when every contributing row is foil).
    let best = best_memberships(&rows);

    // Which of this product's booster cards are exclusive to its booster family (a
    // collector-booster-only printing, say) — one small cross-product lookup, empty for a
    // non-booster product or a set with nothing to compare against.
    let exclusive = booster_exclusive_card_ids(state, game, product, &best).await?;

    // Load the sort keys for every distinct card so the full list can be ordered before
    // it's paged; chunked under the bind limit. A card whose row vanished mid-reimport
    // simply drops out (it's excluded from the ordered list and so from every `total`).
    let card_ids: Vec<i32> = best.keys().copied().collect();
    let mut ordered: Vec<(u8, u8, String, Option<i32>, String, i32)> =
        Vec::with_capacity(card_ids.len());
    for chunk in card_ids.chunks(PRODUCT_CARDS_IN_CHUNK) {
        let keys: Vec<(i32, String, Option<i32>, String)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::SetCode)
            .column(card::Column::CollectorNumberInt)
            .column(card::Column::CollectorNumber)
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        for (cid, set_code, cn_int, cn) in keys {
            let rank = best.get(&cid).map_or(u8::MAX, |entry| entry.0);
            // Within the booster rank, exclusive cards (0) sort ahead of the shared pool
            // (1) so they lead. Non-booster cards are never exclusive, so they all take 1.
            let exclusive_rank = u8::from(!exclusive.contains(&cid));
            ordered.push((rank, exclusive_rank, set_code, cn_int, cn, cid));
        }
    }

    // Membership first (guaranteed cards lead), then family-exclusive booster cards ahead
    // of the shared pool, then set code, then numeric-run-first collector number (NULLs
    // last), with `id` as a stable tiebreak so paging is deterministic — the same order
    // the catalog's set listing uses within a set.
    ordered.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| cn_int_key(a.3).cmp(&cn_int_key(b.3)))
            .then_with(|| a.4.cmp(&b.4))
            .then_with(|| a.5.cmp(&b.5))
    });

    Ok(ProductCardIndex {
        best,
        exclusive,
        ordered: ordered.into_iter().map(|entry| entry.5).collect(),
    })
}

/// Every one of a product's cards, ordered by the requested card-list `sort`/`dir` — the base
/// order the paged read draws from when a caller asks for an explicit sort (the section split
/// still buckets these afterwards, so the sort only re-orders *within* each section). Reuses
/// the catalog's [`apply_card_sort`] so a product's cards sort byte-identically to the same
/// cards on the catalog browse.
///
/// The product's card set is expressed as `cards.id IN (subquery)` over its `sealed_contents`
/// rows rather than a bound id list, so even a giant booster pool needs no chunking and can't
/// blow the SQL bind limit. Deliberately **no** `SELECT DISTINCT` on the subquery: the `IN`
/// semi-join already collapses the duplicate `card_id` rows (a card with foil + non-foil, or
/// several memberships, in one product), and a `DISTINCT card_id` would steer the planner onto
/// the `(game, card_id)` index and scan the whole game partition — the same trap
/// [`booster_exclusive_card_ids`] documents.
async fn sorted_product_card_ids(
    state: &AppState,
    game: &str,
    product: &product::Model,
    field: SortField,
    dir: SortDir,
) -> Result<Vec<i32>, AppError> {
    let product_card_ids = SealedContent::find()
        .select_only()
        .column(sealed_content::Column::CardId)
        .filter(sealed_content::Column::Game.eq(game))
        .filter(sealed_content::Column::ProductId.eq(product.id))
        .into_query();

    let query = Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(Expr::col((card::Entity, card::Column::Id)).in_subquery(product_card_ids))
        .select_only()
        .column(card::Column::Id);
    let ids: Vec<i32> = apply_card_sort(query, field, dir, false, state.dialect())
        .into_tuple()
        .all(&state.db)
        .await?;
    Ok(ids)
}

/// Re-group a list of a product's card ids by display section (`contains` → `exclusive` →
/// `booster` → `variable`), preserving the incoming order *within* each section. Keeps the
/// section grouping intact for the whole-product (`?section=` omitted) response when a `sort`
/// has flattened the ids into a single card-order run: the sections still lead in display
/// order, each holding its cards in the sorted order. A no-op on the already-section-grouped
/// default order, so the unsorted whole-product response is byte-identical to before.
fn group_by_section(index: &ProductCardIndex, order: &[i32]) -> Vec<i32> {
    const SECTIONS: [CardSection; 4] = [
        CardSection::Contains,
        CardSection::Exclusive,
        CardSection::Booster,
        CardSection::Variable,
    ];
    let mut grouped = Vec::with_capacity(order.len());
    for section in SECTIONS {
        grouped.extend(
            order
                .iter()
                .copied()
                .filter(|&cid| index.section_of(cid) == Some(section)),
        );
    }
    grouped
}

/// Restrict a product's ordered card ids to those also matching an optional `q` search,
/// **preserving display order** — the shared filter behind the sealed-product card search
/// (issue #222). A `None`/blank query leaves the list untouched; a malformed query is a
/// 422 (surfaced before any DB work, mirroring the section-filter parse). The search reuses
/// the card catalog's compiler (`c:r`, `t:goblin`, `r:mythic`, name substrings, …) via
/// [`super::parse_search`], run as a membership test over just this product's cards: the
/// compiled condition ANDed with a chunked `IN (ordered ids)`, so even a giant booster pool
/// stays within the SQL bind limit. Both `product_cards` and `product_card_sections` route
/// through here so the paged cards, the section list, and its counts all agree.
async fn filter_ordered_by_search(
    state: &AppState,
    game: &Game,
    q: Option<&str>,
    ordered: &[i32],
) -> Result<Vec<i32>, AppError> {
    let Some(search) = trim_query(q) else {
        return Ok(ordered.to_vec());
    };
    // Compile up front so a malformed query 422s before the per-chunk lookups run.
    let (condition, _shape) = super::parse_search(game, search, state.dialect())?;
    if ordered.is_empty() {
        return Ok(Vec::new());
    }

    // Which of this product's cards satisfy the search: the compiled condition intersected
    // with each chunk of the ordered ids (the game filter matches the paged read's).
    let mut matched: HashSet<i32> = HashSet::with_capacity(ordered.len());
    for chunk in ordered.chunks(PRODUCT_CARDS_IN_CHUNK) {
        let ids: Vec<i32> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(game.id))
            .filter(condition.clone())
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        matched.extend(ids);
    }

    // Keep the product's display order; drop everything the search didn't match.
    Ok(ordered.iter().copied().filter(|cid| matched.contains(cid)).collect())
}

/// Collapse a product's raw membership rows `(card_id, membership, foil)` to one entry
/// per card at its strongest (lowest-[`Membership::rank`]) membership, foil ANDed among
/// the rows of that chosen membership (so `foil` is true only when every contributing row
/// is foil — a foil-only inclusion). Returns `card_id -> (rank, membership, foil)`.
///
/// A card can carry several rows for one product: split finishes (foil + non-foil) and
/// even distinct memberships (e.g. a set booster box that also guarantees a promo). The
/// stronger membership wins and resets the foil accumulator, so a "contains" non-foil
/// row correctly overrides a "booster" foil row.
fn best_memberships(rows: &[(i32, String, bool)]) -> HashMap<i32, (u8, String, bool)> {
    use std::collections::hash_map::Entry;
    let mut best: HashMap<i32, (u8, String, bool)> = HashMap::new();
    for (card_id, membership, foil) in rows {
        let rank = Membership::rank(membership);
        match best.entry(*card_id) {
            Entry::Vacant(slot) => {
                slot.insert((rank, membership.clone(), *foil));
            }
            Entry::Occupied(mut slot) => {
                let entry = slot.get_mut();
                if rank < entry.0 {
                    // A stronger membership: take over and reset the foil accumulator.
                    *entry = (rank, membership.clone(), *foil);
                } else if rank == entry.0 {
                    // Same membership (rank maps 1:1 to the three known values): a
                    // non-foil row downgrades the foil-only flag.
                    entry.2 = entry.2 && *foil;
                }
                // A weaker membership than one already recorded: ignore.
            }
        }
    }
    best
}

/// The subset of this product's `booster`-membership cards that are **exclusive** to its
/// booster family: pullable from this product's booster line but from no booster product
/// of a *different* family in the same set (e.g. a collector-booster-only borderless
/// printing the play / draft / set sheets don't carry).
///
/// Returns an empty set — nothing flagged exclusive — when the product isn't a booster
/// (a deck / bundle / …), when it has no booster cards, or when the set has no other
/// booster family to compare against (a collector-only Commander release, say, where
/// "exclusive" would be vacuously true of every card and so carries no signal). Two small
/// indexed lookups: the set's other-family booster products, then their booster card pool.
async fn booster_exclusive_card_ids(
    state: &AppState,
    game: &str,
    product: &product::Model,
    best: &HashMap<i32, (u8, String, bool)>,
) -> Result<HashSet<i32>, AppError> {
    let Some(family) = booster_family(&product.product_type) else {
        return Ok(HashSet::new());
    };

    // This product's own booster-pullable cards — the only ones exclusivity applies to.
    let booster = Membership::Booster.as_str();
    let own_booster: HashSet<i32> = best
        .iter()
        .filter(|(_, (_, membership, _))| membership == booster)
        .map(|(id, _)| *id)
        .collect();
    if own_booster.is_empty() {
        return Ok(HashSet::new());
    }

    // The set's booster products of a *different* family — the comparison pool. (Same-set
    // scope, so a collector display/case of the same family is excluded by the type list.)
    let comparison_products: Vec<i32> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .filter(product::Column::Game.eq(game))
        .filter(product::Column::SetCode.eq(&product.set_code))
        .filter(product::Column::ProductType.is_in(family.other_booster_types()))
        .into_tuple()
        .all(&state.db)
        .await?;
    if comparison_products.is_empty() {
        return Ok(HashSet::new());
    }

    // Every card those other-family boosters can pull; one of ours not in this pool is
    // exclusive to our family.
    //
    // No `SELECT DISTINCT`: we collect straight into a `HashSet`, so the DB-side dedup is
    // redundant — and worse, it's a performance trap. With no `ANALYZE` statistics (this
    // schema never runs `ANALYZE`), SQLite serves a `DISTINCT card_id` by scanning the
    // `(game, card_id)` index to get pre-sorted ids, which for `game = 'mtg'` walks the
    // *whole* ~1M-row partition plus a table lookup per row (~0.9s) — and this runs on
    // every product-cards / sections request, i.e. every page turn through a collector
    // booster. Dropping `DISTINCT` lets the planner use the covering
    // `idx_sealed_contents_unique` with tight `(game, product_id)` seeks over the small
    // comparison list instead (~1ms). The `HashSet` handles the duplicate ids.
    let comparison_cards: HashSet<i32> = SealedContent::find()
        .select_only()
        .column(sealed_content::Column::CardId)
        .filter(sealed_content::Column::Game.eq(game))
        .filter(sealed_content::Column::Membership.eq(booster))
        .filter(sealed_content::Column::ProductId.is_in(comparison_products))
        .into_tuple()
        .all(&state.db)
        .await?
        .into_iter()
        .collect();
    if comparison_cards.is_empty() {
        return Ok(HashSet::new());
    }

    Ok(own_booster
        .into_iter()
        .filter(|id| !comparison_cards.contains(id))
        .collect())
}

/// Collator key for a card's numeric collector number that parks `NULL` (a non-numeric
/// collector number) last in ascending order, matching the catalog's `NULLS LAST`.
fn cn_int_key(value: Option<i32>) -> (bool, i32) {
    match value {
        Some(n) => (false, n),
        None => (true, 0),
    }
}

/// Resolve a product by its external (TCGplayer) id within a game, 404 if unknown.
async fn load_product(
    state: &AppState,
    game: &str,
    id: &str,
) -> Result<product::Model, AppError> {
    Product::find()
        .filter(product::Column::Game.eq(game))
        .filter(product::Column::ExternalId.eq(id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("product '{id}' not found")))
}

/// The game's `set_code -> set_name` map, for dressing products with their set name.
async fn set_name_map(state: &AppState, game: &str) -> Result<HashMap<String, String>, AppError> {
    let rows: Vec<(String, String)> = CardSet::find()
        .select_only()
        .column(card_set::Column::Code)
        .column(card_set::Column::Name)
        .filter(card_set::Column::Game.eq(game))
        .into_tuple()
        .all(&state.db)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Build the wire DTO, resolving the set name from `names` (falling back to `None`).
fn into_response(p: product::Model, names: &HashMap<String, String>) -> ProductResponse {
    let set_name = names.get(&p.set_code).cloned();
    ProductResponse {
        id: p.external_id,
        name: p.name,
        set_name,
        set_code: p.set_code,
        product_type: p.product_type,
        url: p.url,
        // A sealed product image lives at the deterministic CDN URL keyed on its id;
        // the provider `image_url` presence is a good proxy for "has an image".
        has_image: p.image_url.is_some(),
        prices: ProductPricesResponse {
            usd: p.price_usd,
            usd_foil: p.price_usd_foil,
        },
        released_at: p.released_at,
    }
}

/// The TCGplayer CDN URL for a product image at the requested size.
fn product_cdn_url(product_id: &str, size: &str) -> String {
    let variant = match size {
        "small" => "200w",
        _ => "in_1000x1000",
    };
    format!("https://tcgplayer-cdn.tcgplayer.com/product/{product_id}_{variant}.jpg")
}

/// Map a requested product image size to an allow-listed one (default `normal`).
pub(super) fn normalize_product_size(requested: Option<&str>) -> &'static str {
    match requested {
        Some("small") => "small",
        _ => "normal",
    }
}

/// Apply the requested ordering, ending with a stable `id` tiebreaker so pagination is
/// deterministic. Price sorts on a numeric cast (falling back to the foil price) with
/// unpriced products parked last regardless of direction.
fn apply_product_sort(
    query: Select<product::Entity>,
    field: ProductSort,
    dir: SortDir,
    dialect: Dialect,
) -> Select<product::Entity> {
    let query = match field {
        ProductSort::Name => query.order_by(product::Column::Name, dir.order()),
        ProductSort::Price => query
            .order_by_with_nulls(product_price_expr(dialect), dir.order(), NullOrdering::Last)
            .order_by_asc(product::Column::Name),
        ProductSort::Released => query
            .order_by_with_nulls(product::Column::ReleasedAt, dir.order(), NullOrdering::Last)
            .order_by_asc(product::Column::Name),
    };
    query.order_by_asc(product::Column::Id)
}

/// A product's numeric sort price: the regular USD price, falling back to the foil
/// price, each NULL/empty-guarded so `''` isn't treated as `0` and truly-unpriced
/// products resolve to NULL (parked last by `NULLS LAST`). Column names are fixed —
/// never user input.
///
/// Mirrors [`crate::handlers::shared::sort::price_real_expr`]: SQLite's CAST coerces
/// junk to `0.0`, so it keeps the historical inverse null/empty guard (byte-identical
/// output); Postgres's CAST hard-errors on a non-decimal string, so its arm guards the
/// value with the decimal-shape check (`Dialect::decimal_string_guard`) before casting.
fn product_price_expr(dialect: Dialect) -> SimpleExpr {
    let arm = |col: &str| match dialect {
        Dialect::Sqlite => {
            format!("CASE WHEN {col} IS NULL OR {col} = '' THEN NULL ELSE CAST({col} AS REAL) END")
        }
        Dialect::Postgres => format!(
            "CASE WHEN {} THEN CAST({col} AS REAL) ELSE NULL END",
            dialect.decimal_string_guard(col)
        ),
    };
    Expr::cust(format!(
        "COALESCE({}, {})",
        arm("price_usd"),
        arm("price_usd_foil")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_sort_parses_and_defaults() {
        assert_eq!(ProductSort::parse("name").unwrap(), ProductSort::Name);
        assert_eq!(ProductSort::parse("price").unwrap(), ProductSort::Price);
        assert_eq!(ProductSort::parse("released").unwrap(), ProductSort::Released);
        assert!(ProductSort::parse("nope").is_err());
        assert_eq!(ProductSort::Name.default_dir(), SortDir::Asc);
        assert_eq!(ProductSort::Price.default_dir(), SortDir::Desc);
    }

    #[test]
    fn cdn_url_maps_sizes() {
        assert_eq!(
            product_cdn_url("12345", "normal"),
            "https://tcgplayer-cdn.tcgplayer.com/product/12345_in_1000x1000.jpg"
        );
        assert_eq!(
            product_cdn_url("12345", "small"),
            "https://tcgplayer-cdn.tcgplayer.com/product/12345_200w.jpg"
        );
        assert!(is_allowed_image_url(&product_cdn_url("12345", "normal")));
    }

    #[test]
    fn normalize_size_allowlists() {
        assert_eq!(normalize_product_size(Some("small")), "small");
        assert_eq!(normalize_product_size(Some("../x")), "normal");
        assert_eq!(normalize_product_size(None), "normal");
    }

    fn sealed_row(card_id: i32, membership: &str, foil: bool) -> (i32, String, bool) {
        (card_id, membership.to_string(), foil)
    }

    #[test]
    fn best_memberships_picks_strongest_and_ands_foil() {
        // Card 1: a non-foil "contains" outranks a foil "booster" (guaranteed wins, and
        // the non-foil resets the foil flag). Card 2: two foil "booster" rows stay
        // foil-only. Card 3: one foil + one non-foil "booster" is not foil-only.
        let rows = [
            sealed_row(1, "booster", true),
            sealed_row(1, "contains", false),
            sealed_row(2, "booster", true),
            sealed_row(2, "booster", true),
            sealed_row(3, "booster", true),
            sealed_row(3, "booster", false),
        ];
        let best = best_memberships(&rows);
        assert_eq!(best[&1], (0, "contains".to_string(), false));
        assert_eq!(best[&2], (1, "booster".to_string(), true));
        assert_eq!(best[&3], (1, "booster".to_string(), false));
    }

    #[test]
    fn best_memberships_is_order_independent_for_the_chosen_bucket() {
        // Same facts as card 1 above but with the stronger row seen first: the result is
        // identical (the foil accumulator is reset when the stronger membership arrives,
        // then ANDed across its own rows regardless of visitation order).
        let a = best_memberships(&[
            sealed_row(1, "contains", true),
            sealed_row(1, "booster", false),
            sealed_row(1, "contains", false),
        ]);
        assert_eq!(a[&1], (0, "contains".to_string(), false));
    }

    #[test]
    fn cn_int_key_parks_nulls_last() {
        assert!(cn_int_key(Some(5)) < cn_int_key(None));
        assert!(cn_int_key(Some(2)) < cn_int_key(Some(10)));
    }
}
