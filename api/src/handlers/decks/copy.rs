//! Copy a public deck into the caller's own decks (issue #502).
//!
//! An authenticated user viewing someone's shared deck can duplicate it into their own
//! collection of decks. The source is addressed exactly like the public read
//! (`handlers::sharing::decks::public_deck`) — by the owner's handle + deck id, gated on
//! `is_public` — so a private/unknown source is a uniform `404` (never a `403`; no existence
//! oracle over `/api/u/...`). The write mirrors the atomic whole-deck insert in
//! `deck_import::create_deck_from_rows`: one transaction inserts the new deck, its sections
//! (preserving name + position), then its cards in bounded chunks. Unlike an import there is
//! no card resolution to do — the source's `deck_card.card_id` are already internal `cards.id`,
//! shared with the copy, so they carry across verbatim (a copy survives a catalog re-import
//! for the same reason a deck card does).

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};
use std::collections::HashMap;

use crate::auth::extractor::WritableUser;
use crate::auth::username::handle_of;
use crate::entities::prelude::{Deck, DeckCard, DeckSection};
use crate::entities::{deck, deck_card, deck_section};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::sharing::decks::load_public_deck;
use crate::state::AppState;

use super::{MAX_DECK_NAME, MAX_DECKS_PER_GAME, deck_detail};

/// Cards are bulk-inserted in bounded batches so the SQL parameter count stays within the
/// SQLite/Postgres limits even for a large source deck — the same rationale (and value) as the
/// import pipeline's `INSERT_CHUNK`.
const COPY_INSERT_CHUNK: usize = 100;

/// The suffix appended to a copied deck's name so the owner can tell the duplicate apart.
const COPY_NAME_SUFFIX: &str = " (copy)";

/// Build the copy's name: the source name plus a `(copy)` suffix, truncated on a char boundary
/// so the result still fits `MAX_DECK_NAME`.
fn copy_name(source: &str) -> String {
    let budget = MAX_DECK_NAME.saturating_sub(COPY_NAME_SUFFIX.chars().count());
    let base: String = source.trim().chars().take(budget).collect();
    format!("{base}{COPY_NAME_SUFFIX}")
}

/// Copy public deck
///
/// `POST /api/u/{handle}/decks/{deck_id}/copy` -> duplicate a public deck (addressed by the
/// owner's handle + deck id) into the caller's own decks, returning the new deck's full detail.
/// The copy starts private and loose (no folder), carrying the source's sections (name +
/// position) and cards (with their regular/foil counts) verbatim. `404` when the handle is
/// unknown or the source deck is private/absent (no existence oracle); `422` when the caller is
/// already at their per-game deck cap.
#[utoipa::path(
    post,
    path = "/api/u/{handle}/decks/{deck_id}/copy",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("handle" = String, Path, description = "The source deck owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The source (public) deck's id"),
    ),
    responses(
        (status = 200, description = "The newly created copy's full detail (owned by the caller).", body = super::DeckDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown handle, or the source deck is private/absent."),
        (status = 422, description = "The caller is already at their per-game deck cap."),
    ),
)]
pub async fn copy_public_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((handle, deck_id)): Path<(String, i32)>,
) -> Result<Json<super::DeckDetail>, AppError> {
    // Resolve the source through the same seam as the public read (`public_deck`): the owner's
    // handle, gated on `is_public`. Any miss — bad handle, private deck, wrong owner — collapses
    // to the one identical 404 body, so this write is no more of an existence oracle than the read.
    let (_owner, source) = load_public_deck(&state, &handle, deck_id).await?;

    // Enforce the caller's per-game deck cap before writing anything (same guard as create).
    let count = Deck::find()
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&source.game))
        .count(&state.db)
        .await?;
    if count >= MAX_DECKS_PER_GAME {
        return Err(AppError::Validation(format!(
            "you can have at most {MAX_DECKS_PER_GAME} decks per game"
        )));
    }

    // Read the source's sections (in their display order) and cards up front, outside the
    // transaction — they're another user's already-committed rows.
    let sections = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(source.id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?;
    let cards = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(source.id))
        .all(&state.db)
        .await?;

    let now = Utc::now();
    let txn = state.db.begin().await?;

    // 1. The new deck row, owned by the caller: private, loose, same game/format, `(copy)` name.
    let new_deck = deck::ActiveModel {
        user_id: Set(user.id),
        game: Set(source.game.clone()),
        folder_id: Set(None),
        name: Set(copy_name(&source.name)),
        description: Set(source.description.clone()),
        format: Set(source.format.clone()),
        is_public: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    // 2. Copy the sections one at a time, keeping an old -> new id map to re-file the cards.
    let mut section_map: HashMap<i32, i32> = HashMap::with_capacity(sections.len());
    for section in &sections {
        let inserted = deck_section::ActiveModel {
            deck_id: Set(new_deck.id),
            name: Set(section.name.clone()),
            position: Set(section.position),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        section_map.insert(section.id, inserted.id);
    }

    // 3. Copy the cards, remapped onto the new deck + section ids; counts carry across as-is.
    // A card whose section somehow didn't copy is skipped rather than aborting the whole copy.
    let new_cards: Vec<deck_card::ActiveModel> = cards
        .iter()
        .filter_map(|card| {
            let section_id = section_map.get(&card.section_id)?;
            Some(deck_card::ActiveModel {
                deck_id: Set(new_deck.id),
                section_id: Set(*section_id),
                card_id: Set(card.card_id),
                quantity: Set(card.quantity),
                foil_quantity: Set(card.foil_quantity),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            })
        })
        .collect();
    for chunk in new_cards.chunks(COPY_INSERT_CHUNK) {
        DeckCard::insert_many(chunk.iter().cloned())
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;

    // Return the full detail of the caller's new deck (owner handle = the caller's own).
    Ok(Json(
        deck_detail(&state, &new_deck, handle_of(&user)).await?,
    ))
}
