//! Turning "the deck I want to play" into the cards on the table.
//!
//! Three sources, one answer. One of the caller's own decks, any published precon, or a
//! pasted decklist all resolve to a `Vec<CardDef>` — name, faces, colours, and the two flags
//! the table cares about (`is_commander`, `is_token`). The resolution happens **once**, here,
//! when the deck is loaded into a seat; `engine::start_game` then builds the library from
//! that stored list with no catalog query on the path, which is what keeps starting a
//! four-player game a pure in-memory operation.
//!
//! The three sources differ only in where the command zone comes from, and each reads it off
//! the structure the rest of the app already uses rather than inventing a rule:
//!
//! - a **deck** splits by section name through [`deck_zone`] — the same function the deck
//!   analysis reads a commander off, so a deck that shows a commander on its page seats one
//!   here (and a sideboard / maybeboard section is skipped, because you don't shuffle those);
//! - a **precon** splits by `precon_deck_cards.board`, the column the ingest folds upstream's
//!   boards into (`commander` -> command zone, `side` -> skipped);
//! - a **text list** splits by the section headers the shared `deck_import` text grammar
//!   already parses, so the exact list you'd paste into a deck import behaves the same here.
//!
//! Outside Commander there is no command zone, so the split is simply not applied — a
//! `constructed` room shuffles the commander section into the library like any other card.
//!
//! Unresolved names are a **422 listing them**, never a silently shorter deck: sitting down
//! with 97 of your 100 cards and finding out mid-game is worse than being told now.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::collection_import::{Provider, reconcile::resolve_newest_printing_by_name};
use crate::deck_import::{DeckImportFileFormat, parse_file};
use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, DeckCard, DeckSection, PreconDeckCard};
use crate::entities::{
    card, deck_card, deck_section, play_room, play_seat, precon_deck_card, user,
};
use crate::error::AppError;
use crate::handlers::decks::{DeckZone, deck_zone, load_deck};
use crate::handlers::precons::load_precon;
use crate::handlers::shared::dto::stored_faces;
use crate::play::types::{CardDef, CardFace, FORMAT_COMMANDER, MAX_DECK_CARDS};
use crate::state::AppState;

use super::LoadDeckRequest;

/// How many unresolved names a 422 names before it stops (the rest are counted).
const UNRESOLVED_SAMPLE: usize = 10;

/// A resolved decklist, ready to be stored on a seat.
pub(crate) struct ResolvedDeck {
    /// `deck` / `precon` / `text` — what `play_seats.deck_source` records.
    pub source: &'static str,
    /// The deck id or precon slug it came from; `None` for a pasted list.
    pub deck_ref: Option<String>,
    pub name: String,
    pub cards: Vec<CardDef>,
}

/// Resolve a [`LoadDeckRequest`] against the catalog.
pub(crate) async fn resolve_deck(
    state: &AppState,
    game: &str,
    room: &play_room::Model,
    seat: &play_seat::Model,
    user: Option<&user::Model>,
    request: LoadDeckRequest,
) -> Result<ResolvedDeck, AppError> {
    // Only a Commander table has a command zone; everywhere else the "commander" section is
    // just another pile of cards, so the flag is never set and `start_game` shuffles them in.
    let uses_command_zone = room.format == FORMAT_COMMANDER;

    let resolved = match request {
        LoadDeckRequest::Deck { deck_id } => {
            resolve_own_deck(state, game, seat, user, deck_id, uses_command_zone).await?
        }
        LoadDeckRequest::Precon { slug } => {
            resolve_precon(state, game, &slug, uses_command_zone).await?
        }
        LoadDeckRequest::Text { text } => {
            resolve_text(state, game, &text, uses_command_zone).await?
        }
    };

    if resolved.cards.len() > MAX_DECK_CARDS {
        return Err(AppError::Validation(format!(
            "a deck may hold at most {MAX_DECK_CARDS} cards; this one has {}",
            resolved.cards.len()
        )));
    }
    if resolved.cards.is_empty() {
        return Err(AppError::Validation(
            "that deck has no cards to play with".to_string(),
        ));
    }
    Ok(resolved)
}

