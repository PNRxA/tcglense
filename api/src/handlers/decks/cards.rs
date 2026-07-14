//! Deck-card endpoints: set a card's absolute counts within a section (both zero removes
//! it), and move a card between sections (merging on a collision). Writes take
//! [`WritableUser`]; card ids in the path are the external id, resolved to the internal
//! `cards.id` on write.

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::auth::extractor::WritableUser;
use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::deck_card;
use crate::entities::prelude::DeckCard;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{CollectionQuantities, load_card, require_game, validate_quantity};
use crate::state::AppState;

use super::{MoveDeckCardRequest, SetDeckCardRequest, load_deck, load_section, touch_deck};

/// Set deck card
///
/// `PUT /api/decks/{game}/{deck_id}/cards/{id}` -> set the absolute counts for a card in
/// one of the deck's sections (not a delta). Both zero removes it from that section.
/// `404` for an unknown deck/section/card, `422` for a negative/oversized count.
#[utoipa::path(
    put,
    path = "/api/decks/{game}/{deck_id}/cards/{id}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
        ("id" = String, Path, description = "External card id"),
    ),
    request_body = SetDeckCardRequest,
    responses(
        (status = 200, description = "The resulting counts for the card in that section (both zero removes it).", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, deck, section, or card."),
        (status = 422, description = "A negative or oversized count."),
    ),
)]
pub async fn set_deck_card(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id, id)): Path<(String, i32, String)>,
    JsonBody(payload): JsonBody<SetDeckCardRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    // Prove the section is this deck's, and the card exists, before writing.
    let section = load_section(&state, deck.id, payload.section_id).await?;
    let card = load_card(&state, &game, &id).await?;

    let now = Utc::now();
    if quantity == 0 && foil_quantity == 0 {
        DeckCard::delete_many()
            .filter(deck_card::Column::DeckId.eq(deck.id))
            .filter(deck_card::Column::CardId.eq(card.id))
            .filter(deck_card::Column::SectionId.eq(section.id))
            .exec(&state.db)
            .await?;
        touch_deck(&state.db, deck.id, now).await?;
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    let active = deck_card::ActiveModel {
        deck_id: Set(deck.id),
        section_id: Set(section.id),
        card_id: Set(card.id),
        quantity: Set(quantity),
        foil_quantity: Set(foil_quantity),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    // Upsert on the unique (deck, card, section) index — created_at stays out of the
    // update set so it's preserved when the row already exists.
    DeckCard::insert(active)
        .on_conflict(
            OnConflict::columns([
                deck_card::Column::DeckId,
                deck_card::Column::CardId,
                deck_card::Column::SectionId,
            ])
            .update_columns([
                deck_card::Column::Quantity,
                deck_card::Column::FoilQuantity,
                deck_card::Column::UpdatedAt,
            ])
            .to_owned(),
        )
        .exec(&state.db)
        .await?;
    touch_deck(&state.db, deck.id, now).await?;

    Ok(Json(CollectionQuantities {
        quantity,
        foil_quantity,
    }))
}

/// Move deck card
///
/// `PUT /api/decks/{game}/{deck_id}/cards/{id}/move` -> move a card from one of the deck's
/// sections to another. If the target already holds the card, the counts are summed and the
/// source row removed. Returns the resulting counts in the target section. `404` if the deck,
/// either section, the card, or the card-in-`from_section` isn't found.
#[utoipa::path(
    put,
    path = "/api/decks/{game}/{deck_id}/cards/{id}/move",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
        ("id" = String, Path, description = "External card id"),
    ),
    request_body = MoveDeckCardRequest,
    responses(
        (status = 200, description = "The resulting counts for the card in the target section.", body = CollectionQuantities),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, deck, either section, the card, or the card in `from_section`."),
    ),
)]
pub async fn move_deck_card(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id, id)): Path<(String, i32, String)>,
    JsonBody(payload): JsonBody<MoveDeckCardRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let from = load_section(&state, deck.id, payload.from_section_id).await?;
    let to = load_section(&state, deck.id, payload.to_section_id).await?;
    let card = load_card(&state, &game, &id).await?;

    let source = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .filter(deck_card::Column::CardId.eq(card.id))
        .filter(deck_card::Column::SectionId.eq(from.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("card not in that section".to_string()))?;

    // No-op move: same section.
    if from.id == to.id {
        return Ok(Json(CollectionQuantities {
            quantity: source.quantity,
            foil_quantity: source.foil_quantity,
        }));
    }

    let now = Utc::now();
    let existing = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .filter(deck_card::Column::CardId.eq(card.id))
        .filter(deck_card::Column::SectionId.eq(to.id))
        .one(&state.db)
        .await?;

    let result = if let Some(target) = existing {
        // Merge into the target row, then drop the source.
        let quantity = (target.quantity + source.quantity).min(MAX_CARD_QUANTITY);
        let foil_quantity = (target.foil_quantity + source.foil_quantity).min(MAX_CARD_QUANTITY);
        let mut active: deck_card::ActiveModel = target.into();
        active.quantity = Set(quantity);
        active.foil_quantity = Set(foil_quantity);
        active.updated_at = Set(now);
        active.update(&state.db).await?;
        DeckCard::delete_by_id(source.id).exec(&state.db).await?;
        CollectionQuantities {
            quantity,
            foil_quantity,
        }
    } else {
        // Just re-file the source row into the target section.
        let quantity = source.quantity;
        let foil_quantity = source.foil_quantity;
        let mut active: deck_card::ActiveModel = source.into();
        active.section_id = Set(to.id);
        active.updated_at = Set(now);
        active.update(&state.db).await?;
        CollectionQuantities {
            quantity,
            foil_quantity,
        }
    };
    touch_deck(&state.db, deck.id, now).await?;

    Ok(Json(result))
}
