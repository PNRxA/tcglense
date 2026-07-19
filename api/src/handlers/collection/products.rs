//! Collection sealed-product routes backed by the shared product-holding engine.

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    SelectTwo, Set,
};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::entities::prelude::{CollectionProductItem, Product};
use crate::entities::{collection_product_item, product};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::product_holdings::{
    ProductHoldingEntry, ProductHoldingListParams, ProductHoldingRepository, ProductHoldingRow,
    ProductHoldingSet, ProductHoldingSummary, get_product_holding, list_product_holding_sets,
    list_product_holdings, product_holding_counts, set_product_holding, summarize_product_holdings,
};
use crate::handlers::shared::{
    CollectionQuantities, DataBody, OwnedCountsRequest, OwnedCountsResponse, Page,
    SetQuantitiesRequest,
};
use crate::state::AppState;

fn owned_products_query(
    user_id: i32,
    game: &str,
) -> SelectTwo<collection_product_item::Entity, product::Entity> {
    CollectionProductItem::find()
        .find_also_related(Product)
        .filter(collection_product_item::Column::UserId.eq(user_id))
        .filter(collection_product_item::Column::Game.eq(game))
        .order_by_desc(collection_product_item::Column::UpdatedAt)
        .order_by_desc(collection_product_item::Column::Id)
}

fn row(item: collection_product_item::Model) -> ProductHoldingRow {
    ProductHoldingRow {
        product_id: item.product_id,
        quantity: item.quantity,
        foil_quantity: item.foil_quantity,
    }
}

struct CollectionProductRepository;

