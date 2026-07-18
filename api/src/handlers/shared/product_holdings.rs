//! Shared sealed-product holding engine for the collection and wish list.
//!
//! Both surfaces store `(user, game, product) -> { quantity, foil_quantity }` in
//! independent tables and expose the same list/summary/counts/entry contract. Concrete
//! SeaORM queries stay in each handler module through [`ProductHoldingRepository`]; all
//! validation, external-id resolution, wire shaping, pagination, and valuation live here.

use std::cmp::Ordering;
use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::entities::prelude::{CardSet, Product};
use crate::entities::{card_set, product};
use crate::error::AppError;
use crate::state::AppState;

use super::holdings::{
    CollectionQuantities, MAX_OWNED_IDS, OwnedCountsRequest, OwnedCountsResponse,
    SetQuantitiesRequest, dedupe_ids, validate_quantity,
};
use super::lookup::require_game;
use super::pagination::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, Page, build_page, resolve_page};
use super::valuation::Valuation;

/// A sealed product's market prices (USD only — TCGCSV carries no eur/tix).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductPrices"))]
pub(crate) struct ProductPricesResponse {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

/// A sealed product, as the SPA sees it. Mirrors the `Card` DTO idioms: the provider
/// id is exposed as a string `id`, prices are nested, and images are fetched through
/// the proxy (`has_image` says whether one is available).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
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
    /// Manufacturer's suggested retail price (USD), as a decimal string, or `None` when
    /// unknown. A **retail list** price curated from WotC announcements (no feed carries
    /// it) — kept separate from the TCGCSV *market* prices in `prices`. The SPA hides the
    /// MSRP line when this is absent.
    pub msrp: Option<String>,
    pub released_at: Option<String>,
}

/// One held sealed product: the public product payload plus the caller's counts.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct ProductHoldingEntry {
    pub product: ProductResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Aggregate stats for one surface's sealed products in a game.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct ProductHoldingSummary {
    pub unique_products: i64,
    pub total_products: i64,
    pub total_value_usd: Option<String>,
}

/// One set's slice of a user's sealed-product holding: the set identity, its
/// aggregate stats, and every held product in it. The enclosing Page paginates
/// over these groups (`total` counts sets, not products).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct ProductHoldingSetGroup {
    pub code: String,
    /// The set's display name, or None when the product's group has no matching catalog set.
    pub name: Option<String>,
    pub unique_products: i64,
    /// `quantity + foil_quantity` summed over the group.
    pub total_products: i64,
    /// The group's total market value (regular + foil), or None when nothing held is priced.
    pub total_value_usd: Option<String>,
    pub products: Vec<ProductHoldingEntry>,
}

/// Page and page-size query parameters for a fixed recency-sorted product holding list.
#[derive(Debug, Deserialize)]
pub(crate) struct ProductHoldingListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Entity-neutral fields from a collection/wish-list sealed-product row.
#[derive(Clone, Debug)]
pub(crate) struct ProductHoldingRow {
    pub product_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// The table-specific persistence operations behind the shared handler engine.
pub(crate) trait ProductHoldingRepository {
    async fn page(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        page: u64,
        page_size: u64,
    ) -> Result<(u64, Vec<(ProductHoldingRow, Option<product::Model>)>), AppError>;

    async fn all(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
    ) -> Result<Vec<(ProductHoldingRow, Option<product::Model>)>, AppError>;

    async fn find(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
    ) -> Result<Option<ProductHoldingRow>, AppError>;

    async fn counts(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_ids: Vec<i32>,
    ) -> Result<Vec<ProductHoldingRow>, AppError>;

    async fn delete(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
    ) -> Result<(), AppError>;

    async fn upsert(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
        quantity: i32,
        foil_quantity: i32,
    ) -> Result<(), AppError>;
}

/// Resolve a product by external provider id within one game.
pub(crate) async fn load_product(
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

/// The game's set-code to `(display-name, release-date)` map. The by-set grouping needs
/// both halves (the name to label a group, the date to order the groups), so they're
/// fetched together; [`set_name_map`] projects out the name half for the many callers that
/// only dress product payloads. `released_at` is the catalog set's ISO `YYYY-MM-DD` string
/// (sortable as text), `None` when the provider reported no date.
pub(crate) async fn set_meta_map(
    state: &AppState,
    game: &str,
) -> Result<HashMap<String, (String, Option<String>)>, AppError> {
    let rows: Vec<(String, String, Option<String>)> = CardSet::find()
        .select_only()
        .column(card_set::Column::Code)
        .column(card_set::Column::Name)
        .column(card_set::Column::ReleasedAt)
        .filter(card_set::Column::Game.eq(game))
        .into_tuple()
        .all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(code, name, released_at)| (code, (name, released_at)))
        .collect())
}

