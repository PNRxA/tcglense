//! Wish-list **sealed-product** endpoints (issue #364): the wanted-products list, one
//! product's wanted counts, and the wanted-count upsert.
//!
//! Wishlist-only — the collection deliberately has **no** sealed-product surface. Sealed
//! products live in their own `wishlist_product_items` table (the [`wishlist_item`]
//! twin, with `product_id` in place of `card_id`), so a product can be wanted
//! independently of any card. The `{id}` in the path is the provider's **external**
//! (TCGplayer) product id; it is resolved to the internal `products.id` before storage,
//! so a wish-list row survives a catalog re-import. Rows are always scoped by `user.id`
//! from the token, and — as with the card twin — both counts reaching zero deletes the
//! row.
//!
//! The wire types are the collection's own `CollectionQuantities`/`SetQuantitiesRequest`
//! (reused verbatim), plus one new DTO — [`WishlistProductEntry`] — wrapping the public
//! product payload with the wanted counts.
//!
//! [`wishlist_item`]: crate::entities::wishlist_item

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, SelectTwo, Set};
use serde::{Deserialize, Serialize};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::entities::prelude::{Product, WishlistProductItem};
use crate::entities::{product, wishlist_product_item};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::catalog::{ProductResponse, load_product, product_response, set_name_map};
use crate::handlers::shared::{
    CollectionQuantities, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, Page, SetQuantitiesRequest, build_page,
    require_game, resolve_page, validate_quantity,
};
use crate::state::AppState;

/// One wanted sealed product: the full public product payload plus the wanted counts.
/// Wish-list-only — the collection has no sealed-product holdings (issue #364).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct WishlistProductEntry {
    pub product: ProductResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Query params for the wanted-products list: page + page size only (the list is fixed
/// recency-desc). Named apart from the catalog's differently-shaped `ProductListParams`.
#[derive(Debug, Deserialize)]
pub(crate) struct WantedProductListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// The per-user wanted-products base query: every `wishlist_product_items` row for one
/// `user_id` + `game`, left-joined to its `products` row, ordered newest change first
/// with a stable `id` tiebreaker so paging is deterministic. The recency sort names only
/// `wishlist_product_items` columns, so nothing is ambiguous across the join. Kept
/// `pub(super)` so the module's unit tests can drive it against a seeded DB.
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

/// The user's wish-list row for a sealed product, if any. Shared by the get/set handlers.
async fn find_product_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    product_id: i32,
) -> Result<Option<wishlist_product_item::Model>, AppError> {
    Ok(WishlistProductItem::find()
        .filter(wishlist_product_item::Column::UserId.eq(user_id))
        .filter(wishlist_product_item::Column::Game.eq(game))
        .filter(wishlist_product_item::Column::ProductId.eq(product_id))
        .one(&state.db)
        .await?)
}

/// List wanted sealed products
///
/// `GET /api/wishlist/{game}/products` -> the signed-in user's wanted sealed products
/// for a game, most-recently-updated first, paginated. Each entry carries the full
/// product payload plus the wanted counts.
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
        (status = 200, description = "A page of the signed-in user's wanted sealed products.", body = Page<WishlistProductEntry>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_wishlist_products(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<WantedProductListParams>,
) -> Result<Json<Page<WishlistProductEntry>>, AppError> {
    require_game(&game)?;
    let (page, page_size) =
        resolve_page(params.page, params.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE);

    let paginator = wanted_products_query(user.id, &game).paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    // One set-name lookup for the whole page, then dress each row. `find_also_related` is
    // a LEFT join, so a row whose product is gone (e.g. removed by a catalog re-import)
    // comes back `None` and is skipped — the orphan-tolerant `product_id` invariant.
    let names = set_name_map(&state, &game).await?;
    let data: Vec<WishlistProductEntry> = rows
        .into_iter()
        .filter_map(|(item, prod)| {
            prod.map(|p| WishlistProductEntry {
                product: product_response(p, &names),
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// Get wish list product
///
/// `GET /api/wishlist/{game}/products/{id}` -> how many of one sealed product the user
/// wants (zeros when the product isn't on their wish list). `id` is the external
/// (TCGplayer) product id; a `404` means the game or product is unknown.
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
    require_game(&game)?;
    // 404 an unknown product; a known-but-unwanted product falls through to zeros (the
    // frontend editors seed their absolute-count steppers from this).
    let product = load_product(&state, &game, &id).await?;
    let row = find_product_row(&state, user.id, &game, product.id).await?;
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

/// Update wish list product
///
/// `PUT /api/wishlist/{game}/products/{id}` -> set the wanted counts for one sealed
/// product (absolute values, not a delta). Both zero removes the product from the wish
/// list. Returns the resulting counts. `404` for an unknown game/product, `422` for a
/// negative or oversized count.
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
    require_game(&game)?;
    // Validate the counts (422) before resolving the product (404), matching the card twin.
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let product = load_product(&state, &game, &id).await?;

    // Wanting zero of both is "not on the wish list": drop the row by key if present.
    if quantity == 0 && foil_quantity == 0 {
        WishlistProductItem::delete_many()
            .filter(wishlist_product_item::Column::UserId.eq(user.id))
            .filter(wishlist_product_item::Column::Game.eq(game.as_str()))
            .filter(wishlist_product_item::Column::ProductId.eq(product.id))
            .exec(&state.db)
            .await?;
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    let now = Utc::now();
    let active = wishlist_product_item::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        product_id: Set(product.id),
        quantity: Set(quantity),
        foil_quantity: Set(foil_quantity),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    // Upsert on the unique (user, game, product) index so a concurrent first-add can't
    // abort on a unique violation. `created_at` stays out of the update set, so it's
    // preserved when the row already exists.
    WishlistProductItem::insert(active)
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
        .exec(&state.db)
        .await?;

    Ok(Json(CollectionQuantities {
        quantity,
        foil_quantity,
    }))
}
