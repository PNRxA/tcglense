//! Shared sealed-product holding engine for the collection and wish list.
//!
//! Both surfaces store `(user, game, product) -> { quantity, foil_quantity }` in
//! independent tables and expose the same list/summary/counts/entry contract. Concrete
//! SeaORM queries stay in each handler module through [`ProductHoldingRepository`]; all
//! validation, external-id resolution, wire shaping, pagination, and valuation live here.

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

/// The game's set-code to display-name map used to dress product payloads.
pub(crate) async fn set_name_map(
    state: &AppState,
    game: &str,
) -> Result<HashMap<String, String>, AppError> {
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
