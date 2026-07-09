//! Wish-list write endpoint: set the absolute wanted counts for one card (both-zero
//! deletes the row).

use axum::{
    Json,
    extract::State,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::auth::extractor::WritableUser;
use crate::entities::prelude::WishlistItem;
use crate::entities::wishlist_item;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{
    CollectionQuantities, SetQuantitiesRequest, load_card, require_game, validate_quantity,
};
use crate::state::AppState;

/// Update wish list card
///
/// `PUT /api/wishlist/{game}/cards/{id}` -> set the wanted counts for one card
/// (absolute values, not a delta). Both zero removes the card from the wish list.
/// Returns the resulting counts. `404` for an unknown game/card, `422` for a
/// negative or oversized count.
#[utoipa::path(
    put,
    path = "/api/wishlist/{game}/cards/{id}",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
    ),
    request_body = SetQuantitiesRequest,
    responses(
        (status = 200, description = "The resulting wanted counts (both zero removes the holding).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "A read-scoped API key cannot write."),
        (status = 404, description = "Unknown game or card."),
        (status = 422, description = "A negative or oversized count."),
    ),
)]
pub async fn set_wishlist_entry(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let card = load_card(&state, &game, &id).await?;

    // Wanting zero of both is "not on the wish list": drop the row by key if present.
    if quantity == 0 && foil_quantity == 0 {
        WishlistItem::delete_many()
            .filter(wishlist_item::Column::UserId.eq(user.id))
            .filter(wishlist_item::Column::Game.eq(game.as_str()))
            .filter(wishlist_item::Column::CardId.eq(card.id))
            .exec(&state.db)
            .await?;
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    let now = Utc::now();
    let active = wishlist_item::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        card_id: Set(card.id),
        quantity: Set(quantity),
        foil_quantity: Set(foil_quantity),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    // Upsert on the unique (user, game, card) index so a concurrent first-add can't
    // abort on a unique violation — the row is created or updated atomically either way.
    // `created_at` stays out of the update set, so it's preserved when the row exists.
    WishlistItem::insert(active)
        .on_conflict(
            OnConflict::columns([
                wishlist_item::Column::UserId,
                wishlist_item::Column::Game,
                wishlist_item::Column::CardId,
            ])
            .update_columns([
                wishlist_item::Column::Quantity,
                wishlist_item::Column::FoilQuantity,
                wishlist_item::Column::UpdatedAt,
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
