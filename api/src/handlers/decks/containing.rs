//! The card page's "in your decks" lookup: across all of the caller's decks for a game,
//! which ones contain a given card — any printing of it (gameplay identity, the stance
//! `/prints` and the needed-cards list take), with the copies split between the deck
//! proper and the maybeboard so a deck that's only *considering* the card can say so
//! rather than be hidden (issue #570's rule is about deck summaries; naming the deck here
//! with a "considering" flag is the honest answer for a containment question).
//!
//! Reads only (`AuthUser`), in the no-store private group. A deck card has no `user_id`,
//! so the scan is scoped to the deck ids the caller owns for the game — the same shape as
//! the needed-cards endpoint's demand scan.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{Card, Deck, DeckCard, DeckSection};
use crate::entities::{card, deck, deck_card, deck_section};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, identity_printing_ids, load_card, require_game};
use crate::state::AppState;

use super::{CardDeckPrintingRef, CardDeckRef, deck_headers};

/// List the caller's decks containing a card
///
/// `GET /api/decks/{game}/containing/{id}` -> the caller's decks that contain the card —
/// **any printing** of it (gameplay identity, as `/prints` resolves siblings) — most
/// recently updated deck first, matching the deck list. Each entry carries the deck's
/// full list header plus the copy counts, split between the deck proper (`quantity`) and
/// the maybeboard (`maybeboard_quantity`) so "runs it" and "only considering it" read
/// apart. Empty `{ "data": [] }` when no deck holds it. `404` for an unknown game/card.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/containing/{id}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
    ),
    responses(
        (status = 200, description = "The caller's decks containing any printing of the card, most recently updated first.", body = DataBody<Vec<CardDeckRef>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or card."),
    ),
)]
pub async fn decks_containing_card(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<DataBody<Vec<CardDeckRef>>>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;

    // The caller's decks for this game, in the deck list's own order — a deck card has no
    // user_id, so containment is only ever reached through these owned deck ids.
    let decks: Vec<deck::Model> = Deck::find()
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game))
        .order_by_desc(deck::Column::UpdatedAt)
        .order_by_desc(deck::Column::Id)
        .all(&state.db)
        .await?;
    if decks.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }
    let deck_ids: Vec<i32> = decks.iter().map(|d| d.id).collect();

    // Every row of any printing in any of those decks (bounded by the caller's own decks,
    // via idx_deck_cards_deck_id).
    let printing_ids = identity_printing_ids(&state, &card).await?;
    let rows: Vec<(i32, i32, i32, i32, i32)> = DeckCard::find()
        .select_only()
        .column(deck_card::Column::DeckId)
        .column(deck_card::Column::SectionId)
        .column(deck_card::Column::CardId)
        .column(deck_card::Column::Quantity)
        .column(deck_card::Column::FoilQuantity)
        .filter(deck_card::Column::DeckId.is_in(deck_ids.iter().copied()))
        .filter(deck_card::Column::CardId.is_in(printing_ids))
        .into_tuple()
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }

    // Which sections are maybeboards, resolved as a set (not the usual anti-join
    // sub-select) because each row must be *classified*, not filtered out.
    let maybeboard_sections: HashSet<i32> = DeckSection::find()
        .select_only()
        .column(deck_section::Column::Id)
        .filter(deck_section::Column::DeckId.is_in(deck_ids))
        .filter(deck_section::Column::IsMaybeboard.eq(true))
        .into_tuple::<i32>()
        .all(&state.db)
        .await?
        .into_iter()
        .collect();

    // Fold per deck: copies (regular + foil) in the deck proper vs the maybeboard, and
    // the same copies again per exact printing (maybeboard included — "which printing"
    // is the question, not "which board").
    #[derive(Default)]
    struct DeckCounts {
        quantity: i64,
        maybeboard_quantity: i64,
        by_printing: HashMap<i32, i64>,
    }
    let mut counts: HashMap<i32, DeckCounts> = HashMap::new();
    for (deck_id, section_id, card_id, quantity, foil_quantity) in rows {
        let copies = i64::from(quantity) + i64::from(foil_quantity);
        let entry = counts.entry(deck_id).or_default();
        if maybeboard_sections.contains(&section_id) {
            entry.maybeboard_quantity += copies;
        } else {
            entry.quantity += copies;
        }
        *entry.by_printing.entry(card_id).or_default() += copies;
    }

    // Resolve the printings the decks actually hold to their wire identities, in one
    // bounded query (a card has few printings across a user's decks). A printing whose
    // catalog row is gone is skipped, as every other card link tolerates.
    let held_printing_ids: Vec<i32> = counts
        .values()
        .flat_map(|c| c.by_printing.keys().copied())
        .collect::<HashSet<i32>>()
        .into_iter()
        .collect();
    let printing_info: HashMap<i32, (String, String, String)> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::ExternalId)
        .column(card::Column::SetCode)
        .column(card::Column::CollectorNumber)
        .filter(card::Column::Id.is_in(held_printing_ids))
        .into_tuple::<(i32, String, String, String)>()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|(id, external, set_code, number)| (id, (external, set_code, number)))
        .collect();

    // Shape the matched decks through the one header seam, keeping the list's order.
    let matched: Vec<deck::Model> = decks
        .into_iter()
        .filter(|d| counts.contains_key(&d.id))
        .collect();
    let headers = deck_headers(&state.db, &matched).await?;
    let data: Vec<CardDeckRef> = headers
        .into_iter()
        .map(|deck| {
            let deck_counts = counts.remove(&deck.id).unwrap_or_default();
            let mut printings: Vec<CardDeckPrintingRef> = deck_counts
                .by_printing
                .into_iter()
                .filter_map(|(card_id, quantity)| {
                    printing_info
                        .get(&card_id)
                        .map(|(external, set_code, number)| CardDeckPrintingRef {
                            id: external.clone(),
                            set_code: set_code.clone(),
                            collector_number: number.clone(),
                            quantity,
                        })
                })
                .collect();
            // Most copies first; set + number break ties so the order is stable.
            printings.sort_by(|a, b| {
                b.quantity
                    .cmp(&a.quantity)
                    .then_with(|| a.set_code.cmp(&b.set_code))
                    .then_with(|| a.collector_number.cmp(&b.collector_number))
            });
            CardDeckRef {
                deck,
                quantity: deck_counts.quantity,
                maybeboard_quantity: deck_counts.maybeboard_quantity,
                printings,
            }
        })
        .collect();
    Ok(Json(DataBody { data }))
}