/// The game's set-code to display-name map used to dress product payloads — the name-only
/// projection of [`set_meta_map`], so payload dressing and by-set ordering share one query
/// definition.
pub(crate) async fn set_name_map(
    state: &AppState,
    game: &str,
) -> Result<HashMap<String, String>, AppError> {
    Ok(set_meta_map(state, game)
        .await?
        .into_iter()
        .map(|(code, (name, _))| (code, name))
        .collect())
}

/// Build the public product DTO, resolving its set name when one exists.
pub(crate) fn product_response(
    p: product::Model,
    names: &HashMap<String, String>,
) -> ProductResponse {
    let set_name = names.get(&p.set_code).cloned();
    ProductResponse {
        id: p.external_id,
        name: p.name,
        set_name,
        set_code: p.set_code,
        product_type: p.product_type,
        url: p.url,
        has_image: p.image_url.is_some(),
        prices: ProductPricesResponse {
            usd: p.price_usd,
            usd_foil: p.price_usd_foil,
        },
        msrp: p.msrp,
        released_at: p.released_at,
    }
}

fn quantities(row: Option<ProductHoldingRow>) -> CollectionQuantities {
    row.map_or(
        CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        },
        |row| CollectionQuantities {
            quantity: row.quantity,
            foil_quantity: row.foil_quantity,
        },
    )
}

pub(crate) fn summarize_product_rows(
    rows: Vec<(ProductHoldingRow, Option<product::Model>)>,
) -> ProductHoldingSummary {
    let mut unique_products = 0;
    let mut total_products = 0;
    let mut valuation = Valuation::default();
    for (item, product) in rows {
        let Some(product) = product else { continue };
        unique_products += 1;
        total_products += i64::from(item.quantity) + i64::from(item.foil_quantity);
        valuation.add(
            product.price_usd.as_deref(),
            item.quantity,
            product.price_usd_foil.as_deref(),
            item.foil_quantity,
        );
    }
    ProductHoldingSummary {
        unique_products,
        total_products,
        total_value_usd: valuation.total_usd(),
    }
}

