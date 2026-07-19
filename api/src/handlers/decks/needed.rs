//! The deck "cards needed" endpoint (issue #499): across all of a user's decks for a game,
//! the cards their decks collectively want more copies of than their collection holds — a
//! shopping list for building every deck at once.
//!
//! Two matching modes (see [`NeedMode`]): `card` aggregates by gameplay identity so any
//! printing you own covers any printing a deck wants (the default — "two decks want a
//! Command Tower, you own one, you need one more"); `printing` matches a deck's exact
//! printing against that same printing in the collection, naming the precise printing
//! that's short. Either way, each result carries which of the caller's decks want it.
//!
//! Reads only (`AuthUser`), in the no-store private group. A deck card has no `user_id`, so
//! the demand scan is scoped to the deck ids the caller owns for the game (never queried by
//! user directly); catalog rows gone after a re-import are skipped, exactly as the deck
//! detail read does.

use std::collections::{BTreeSet, HashMap};

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{Card, CollectionItem, Deck, DeckCard};
use crate::entities::{card, collection_item, deck, deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{CardResponse, DataBody, require_game};
use crate::state::AppState;

use super::{NeedMode, NeededCard, NeededCardDeck, NeededParams};

/// List cards needed across a game's decks
///
/// `GET /api/decks/{game}/needed` -> every card the caller's decks collectively want more
/// copies of than their collection holds, sorted by name. `mode=card` (default) aggregates
/// across any printing of a gameplay card; `mode=printing` reports the exact missing
/// printing. Each entry lists the decks that want the card. Returns `{ data: NeededCard[] }`.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/needed",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("mode" = Option<String>, Query, description = "`card` (default) counts any printing of a gameplay card; `printing` reports the exact missing printing."),
    ),
    responses(
        (status = 200, description = "Cards the caller's decks need beyond their collection, by name.", body = DataBody<Vec<NeededCard>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn needed_cards(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<NeededParams>,
) -> Result<Json<DataBody<Vec<NeededCard>>>, AppError> {
    require_game(&game)?;

    // The caller's decks for this game — id + name only (id scopes the card scan below;
    // name labels each result's affected decks). A deck card has no user_id, so its
    // demand is only ever reached through these owned deck ids.
    let decks: Vec<(i32, String)> = Deck::find()
        .select_only()
        .column(deck::Column::Id)
        .column(deck::Column::Name)
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game))
        .into_tuple()
        .all(&state.db)
        .await?;
    if decks.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }
    let deck_names: HashMap<i32, String> = decks.iter().cloned().collect();
    let deck_ids: Vec<i32> = decks.iter().map(|(id, _)| *id).collect();

    // Every deck card joined to its catalog row (a card gone after a re-import LEFT-joins to
    // None and is skipped, matching the deck detail read).
    let deck_rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.is_in(deck_ids.iter().copied()))
        .all(&state.db)
        .await?;

    // The caller's collection for the game, summed two ways so either mode has its supply:
    // per printing (internal card id) and per gameplay identity (oracle id, else name).
    let owned_rows: Vec<(i32, Option<String>, String, i32, i32)> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(card::Column::OracleId)
        .column(card::Column::Name)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .inner_join(Card)
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(&game))
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut owned_by_printing: HashMap<i32, i64> = HashMap::new();
    let mut owned_by_identity: HashMap<String, i64> = HashMap::new();
    for (card_id, oracle_id, name, quantity, foil_quantity) in owned_rows {
        let copies = i64::from(quantity) + i64::from(foil_quantity);
        *owned_by_printing.entry(card_id).or_default() += copies;
        *owned_by_identity
            .entry(identity_key(oracle_id.as_deref(), &name))
            .or_default() += copies;
    }

    // Fold deck demand into groups keyed by the mode's grouping, tracking the total copies
    // wanted, which decks want them, and each contributing printing (to pick the
    // representative card the `card` mode shows).
    #[derive(Default)]
    struct Group {
        required: i64,
        deck_ids: BTreeSet<i32>,
        /// `card_id` -> (copies wanted for that printing, its catalog row).
        printings: HashMap<i32, (i64, card::Model)>,
    }
    let mut groups: HashMap<String, Group> = HashMap::new();
    for (entry, card) in deck_rows {
        let Some(card) = card else { continue };
        let copies = i64::from(entry.quantity) + i64::from(entry.foil_quantity);
        let key = match params.mode {
            NeedMode::Card => identity_key(card.oracle_id.as_deref(), &card.name),
            NeedMode::Printing => format!("p:{}", card.id),
        };
        let group = groups.entry(key).or_default();
        group.required += copies;
        group.deck_ids.insert(entry.deck_id);
        let printing = group.printings.entry(card.id).or_insert((0, card));
        printing.0 += copies;
    }

    // Emit the shortfalls (demand beyond supply), sorted by card name.
    let mut data: Vec<NeededCard> = Vec::new();
    for (key, group) in groups {
        // The representative printing is the one the decks want most (ties: lowest catalog
        // id), so `card` mode shows a printing the decks actually reference. In `printing`
        // mode the group is a single printing, so this just returns it.
        let (rep_id, (_, rep_card)) = group
            .printings
            .into_iter()
            .max_by(|(a_id, (a_copies, _)), (b_id, (b_copies, _))| {
                a_copies.cmp(b_copies).then_with(|| b_id.cmp(a_id))
            })
            .expect("a demand group always has at least one printing");
        let owned = match params.mode {
            NeedMode::Card => owned_by_identity.get(&key).copied().unwrap_or(0),
            NeedMode::Printing => owned_by_printing.get(&rep_id).copied().unwrap_or(0),
        };
        let needed = group.required - owned;
        if needed <= 0 {
            continue;
        }
        let mut decks: Vec<NeededCardDeck> = group
            .deck_ids
            .iter()
            .filter_map(|id| {
                deck_names.get(id).map(|name| NeededCardDeck {
                    id: *id,
                    name: name.clone(),
                })
            })
            .collect();
        decks.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        data.push(NeededCard {
            card: CardResponse::from(rep_card),
            needed,
            required: group.required,
            owned,
            decks,
        });
    }
    data.sort_by(|a, b| {
        a.card
            .name
            .cmp(&b.card.name)
            .then_with(|| a.card.id.cmp(&b.card.id))
    });

    Ok(Json(DataBody { data }))
}

/// The gameplay identity of a card across printings: its `oracle_id`, or its name when the
/// catalog has none — the same rule the deck printing-swap uses to decide two rows are the
/// same card. Namespaced (`o:` / `n:`) so an oracle id can never collide with a name.
fn identity_key(oracle_id: Option<&str>, name: &str) -> String {
    match oracle_id {
        Some(oracle) => format!("o:{oracle}"),
        None => format!("n:{name}"),
    }
}
