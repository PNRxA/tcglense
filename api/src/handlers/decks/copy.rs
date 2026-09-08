//! Copy a public deck into the caller's own decks (issue #502), and the **write seam** every
//! such copy goes through ([`insert_deck_with_cards`]).
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
//!
//! That last paragraph describes **three** callers now: this one, the owner's own duplicate
//! ([`copy_deck`], issue #674 — "make a v2 with the new commander" without publishing first),
//! and the precon copy ([`crate::handlers::precons::copy`]), whose source rows likewise
//! already carry internal card ids. All three go through [`insert_deck_with_cards`], so the
//! deck cap, the transaction boundary, and the chunked insert are stated once — the
//! difference between them is only *where the sections come from* (an existing deck's, read
//! through [`source_sections`], or a mapping of upstream's boards) and where the copy is
//! filed (a duplicate stays in its source's folder; the other two start loose).

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
use crate::handlers::shared::require_game;
use crate::handlers::sharing::decks::load_public_deck;
use crate::state::AppState;

use super::{DeckResponse, MAX_DECK_NAME, MAX_DECKS_PER_GAME, deck_detail, deck_header, load_deck};

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
/// The copy starts private and loose (no folder), carrying the source's sections (name,
/// position, and maybeboard flag) and cards (with their regular/foil counts) verbatim. `404` when the handle is
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

    // Read the source's sections and cards up front, outside the transaction — they're
    // another user's already-committed rows.
    let new_sections = source_sections(&state, source.id).await?;

    let new_deck = insert_deck_with_cards(
        &state,
        user.id,
        NewDeck {
            game: source.game.clone(),
            name: copy_name(&source.name),
            description: source.description.clone(),
            format: source.format.clone(),
            // Someone else's folders mean nothing to the caller: the copy starts loose.
            folder_id: None,
        },
        new_sections,
    )
    .await?;

    // Return the full detail of the caller's new deck (owner handle = the caller's own).
    Ok(Json(
        deck_detail(&state, &new_deck, handle_of(&user)).await?,
    ))
}

/// Duplicate deck
///
/// `POST /api/decks/{game}/{deck_id}/copy` -> duplicate one of the caller's **own** decks
/// (issue #674), returning the new deck's list header. The copy is named
/// `"<name> (copy)"`, starts private, is filed in the **same folder** as its source, and
/// carries the source's sections (name, position, maybeboard flag) and cards (regular +
/// foil counts) verbatim — the same clone the public copy makes, without having to publish
/// the deck first. `404` when the deck isn't the caller's; `422` at the per-game deck cap.
#[utoipa::path(
    post,
    path = "/api/decks/{game}/{deck_id}/copy",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "The deck to duplicate (must be the caller's)"),
    ),
    responses(
        (status = 200, description = "The new copy's list header (private, same folder as its source).", body = DeckResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 422, description = "The caller is already at their per-game deck cap."),
    ),
)]
pub async fn copy_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckResponse>, AppError> {
    require_game(&game)?;
    // Ownership first: a deck that isn't the caller's is the uniform 404, like every other
    // deck-scoped route (never a 403 — no existence oracle over deck ids).
    let source = load_deck(&state, user.id, &game, deck_id).await?;
    let new_sections = source_sections(&state, source.id).await?;

    let new_deck = insert_deck_with_cards(
        &state,
        user.id,
        NewDeck {
            game: source.game.clone(),
            name: copy_name(&source.name),
            description: source.description.clone(),
            format: source.format.clone(),
            // The caller's own folder, so the v2 lands beside the v1 on the shelf.
            folder_id: source.folder_id,
        },
        new_sections,
    )
    .await?;

    // A header, like an import's response: the SPA navigates to the copy and loads its detail
    // there. Built through the one `deck_header` seam every `DeckResponse` producer uses.
    Ok(Json(deck_header(&state.db, &new_deck).await?))
}