pub(crate) async fn list_product_holdings<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
    params: ProductHoldingListParams,
) -> Result<Page<ProductHoldingEntry>, AppError> {
    require_game(game)?;
    let (page, page_size) = resolve_page(
        params.page,
        params.page_size,
        DEFAULT_PAGE_SIZE,
        MAX_PAGE_SIZE,
    );
    let (total, rows) = R::page(&state.db, user_id, game, page, page_size).await?;
    let names = set_name_map(state, game).await?;
    let data = rows
        .into_iter()
        .filter_map(|(item, product)| {
            product.map(|product| ProductHoldingEntry {
                product: product_response(product, &names),
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();
    Ok(build_page(data, page, page_size, total))
}

/// Group a surface's sealed-product holdings by set, newest set first.
///
/// The by-set companion to [`list_product_holdings`]: it pulls *every* holding via
/// `R::all` (they're few — the same all-rows fetch [`summarize_product_holdings`] does),
/// buckets them by `set_code`, and paginates over the **groups**, so `Page::total` counts
/// sets rather than products. Each group carries the same aggregate stats
/// [`summarize_product_rows`] computes, scoped to that set, plus its held products.
///
/// Group order is newest set first: a set's own catalog `released_at` when it has a
/// `card_sets` row, else the newest `released_at` among the products held in it (so a set
/// with no catalog row still sorts by real dates rather than sinking on identity alone);
/// date-less groups sort last, ties broken by set code ascending. Within a group products
/// sort by name case-insensitively, then external id — a stable order independent of how
/// `R::all` fetched the rows.
pub(crate) async fn list_product_holdings_by_set<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
    params: ProductHoldingListParams,
) -> Result<Page<ProductHoldingSetGroup>, AppError> {
    require_game(game)?;
    let meta = set_meta_map(state, game).await?;
    // The name-only view `product_response` expects, projected from the same fetch (no
    // second query) — every product in a group resolves to that group's set name.
    let names: HashMap<String, String> = meta
        .iter()
        .map(|(code, (name, _))| (code.clone(), name.clone()))
        .collect();

    // Bucket every held product by set code; holdings whose product row is gone are
    // skipped, exactly as the flat list does.
    let mut buckets: HashMap<String, Vec<(ProductHoldingRow, product::Model)>> = HashMap::new();
    for (item, product) in R::all(&state.db, user_id, game).await? {
        let Some(product) = product else { continue };
        buckets
            .entry(product.set_code.clone())
            .or_default()
            .push((item, product));
    }

    // Shape each bucket into its group DTO plus the date the group orders on. The
    // `order_date` rides alongside only for the sort below; it never reaches the wire.
    let mut groups: Vec<(Option<String>, ProductHoldingSetGroup)> = buckets
        .into_iter()
        .map(|(code, mut rows)| {
            rows.sort_by(|(_, a), (_, b)| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.external_id.cmp(&b.external_id))
            });

            let (name, order_date) = match meta.get(&code) {
                // A known set labels the group and orders by its own release date (which
                // may itself be absent — then the group is date-less).
                Some((name, released_at)) => (Some(name.clone()), released_at.clone()),
                // An unknown set (no `card_sets` row) has no name; fall back to the newest
                // release date among the products held in it.
                None => (
                    None,
                    rows.iter().filter_map(|(_, p)| p.released_at.clone()).max(),
                ),
            };

            let unique_products = rows.len() as i64;
            let mut total_products = 0i64;
            let mut valuation = Valuation::default();
            for (item, product) in &rows {
                total_products += i64::from(item.quantity) + i64::from(item.foil_quantity);
                valuation.add(
                    product.price_usd.as_deref(),
                    item.quantity,
                    product.price_usd_foil.as_deref(),
                    item.foil_quantity,
                );
            }
            let products = rows
                .into_iter()
                .map(|(item, product)| ProductHoldingEntry {
                    product: product_response(product, &names),
                    quantity: item.quantity,
                    foil_quantity: item.foil_quantity,
                })
                .collect();

            let group = ProductHoldingSetGroup {
                code,
                name,
                unique_products,
                total_products,
                total_value_usd: valuation.total_usd(),
                products,
            };
            (order_date, group)
        })
        .collect();

    // Newest set first; date-less groups sink last; ties (or two date-less groups) break by
    // code ascending, so the order is fully deterministic.
    groups.sort_by(|(a_date, a), (b_date, b)| match (a_date, b_date) {
        (Some(a_date), Some(b_date)) => b_date.cmp(a_date).then_with(|| a.code.cmp(&b.code)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.code.cmp(&b.code),
    });

    // Paginate over the groups in memory (bounded set count), mirroring the flat list's
    // clamp/`has_more` semantics via the shared helpers.
    let (page, page_size) = resolve_page(
        params.page,
        params.page_size,
        DEFAULT_PAGE_SIZE,
        MAX_PAGE_SIZE,
    );
    let total = groups.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let data = groups
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(|(_, group)| group)
        .collect();
    Ok(build_page(data, page, page_size, total))
}

pub(crate) async fn summarize_product_holdings<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<ProductHoldingSummary, AppError> {
    require_game(game)?;
    Ok(summarize_product_rows(
        R::all(&state.db, user_id, game).await?,
    ))
}

pub(crate) async fn product_holding_counts<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
    payload: OwnedCountsRequest,
) -> Result<OwnedCountsResponse, AppError> {
    require_game(game)?;
    let external_ids = dedupe_ids(payload.ids);
    if external_ids.is_empty() {
        return Ok(OwnedCountsResponse {
            data: HashMap::new(),
        });
    }
    if external_ids.len() > MAX_OWNED_IDS {
        return Err(AppError::Validation(format!(
            "at most {MAX_OWNED_IDS} product ids may be looked up at once"
        )));
    }

    let external_by_internal: HashMap<i32, String> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::ExternalId)
        .filter(product::Column::Game.eq(game))
        .filter(product::Column::ExternalId.is_in(external_ids))
        .into_tuple::<(i32, String)>()
        .all(&state.db)
        .await?
        .into_iter()
        .collect();
    if external_by_internal.is_empty() {
        return Ok(OwnedCountsResponse {
            data: HashMap::new(),
        });
    }

    let rows = R::counts(
        &state.db,
        user_id,
        game,
        external_by_internal.keys().copied().collect(),
    )
    .await?;
    let data = rows
        .into_iter()
        .filter_map(|row| {
            external_by_internal.get(&row.product_id).map(|id| {
                (
                    id.clone(),
                    CollectionQuantities {
                        quantity: row.quantity,
                        foil_quantity: row.foil_quantity,
                    },
                )
            })
        })
        .collect();
    Ok(OwnedCountsResponse { data })
}

pub(crate) async fn get_product_holding<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
    id: &str,
) -> Result<CollectionQuantities, AppError> {
    require_game(game)?;
    let product = load_product(state, game, id).await?;
    Ok(quantities(
        R::find(&state.db, user_id, game, product.id).await?,
    ))
}

pub(crate) async fn set_product_holding<R: ProductHoldingRepository>(
    state: &AppState,
    user_id: i32,
    game: &str,
    id: &str,
    payload: SetQuantitiesRequest,
) -> Result<CollectionQuantities, AppError> {
    require_game(game)?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let product = load_product(state, game, id).await?;
    if quantity == 0 && foil_quantity == 0 {
        R::delete(&state.db, user_id, game, product.id).await?;
    } else {
        R::upsert(
            &state.db,
            user_id,
            game,
            product.id,
            quantity,
            foil_quantity,
        )
        .await?;
    }
    Ok(CollectionQuantities {
        quantity,
        foil_quantity,
    })
}