impl ProductHoldingRepository for CollectionProductRepository {
    async fn page(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        set: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> Result<(u64, Vec<(ProductHoldingRow, Option<product::Model>)>), AppError> {
        let mut query = owned_products_query(user_id, game);
        if let Some(set) = set {
            query = query.filter(product::Column::SetCode.eq(set));
        }
        let paginator = query.paginate(db, page_size);
        let total = paginator.num_items().await?;
        let rows = paginator
            .fetch_page(page - 1)
            .await?
            .into_iter()
            .map(|(item, product)| (row(item), product))
            .collect();
        Ok((total, rows))
    }

    async fn all(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
    ) -> Result<Vec<(ProductHoldingRow, Option<product::Model>)>, AppError> {
        Ok(owned_products_query(user_id, game)
            .all(db)
            .await?
            .into_iter()
            .map(|(item, product)| (row(item), product))
            .collect())
    }

    async fn find(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
    ) -> Result<Option<ProductHoldingRow>, AppError> {
        Ok(CollectionProductItem::find()
            .filter(collection_product_item::Column::UserId.eq(user_id))
            .filter(collection_product_item::Column::Game.eq(game))
            .filter(collection_product_item::Column::ProductId.eq(product_id))
            .one(db)
            .await?
            .map(row))
    }

    async fn counts(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_ids: Vec<i32>,
    ) -> Result<Vec<ProductHoldingRow>, AppError> {
        Ok(CollectionProductItem::find()
            .filter(collection_product_item::Column::UserId.eq(user_id))
            .filter(collection_product_item::Column::Game.eq(game))
            .filter(collection_product_item::Column::ProductId.is_in(product_ids))
            .all(db)
            .await?
            .into_iter()
            .map(row)
            .collect())
    }

    async fn delete(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
    ) -> Result<(), AppError> {
        CollectionProductItem::delete_many()
            .filter(collection_product_item::Column::UserId.eq(user_id))
            .filter(collection_product_item::Column::Game.eq(game))
            .filter(collection_product_item::Column::ProductId.eq(product_id))
            .exec(db)
            .await?;
        Ok(())
    }

    async fn upsert(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        product_id: i32,
        quantity: i32,
        foil_quantity: i32,
    ) -> Result<(), AppError> {
        let now = Utc::now();
        CollectionProductItem::insert(collection_product_item::ActiveModel {
            user_id: Set(user_id),
            game: Set(game.to_string()),
            product_id: Set(product_id),
            quantity: Set(quantity),
            foil_quantity: Set(foil_quantity),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::columns([
                collection_product_item::Column::UserId,
                collection_product_item::Column::Game,
                collection_product_item::Column::ProductId,
            ])
            .update_columns([
                collection_product_item::Column::Quantity,
                collection_product_item::Column::FoilQuantity,
                collection_product_item::Column::UpdatedAt,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
}

// ---- Public-sharing read cores ----
//
// The `user_id`-parameterised sealed-product reads reused by the public collection mirror
// (`crate::handlers::sharing::public`), so a public read shares the exact query/valuation
// logic with the authed handlers — only how `user_id` is resolved differs (a resolved handle
// vs. the caller's token). Kept here (not in the shared engine) so `CollectionProductRepository`
// stays private to this module, mirroring how `read`/`sets` expose card cores to public.rs.

pub(crate) async fn owned_products_page(
    state: &AppState,
    user_id: i32,
    game: &str,
    params: ProductHoldingListParams,
) -> Result<Page<ProductHoldingEntry>, AppError> {
    list_product_holdings::<CollectionProductRepository>(state, user_id, game, params).await
}

pub(crate) async fn owned_product_sets(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<Vec<ProductHoldingSet>, AppError> {
    list_product_holding_sets::<CollectionProductRepository>(state, user_id, game).await
}

pub(crate) async fn owned_product_summary(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<ProductHoldingSummary, AppError> {
    summarize_product_holdings::<CollectionProductRepository>(state, user_id, game).await
}

/// List owned sealed products
#[utoipa::path(
    get,
    path = "/api/collection/{game}/products",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
        ("set" = Option<String>, Query, description = "Restrict to one set code; an unknown/unheld code yields an empty page"),
    ),
    responses(
        (status = 200, description = "A page of the signed-in user's owned sealed products.", body = Page<ProductHoldingEntry>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_collection_products(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ProductHoldingListParams>,
) -> Result<Json<Page<ProductHoldingEntry>>, AppError> {
    Ok(Json(
        list_product_holdings::<CollectionProductRepository>(&state, user.id, &game, params)
            .await?,
    ))
}

/// List owned product sets
#[utoipa::path(
    get,
    path = "/api/collection/{game}/products/sets",
    tag = "Collection",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "Every set the user owns sealed products in, newest set first, each an aggregate tile.", body = DataBody<Vec<ProductHoldingSet>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_collection_product_sets(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<ProductHoldingSet>>>, AppError> {
    Ok(Json(DataBody {
        data: list_product_holding_sets::<CollectionProductRepository>(&state, user.id, &game)
            .await?,
    }))
}

/// Get collection sealed summary
#[utoipa::path(
    get,
    path = "/api/collection/{game}/products/summary",
    tag = "Collection",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "Aggregate stats for the user's owned sealed products.", body = ProductHoldingSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn collection_product_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<ProductHoldingSummary>, AppError> {
    Ok(Json(
        summarize_product_holdings::<CollectionProductRepository>(&state, user.id, &game).await?,
    ))
}

/// Batch owned product counts
#[utoipa::path(
    post,
    path = "/api/collection/{game}/products/owned",
    tag = "Collection",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    request_body = OwnedCountsRequest,
    responses(
        (status = 200, description = "Owned counts for requested external product ids; unowned ids are absent.", body = OwnedCountsResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "More than the per-request id cap was requested."),
    ),
)]
pub async fn collection_product_counts(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    Ok(Json(
        product_holding_counts::<CollectionProductRepository>(&state, user.id, &game, payload)
            .await?,
    ))
}

/// Get collection product counts
#[utoipa::path(
    get,
    path = "/api/collection/{game}/products/{id}",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External (TCGplayer) product id"),
    ),
    responses(
        (status = 200, description = "How many of the product the user owns (zeros if none).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or product."),
    ),
)]
pub async fn get_collection_product_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CollectionQuantities>, AppError> {
    Ok(Json(
        get_product_holding::<CollectionProductRepository>(&state, user.id, &game, &id).await?,
    ))
}

/// Update collection product counts
#[utoipa::path(
    put,
    path = "/api/collection/{game}/products/{id}",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External (TCGplayer) product id"),
    ),
    request_body = SetQuantitiesRequest,
    responses(
        (status = 200, description = "The resulting owned counts (both zero removes the holding).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "A read-scoped API key cannot write."),
        (status = 404, description = "Unknown game or product."),
        (status = 422, description = "A negative or oversized count."),
    ),
)]
pub async fn set_collection_product_entry(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    let quantities =
        set_product_holding::<CollectionProductRepository>(&state, user.id, &game, &id, payload)
            .await?;
    // Collection analytics include sealed products: orphan the user's cached
    // analytics bodies (#413). The wishlist twin has no analytics, so its
    // handler deliberately has no bump.
    state.analytics_cache.bump_holdings(user.id, &game).await;
    Ok(Json(quantities))
}
