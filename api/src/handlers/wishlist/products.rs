//! Wish-list sealed-product routes backed by the shared product-holding engine.

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    SelectTwo, Set,
};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::entities::prelude::{Product, WishlistProductItem};
use crate::entities::{product, wishlist_product_item};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
#[cfg(test)]
use crate::handlers::shared::product_holdings::summarize_product_rows;
use crate::handlers::shared::product_holdings::{
    ProductHoldingEntry, ProductHoldingListParams, ProductHoldingRepository, ProductHoldingRow,
    ProductHoldingSetGroup, ProductHoldingSummary, get_product_holding, list_product_holdings,
    list_product_holdings_by_set, product_holding_counts, set_product_holding,
    summarize_product_holdings,
};
use crate::handlers::shared::{
    CollectionQuantities, OwnedCountsRequest, OwnedCountsResponse, Page, SetQuantitiesRequest,
};
use crate::state::AppState;

pub(super) fn wanted_products_query(
    user_id: i32,
    game: &str,
) -> SelectTwo<wishlist_product_item::Entity, product::Entity> {
    WishlistProductItem::find()
        .find_also_related(Product)
        .filter(wishlist_product_item::Column::UserId.eq(user_id))
        .filter(wishlist_product_item::Column::Game.eq(game))
        .order_by_desc(wishlist_product_item::Column::UpdatedAt)
        .order_by_desc(wishlist_product_item::Column::Id)
}

fn row(item: wishlist_product_item::Model) -> ProductHoldingRow {
    ProductHoldingRow {
        product_id: item.product_id,
        quantity: item.quantity,
        foil_quantity: item.foil_quantity,
    }
}

struct WishlistProductRepository;

impl ProductHoldingRepository for WishlistProductRepository {
    async fn page(
        db: &DatabaseConnection,
        user_id: i32,
        game: &str,
        page: u64,
        page_size: u64,
    ) -> Result<(u64, Vec<(ProductHoldingRow, Option<product::Model>)>), AppError> {
        let paginator = wanted_products_query(user_id, game).paginate(db, page_size);
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
        Ok(wanted_products_query(user_id, game)
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
        Ok(WishlistProductItem::find()
            .filter(wishlist_product_item::Column::UserId.eq(user_id))
            .filter(wishlist_product_item::Column::Game.eq(game))
            .filter(wishlist_product_item::Column::ProductId.eq(product_id))
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
        Ok(WishlistProductItem::find()
            .filter(wishlist_product_item::Column::UserId.eq(user_id))
            .filter(wishlist_product_item::Column::Game.eq(game))
            .filter(wishlist_product_item::Column::ProductId.is_in(product_ids))
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
        WishlistProductItem::delete_many()
            .filter(wishlist_product_item::Column::UserId.eq(user_id))
            .filter(wishlist_product_item::Column::Game.eq(game))
            .filter(wishlist_product_item::Column::ProductId.eq(product_id))
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
        WishlistProductItem::insert(wishlist_product_item::ActiveModel {
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
                wishlist_product_item::Column::UserId,
                wishlist_product_item::Column::Game,
                wishlist_product_item::Column::ProductId,
            ])
            .update_columns([
                wishlist_product_item::Column::Quantity,
                wishlist_product_item::Column::FoilQuantity,
                wishlist_product_item::Column::UpdatedAt,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) async fn product_summary(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<ProductHoldingSummary, AppError> {
    let rows =
        <WishlistProductRepository as ProductHoldingRepository>::all(db, user_id, game).await?;
    Ok(summarize_product_rows(rows))
}

/// List wanted sealed products
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/products",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number"),
        ("page_size" = Option<u64>, Query, description = "Rows per page (clamped)"),
    ),
    responses(
        (status = 200, description = "A page of the signed-in user's wanted sealed products.", body = Page<ProductHoldingEntry>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_wishlist_products(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ProductHoldingListParams>,
) -> Result<Json<Page<ProductHoldingEntry>>, AppError> {
    Ok(Json(
        list_product_holdings::<WishlistProductRepository>(&state, user.id, &game, params).await?,
    ))
}

/// List wanted sealed products by set
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/products/by-set",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("page" = Option<u64>, Query, description = "1-based page number (paginated by set)"),
        ("page_size" = Option<u64>, Query, description = "Sets per page (clamped)"),
    ),
    responses(
        (status = 200, description = "A page of the user's wanted sealed products grouped by set, newest set first.", body = Page<ProductHoldingSetGroup>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_wishlist_products_by_set(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ProductHoldingListParams>,
) -> Result<Json<Page<ProductHoldingSetGroup>>, AppError> {
    Ok(Json(
        list_product_holdings_by_set::<WishlistProductRepository>(&state, user.id, &game, params)
            .await?,
    ))
}

/// Get wish list sealed summary
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/products/summary",
    tag = "Wish list",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "Aggregate stats for the user's wanted sealed products.", body = ProductHoldingSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn wishlist_product_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<ProductHoldingSummary>, AppError> {
    Ok(Json(
        summarize_product_holdings::<WishlistProductRepository>(&state, user.id, &game).await?,
    ))
}

/// Batch wanted product counts
#[utoipa::path(
    post,
    path = "/api/wishlist/{game}/products/counts",
    tag = "Wish list",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    request_body = OwnedCountsRequest,
    responses(
        (status = 200, description = "Wanted counts for requested external product ids; unwanted ids are absent.", body = OwnedCountsResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "More than the per-request id cap was requested."),
    ),
)]
pub async fn wishlist_product_counts(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    Ok(Json(
        product_holding_counts::<WishlistProductRepository>(&state, user.id, &game, payload)
            .await?,
    ))
}

/// Get wish list product counts
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/products/{id}",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External (TCGplayer) product id"),
    ),
    responses(
        (status = 200, description = "How many of the product the user wants (zeros if none).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or product."),
    ),
)]
pub async fn get_wishlist_product_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CollectionQuantities>, AppError> {
    Ok(Json(
        get_product_holding::<WishlistProductRepository>(&state, user.id, &game, &id).await?,
    ))
}

/// Update wish list product counts
#[utoipa::path(
    put,
    path = "/api/wishlist/{game}/products/{id}",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External (TCGplayer) product id"),
    ),
    request_body = SetQuantitiesRequest,
    responses(
        (status = 200, description = "The resulting wanted counts (both zero removes the holding).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "A read-scoped API key cannot write."),
        (status = 404, description = "Unknown game or product."),
        (status = 422, description = "A negative or oversized count."),
    ),
)]
pub async fn set_wishlist_product_entry(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    Ok(Json(
        set_product_holding::<WishlistProductRepository>(&state, user.id, &game, &id, payload)
            .await?,
    ))
}
