//! Collection write endpoint: set the absolute owned counts for one card (both-zero
//! deletes the holding).

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, Set, SqlErr};

use crate::auth::extractor::AuthUser;
use crate::entities::collection_item;
use crate::entities::prelude::CollectionItem;
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::handlers::shared::{load_card, require_game};
use crate::state::AppState;

use super::{CollectionQuantities, MAX_QUANTITY, SetQuantitiesRequest, find_row};

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

pub(super) fn validate_quantity(value: i32, field: &str) -> Result<i32, AppError> {
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