/// Read an existing deck's sections (in display order) and cards into the shape
/// [`insert_deck_with_cards`] writes — the half of a deck copy that every deck-sourced copy
/// shares, whoever owns the source. The caller has already proved it may read `deck_id`.
///
/// The copy's stored `position` values are compacted to `0..n` by the seam rather than
/// mirroring the source's integers — the *order* is what a section's position means, and
/// it is unchanged. A card whose section somehow didn't come back is dropped rather than
/// aborting the whole copy — the same tolerance the per-card section lookup had.
pub(crate) async fn source_sections(
    state: &AppState,
    deck_id: i32,
) -> Result<Vec<NewDeckSection>, AppError> {
    let sections = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck_id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?;
    let cards = DeckCard::find()
        .filter(deck_card::Column::DeckId.eq(deck_id))
        .all(&state.db)
        .await?;
    let mut by_section: HashMap<i32, Vec<NewDeckCard>> = HashMap::with_capacity(sections.len());
    for card in &cards {
        by_section
            .entry(card.section_id)
            .or_default()
            .push(NewDeckCard {
                card_id: card.card_id,
                quantity: card.quantity,
                foil_quantity: card.foil_quantity,
            });
    }
    Ok(sections
        .into_iter()
        .map(|section| NewDeckSection {
            cards: by_section.remove(&section.id).unwrap_or_default(),
            name: section.name,
            is_maybeboard: section.is_maybeboard,
        })
        .collect())
}

/// The metadata of a deck about to be written by [`insert_deck_with_cards`]. Everything else
/// about a new deck is fixed by the seam: it is the caller's and private.
pub(crate) struct NewDeck {
    pub game: String,
    pub name: String,
    pub description: Option<String>,
    pub format: Option<String>,
    /// The caller's folder to file the copy under, or `None` for loose. Only a duplicate of
    /// the caller's own deck ever sets it — a source it already owns is the one case where
    /// the folder is theirs to inherit.
    pub folder_id: Option<i32>,
}

/// One section of a deck about to be written, in display order, with the cards filed under
/// it. Card ids are **internal** `cards.id` — both callers already hold them, which is
/// exactly what distinguishes a copy from an import.
pub(crate) struct NewDeckSection {
    pub name: String,
    pub is_maybeboard: bool,
    pub cards: Vec<NewDeckCard>,
}

/// One card of a section about to be written: an internal card id and its two counts.
pub(crate) struct NewDeckCard {
    pub card_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Write a whole deck — row, sections (in the given order), and cards — for `user_id` in one
/// transaction, returning the new deck row.
///
/// The per-game deck cap is enforced **before** anything is written (the same guard `create`
/// applies), and the cards go in bounded chunks so the bind count stays within SQLite's and
/// Postgres' limits for a large source deck. An empty section is still created: a copied
/// deck should look like its source, including the buckets its owner left empty. Nothing
/// here re-validates `meta.folder_id` — every caller passes either `None` or a folder read
/// off a deck row the caller already owns.
pub(crate) async fn insert_deck_with_cards(
    state: &AppState,
    user_id: i32,
    meta: NewDeck,
    sections: Vec<NewDeckSection>,
) -> Result<deck::Model, AppError> {
    let count = Deck::find()
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(&meta.game))
        .count(&state.db)
        .await?;
    if count >= MAX_DECKS_PER_GAME {
        return Err(AppError::Validation(format!(
            "you can have at most {MAX_DECKS_PER_GAME} decks per game"
        )));
    }

    let now = Utc::now();
    let txn = state.db.begin().await?;

    // 1. The new deck row, owned by the caller: private, in whichever folder the caller
    //    chose (the seam trusts `meta.folder_id` — it is the source deck's own, or none).
    let new_deck = deck::ActiveModel {
        user_id: Set(user_id),
        game: Set(meta.game),
        folder_id: Set(meta.folder_id),
        name: Set(meta.name),
        description: Set(meta.description),
        format: Set(meta.format),
        is_public: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    // 2. The sections, one at a time (each insert hands back the id its cards need), then
    // 3. that section's cards, in bounded chunks.
    let mut new_cards: Vec<deck_card::ActiveModel> = Vec::new();
    for (position, section) in sections.into_iter().enumerate() {
        let inserted = deck_section::ActiveModel {
            deck_id: Set(new_deck.id),
            name: Set(section.name),
            position: Set(position as i32),
            is_maybeboard: Set(section.is_maybeboard),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        new_cards.extend(
            section
                .cards
                .into_iter()
                .map(|card| deck_card::ActiveModel {
                    deck_id: Set(new_deck.id),
                    section_id: Set(inserted.id),
                    card_id: Set(card.card_id),
                    quantity: Set(card.quantity),
                    foil_quantity: Set(card.foil_quantity),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }),
        );
    }
    for chunk in new_cards.chunks(COPY_INSERT_CHUNK) {
        DeckCard::insert_many(chunk.iter().cloned())
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;
    Ok(new_deck)
}