/// One of the caller's own decks. The seat must be a signed-in one **and** the session must
/// be that same account: a guest seat has no "my decks", and borrowing someone else's seat id
/// must not let you read their library.
async fn resolve_own_deck(
    state: &AppState,
    game: &str,
    seat: &play_seat::Model,
    user: Option<&user::Model>,
    deck_id: i32,
    uses_command_zone: bool,
) -> Result<ResolvedDeck, AppError> {
    let user = user
        .ok_or_else(|| AppError::Validation("sign in to play one of your own decks".to_string()))?;
    if seat.user_id != Some(user.id) {
        // Same call `load_deck` makes for a foreign deck: a 404, never a 403.
        return Err(AppError::NotFound("deck not found".to_string()));
    }
    // `load_deck` is the ownership gate — a deck that isn't the caller's is a 404.
    let deck = load_deck(state, user.id, game, deck_id).await?;

    let sections: HashMap<i32, deck_section::Model> = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|section| (section.id, section))
        .collect();

    let rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .order_by_asc(deck_card::Column::SectionId)
        .order_by_asc(deck_card::Column::Id)
        .all(&state.db)
        .await?;

    let mut cards = Vec::new();
    for (row, card) in rows {
        let Some(card) = card else { continue };
        let Some(section) = sections.get(&row.section_id) else {
            continue;
        };
        // A maybeboard is a shortlist, not part of the deck; a sideboard isn't shuffled in.
        if section.is_maybeboard || deck_zone(&section.name) == DeckZone::Sideboard {
            continue;
        }
        let is_commander = uses_command_zone && deck_zone(&section.name) == DeckZone::Command;
        // Both finishes of a printing are the same card at a manual table.
        let copies = row.quantity.saturating_add(row.foil_quantity).max(0);
        push_copies(&mut cards, &card, is_commander, copies);
    }

    Ok(ResolvedDeck {
        source: "deck",
        deck_ref: Some(deck.id.to_string()),
        name: deck.name,
        cards,
    })
}

/// Any published preconstructed deck, by slug.
async fn resolve_precon(
    state: &AppState,
    game: &str,
    slug: &str,
    uses_command_zone: bool,
) -> Result<ResolvedDeck, AppError> {
    let precon = load_precon(state, game, slug).await?;

    let rows: Vec<(precon_deck_card::Model, Option<card::Model>)> = PreconDeckCard::find()
        .find_also_related(Card)
        .filter(precon_deck_card::Column::PreconDeckId.eq(precon.id))
        .order_by_asc(precon_deck_card::Column::Position)
        .order_by_asc(precon_deck_card::Column::Id)
        .all(&state.db)
        .await?;

    let mut cards = Vec::new();
    for (row, card) in rows {
        let Some(card) = card else { continue };
        if row.board == PreconBoard::Side.as_str() {
            continue;
        }
        let is_commander = uses_command_zone && row.board == PreconBoard::Commander.as_str();
        push_copies(&mut cards, &card, is_commander, row.quantity.max(0));
    }

    Ok(ResolvedDeck {
        source: "precon",
        deck_ref: Some(precon.slug),
        name: precon.name,
        cards,
    })
}

