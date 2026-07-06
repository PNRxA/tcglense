//! Collection write endpoint: set the absolute owned counts for one card (both-zero
//! deletes the holding).

use axum::{
    Json,
    extract::State,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::auth::extractor::AuthUser;
use crate::entities::collection_item;
use crate::entities::prelude::CollectionItem;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{load_card, require_game, validate_quantity};
use crate::state::AppState;

use super::{CollectionQuantities, SetQuantitiesRequest};

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

    // Owning zero of both is "not in the collection": drop the row by key if present.
    if quantity == 0 && foil_quantity == 0 {
        CollectionItem::delete_many()
            .filter(collection_item::Column::UserId.eq(user.id))
            .filter(collection_item::Column::Game.eq(game.as_str()))
            .filter(collection_item::Column::CardId.eq(card.id))
            .exec(&state.db)
            .await?;
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    let now = Utc::now();
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
    // Upsert on the unique (user, game, card) index so a concurrent first-add can't
    // abort on a unique violation — the row is created or updated atomically either way.
    // `created_at` stays out of the update set, so it's preserved when the row exists.
    CollectionItem::insert(active)
        .on_conflict(
            OnConflict::columns([
                collection_item::Column::UserId,
                collection_item::Column::Game,
                collection_item::Column::CardId,
            ])
            .update_columns([
                collection_item::Column::Quantity,
                collection_item::Column::FoilQuantity,
                collection_item::Column::UpdatedAt,
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
