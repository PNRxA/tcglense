//! Deck-section endpoints: add / rename / reposition / delete / reorder the sections
//! (categories) within a deck, and move cards out of a deleted section. Writes take
//! [`WritableUser`].

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};

use crate::auth::extractor::WritableUser;
use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::prelude::{DeckCard, DeckSection};
use crate::entities::{deck_card, deck_section};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::{
    CreateSectionRequest, DeckSectionResponse, MAX_SECTION_NAME, MAX_SECTIONS_PER_DECK,
    ReorderSectionsRequest, UpdateSectionRequest, load_deck, load_section, touch_deck,
    validate_name,
};

/// `POST /api/decks/{game}/{deck_id}/sections` -> add a custom section (appended after the
/// last one). `422` for a blank/oversized name or over the per-deck cap; `409` if the deck
/// already has a section with that name.
pub async fn create_section(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<CreateSectionRequest>,
) -> Result<Json<DeckSectionResponse>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let name = validate_name(&payload.name, "name", MAX_SECTION_NAME)?;

    let count = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .count(&state.db)
        .await?;
    if count >= MAX_SECTIONS_PER_DECK {
        return Err(AppError::Validation(format!(
            "a deck can have at most {MAX_SECTIONS_PER_DECK} sections"
        )));
    }
    ensure_unique_name(&state, deck.id, &name, None).await?;

    // Append after the current last section.
    let last = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .order_by_desc(deck_section::Column::Position)
        .one(&state.db)
        .await?;
    let position = last.map(|s| s.position + 1).unwrap_or(0);

    let now = Utc::now();
    let section = deck_section::ActiveModel {
        deck_id: Set(deck.id),
        name: Set(name),
        position: Set(position),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    touch_deck(&state.db, deck.id, now).await?;

    Ok(Json(DeckSectionResponse::from(section)))
}

/// `PUT /api/decks/{game}/{deck_id}/sections/{section_id}` -> rename and/or reposition a
/// section (each field optional). A rename collides-checks like [`create_section`].
pub async fn update_section(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id, section_id)): Path<(String, i32, i32)>,
    JsonBody(payload): JsonBody<UpdateSectionRequest>,
) -> Result<Json<DeckSectionResponse>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let section = load_section(&state, deck.id, section_id).await?;

    let mut active: deck_section::ActiveModel = section.into();
    if let Some(raw_name) = payload.name {
        let name = validate_name(&raw_name, "name", MAX_SECTION_NAME)?;
        ensure_unique_name(&state, deck.id, &name, Some(section_id)).await?;
        active.name = Set(name);
    }
    if let Some(position) = payload.position {
        active.position = Set(position);
    }
    let now = Utc::now();
    active.updated_at = Set(now);
    let updated = active.update(&state.db).await?;
    touch_deck(&state.db, deck.id, now).await?;

    Ok(Json(DeckSectionResponse::from(updated)))
}

/// `DELETE /api/decks/{game}/{deck_id}/sections/{section_id}` -> delete a section, moving
/// its cards to the deck's first remaining section (merging counts on a collision). `409`
/// if it's the deck's only section (a deck must keep at least one).
pub async fn delete_section(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id, section_id)): Path<(String, i32, i32)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let section = load_section(&state, deck.id, section_id).await?;

    // The fallback the cards move to: the deck's lowest-position OTHER section.
    let fallback = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .filter(deck_section::Column::Id.ne(section.id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Conflict("a deck must have at least one section".to_string()))?;

    let now = Utc::now();
    let cards = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .filter(deck_card::Column::SectionId.eq(section.id))
        .all(&state.db)
        .await?;
    // Cards already in the fallback, keyed by card id, so a moved card merges rather than
    // colliding on the (deck, card, section) unique index.
    let fallback_cards: HashMap<i32, deck_card::Model> = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .filter(deck_card::Column::SectionId.eq(fallback.id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.card_id, c))
        .collect();

    for card in cards {
        if let Some(target) = fallback_cards.get(&card.card_id) {
            let mut active: deck_card::ActiveModel = target.clone().into();
            active.quantity = Set((target.quantity + card.quantity).min(MAX_CARD_QUANTITY));
            active.foil_quantity =
                Set((target.foil_quantity + card.foil_quantity).min(MAX_CARD_QUANTITY));
            active.updated_at = Set(now);
            active.update(&state.db).await?;
            DeckCard::delete_by_id(card.id).exec(&state.db).await?;
        } else {
            let mut active: deck_card::ActiveModel = card.into();
            active.section_id = Set(fallback.id);
            active.updated_at = Set(now);
            active.update(&state.db).await?;
        }
    }

    // The section is now empty, so this deletes nothing further via the card FK.
    DeckSection::delete_by_id(section.id)
        .exec(&state.db)
        .await?;
    touch_deck(&state.db, deck.id, now).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /api/decks/{game}/{deck_id}/sections/reorder` -> set the section order. The body
/// must list exactly the deck's section ids (any permutation); `422` otherwise. Returns the
/// sections in the new order.
pub async fn reorder_sections(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<ReorderSectionsRequest>,
) -> Result<Json<DataBody<Vec<DeckSectionResponse>>>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;

    let sections = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .all(&state.db)
        .await?;
    let existing: std::collections::HashSet<i32> = sections.iter().map(|s| s.id).collect();
    let requested: std::collections::HashSet<i32> = payload.section_ids.iter().copied().collect();
    if requested != existing || payload.section_ids.len() != existing.len() {
        return Err(AppError::Validation(
            "section_ids must be exactly the deck's sections".to_string(),
        ));
    }

    let now = Utc::now();
    let by_id: HashMap<i32, deck_section::Model> =
        sections.into_iter().map(|s| (s.id, s)).collect();
    let mut ordered = Vec::with_capacity(payload.section_ids.len());
    for (position, id) in payload.section_ids.iter().enumerate() {
        let section = by_id.get(id).expect("membership checked above").clone();
        let mut active: deck_section::ActiveModel = section.into();
        active.position = Set(position as i32);
        active.updated_at = Set(now);
        ordered.push(DeckSectionResponse::from(active.update(&state.db).await?));
    }
    touch_deck(&state.db, deck.id, now).await?;

    Ok(Json(DataBody { data: ordered }))
}

/// 409 if another section in this deck already has `name` (excluding `exclude_id`), a clean
/// error ahead of the unique index.
async fn ensure_unique_name(
    state: &AppState,
    deck_id: i32,
    name: &str,
    exclude_id: Option<i32>,
) -> Result<(), AppError> {
    let existing = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck_id))
        .filter(deck_section::Column::Name.eq(name))
        .one(&state.db)
        .await?;
    if let Some(s) = existing {
        if Some(s.id) != exclude_id {
            return Err(AppError::Conflict(format!(
                "a section named \"{name}\" already exists"
            )));
        }
    }
    Ok(())
}