/// A pasted decklist, through the shared `deck_import` text grammar — so the exact list a
/// player would paste into a deck import (including its section headers) behaves identically
/// here, rather than meeting a second, subtly different parser.
async fn resolve_text(
    state: &AppState,
    game: &str,
    text: &str,
    uses_command_zone: bool,
) -> Result<ResolvedDeck, AppError> {
    // The provider only picks the *file* dialect; a plain text list is provider-neutral.
    let parsed = parse_file(
        Provider::Moxfield,
        DeckImportFileFormat::Text,
        "Pasted list".to_string(),
        text.as_bytes(),
    )
    .map_err(AppError::from)?;

    if parsed.rows.is_empty() {
        return Err(AppError::Validation(
            "that list has no cards in it".to_string(),
        ));
    }

    // A pasted list has no printing key, so every line resolves by name to the newest
    // printing — the same fallback a name-only deck-import row takes.
    let names: Vec<String> = parsed
        .rows
        .iter()
        .map(|row| row.card_name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let by_name = resolve_newest_printing_by_name(&state.db, game, &names)
        .await
        .map_err(AppError::from)?;

    let ids: Vec<i32> = by_name.values().map(|(id, _)| *id).collect();
    let cards_by_id: HashMap<i32, card::Model> = Card::find()
        .filter(card::Column::Id.is_in(ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|card| (card.id, card))
        .collect();

    let mut unresolved: Vec<String> = Vec::new();
    let mut cards = Vec::new();
    for row in &parsed.rows {
        let resolved = by_name
            .get(&row.card_name)
            .and_then(|(id, _)| cards_by_id.get(id));
        let Some(card) = resolved else {
            if !unresolved.contains(&row.card_name) {
                unresolved.push(row.card_name.clone());
            }
            continue;
        };
        let is_commander = uses_command_zone && deck_zone(&row.section) == DeckZone::Command;
        if deck_zone(&row.section) == DeckZone::Sideboard {
            continue;
        }
        push_copies(&mut cards, card, is_commander, row.quantity.max(0));
    }

    if !unresolved.is_empty() {
        let shown = unresolved
            .iter()
            .take(UNRESOLVED_SAMPLE)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let extra = unresolved.len().saturating_sub(UNRESOLVED_SAMPLE);
        let suffix = if extra > 0 {
            format!(" (and {extra} more)")
        } else {
            String::new()
        };
        return Err(AppError::Validation(format!(
            "these cards weren't found in the catalog: {shown}{suffix}"
        )));
    }

    Ok(ResolvedDeck {
        source: "text",
        deck_ref: None,
        name: parsed.name,
        cards,
    })
}

// ---------- CardDef ----------

/// Append `copies` identical definitions. Each copy is its own card at the table (they get
/// distinct instance ids when the game starts), so the list is flat rather than counted.
fn push_copies(out: &mut Vec<CardDef>, card: &card::Model, is_commander: bool, copies: i32) {
    if copies <= 0 {
        return;
    }
    // Bounded here as well as at the caller so one absurd `99999x Island` line can't build a
    // huge vector before the total check sees it.
    let copies = copies.min(MAX_DECK_CARDS as i32) as usize;
    let def = card_def(card, is_commander);
    for _ in 0..copies {
        out.push(def.clone());
    }
}

/// A catalog row as the table sees it. Lean on purpose — the image is fetched through the
/// catalog proxy by `card_id`, and the faces carry just enough text for the hover preview.
fn card_def(card: &card::Model, is_commander: bool) -> CardDef {
    let stored = stored_faces(card);
    // A double-faced card only has its own back image when the stored face carries one; the
    // SPA uses that to decide whether `?face=1` is worth requesting.
    let back_image = stored
        .get(1)
        .is_some_and(|face| face.image_normal.is_some() || face.image_small.is_some());

    let faces: Vec<CardFace> = if stored.is_empty() {
        vec![CardFace {
            name: card.name.clone(),
            mana_cost: card.mana_cost.clone(),
            type_line: card.type_line.clone(),
            oracle_text: card.oracle_text.clone(),
            power: card.power.clone(),
            toughness: card.toughness.clone(),
            loyalty: card.loyalty.clone(),
        }]
    } else {
        stored
            .into_iter()
            .map(|face| CardFace {
                name: face.name.unwrap_or_else(|| card.name.clone()),
                mana_cost: face.mana_cost,
                type_line: face.type_line,
                oracle_text: face.oracle_text,
                power: face.power,
                toughness: face.toughness,
                loyalty: face.loyalty,
            })
            .collect()
    };

    CardDef {
        card_id: Some(card.external_id.clone()),
        game: card.game.clone(),
        name: card.name.clone(),
        faces,
        back_image,
        colors: card
            .colors
            .as_deref()
            .map(|colors| {
                colors
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        cmc: card.cmc,
        is_commander,
        // A token printing in a decklist is still a token at the table: it ceases to exist
        // when it leaves the battlefield rather than going to a graveyard.
        is_token: card
            .type_line
            .as_deref()
            .is_some_and(|line| line.contains("Token")),
    }
}
