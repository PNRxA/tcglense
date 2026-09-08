//! Add a deck's cards to the caller's collection — the "I bought this" write.
//!
//! A user who buys a preconstructed deck, or builds one of their own decks out of cards
//! they now physically own, wants the collection to know: one click, every card. This is
//! the bridge *from* the deck idea *to* the holdings surface, the mirror of the precon copy
//! (which bridges the other way, catalog -> deck). Two entry points share it — the owner's
//! deck ([`add_deck_to_collection`]) and a published precon
//! ([`crate::handlers::precons::add_precon_to_collection`]) — and a third, someone else's
//! **public** deck ([`add_public_deck_to_collection`]: "I bought the singles for this list"),
//! addressed exactly as its public read and copy are — and they only differ in where the rows
//! come from, so the write itself is stated once, in [`add_rows_to_collection`].
//!
//! Three things about that write are load-bearing:
//!
//! * **It is additive.** The counts are added on top of whatever the user already owns, never
//!   set to the deck's counts: someone who owns two Sol Rings and buys a precon with one now
//!   owns three. The SPA confirms before it fires (a second click adds a second copy of
//!   everything — that is the honest reading of "I bought another one", not a bug to
//!   de-duplicate away). Idempotency would need "which purchase" to key on, and there is no
//!   such fact.
//! * **It is the import engine's `merge`, not a loop over the single-card upsert.** Every bulk
//!   collection write goes through `collection_import::reconcile` so the foil-★ variant fold,
//!   the by-card aggregation (a deck may hold one printing in two sections), the count clamp
//!   and the single all-or-nothing transaction are stated once. `merge_holdings` is that
//!   engine with the mode fixed and no provider to name.
//! * **A deck's maybeboard is skipped.** A card the deck is only *considering* is not one its
//!   owner bought, so the rows come from the deck proper plus the sideboard — the same split
//!   the `needed` shopping list and the deck's own `summary` make (issue #570). A precon has no
//!   maybeboard: every board it ships is in the box.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;

use crate::auth::extractor::WritableUser;
use crate::collection_import::{self, FetchedHolding, MAX_IMPORT_ROWS};
use crate::entities::prelude::{Card, DeckCard};
use crate::entities::{card, deck_card};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::require_game;
use crate::handlers::sharing::decks::load_public_deck;
use crate::state::AppState;

use super::{load_deck, maybeboard_section_ids};

/// What an add-to-collection did: how many printings and copies went in, and how many source
/// rows could not. Deliberately not [`ImportSummary`](crate::collection_import::ImportSummary):
/// there is no provider, no mode choice and no unmatched sample to report — the rows came from
/// the catalog, so the only way one fails to land is its card having left it since.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionAddSummary {
    /// Distinct printings whose owned counts went up.
    pub cards: usize,
    /// Regular copies added on top of what was already owned.
    pub regular_copies: i64,
    /// Foil copies added on top of what was already owned.
    pub foil_copies: i64,
    /// Distinct cards that could not be added because they are no longer in the catalog —
    /// the same unit as `cards`, so the two add up to what the source listed. Zero for
    /// anything built or bought since the last catalog sync.
    pub skipped_cards: usize,
}

/// One source row about to be added: the card's **external** id (the shape the reconcile
/// engine resolves — a deck row holds the internal id, but the joined catalog row carries the
/// external one) and its two counts.
pub(crate) struct CollectionAddRow {
    pub external_id: String,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Add `rows` to the caller's collection for `game` through the import engine's `merge`, and
/// report what landed. `skipped` is how many distinct cards the caller already dropped for
/// having no catalog row — folded into the summary so the client sees one number.
///
/// `422` (`empty_message`) when there is nothing to add: the caller words what "nothing"
/// means for its source, since an empty deck and a precon whose every card has left the
/// catalog are different things to tell a user.
pub(crate) async fn add_rows_to_collection(
    state: &AppState,
    user_id: i32,
    game: &str,
    rows: Vec<CollectionAddRow>,
    skipped: usize,
    empty_message: &str,
) -> Result<CollectionAddSummary, AppError> {
    // The engine's own input ceiling, applied here too: a CSV import is capped at this many
    // rows, and a deck's rows are caller-controlled (one per `PUT`, no cap of their own), so
    // the deck entry point must not be the one way past it. Far above any real deck.
    if rows.len() > MAX_IMPORT_ROWS {
        return Err(AppError::Validation(format!(
            "at most {MAX_IMPORT_ROWS} cards can be added to the collection at once"
        )));
    }

    // One holding per non-zero finish: the engine splits regular from foil by a flag, where a
    // deck row carries both counts. A row with neither (impossible for a stored deck card, which
    // both-zero deletes; defensive for a precon row stating a zero quantity) contributes nothing.
    let mut holdings: Vec<FetchedHolding> = Vec::with_capacity(rows.len());
    for row in rows {
        if row.quantity > 0 {
            holdings.push(FetchedHolding {
                external_card_id: row.external_id.clone(),
                foil: false,
                quantity: row.quantity,
            });
        }
        if row.foil_quantity > 0 {
            holdings.push(FetchedHolding {
                external_card_id: row.external_id,
                foil: true,
                quantity: row.foil_quantity,
            });
        }
    }
    if holdings.is_empty() {
        return Err(AppError::Validation(empty_message.to_string()));
    }

    let result = collection_import::merge_holdings(&state.db, user_id, game, holdings).await;
    // Orphan the user's cached analytics bodies (#413) on success AND failure, exactly as the
    // import handler does: the engine's star-holding fold commits in its own transaction ahead
    // of the plan apply, so a failed merge may still have changed the holdings.
    state.analytics_cache.bump_holdings(user_id, game).await;
    let outcome = result.map_err(AppError::from)?;

    Ok(CollectionAddSummary {
        cards: outcome.matched_cards,
        regular_copies: outcome.regular_copies,
        foil_copies: outcome.foil_copies,
        // A card that resolved when its rows were read but not when the engine looked it up
        // again is a sync racing this request; count it with the cards the caller dropped
        // rather than silently under-reporting. Both are distinct-card counts (the engine
        // aggregates by external id before it resolves), so the sum is one unit.
        skipped_cards: skipped + outcome.unmatched_cards,
    })
}

/// Add deck to collection
///
/// `POST /api/decks/{game}/{deck_id}/collection` -> add every card of one of the caller's
/// decks to their collection, **on top of** what they already own (the deck proper plus its
/// sideboard; maybeboard sections are skipped, as in `needed`). Returns what was added. Not
/// idempotent by design: a second call adds a second copy of everything. `404` when the deck
/// isn't the caller's; `422` when the deck has no cards outside its maybeboards.
#[utoipa::path(
    post,
    path = "/api/decks/{game}/{deck_id}/collection",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "What was added: distinct printings, regular + foil copies, and rows skipped because their card left the catalog.", body = CollectionAddSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 422, description = "The deck has no cards to add (empty, or only a maybeboard)."),
    ),
)]
pub async fn add_deck_to_collection(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<CollectionAddSummary>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let (rows, skipped) = deck_rows(&state, deck.id).await?;
    Ok(Json(
        add_rows_to_collection(
            &state,
            user.id,
            &game,
            rows,
            skipped,
            "this deck has no cards to add to your collection (its maybeboards don't count)",
        )
        .await?,
    ))
}

/// Add public deck to collection
///
/// `POST /api/u/{handle}/decks/{deck_id}/collection` -> add every card of someone's **public**
/// deck to the caller's own collection, on top of what they already own ("I bought the
/// singles for this list") — the deck proper plus its sideboard, maybeboards skipped, exactly
/// as for one of the caller's own decks. The source is addressed like the public read and the
/// copy: `404` when the handle is unknown or the deck is private/absent (one identical body —
/// no existence oracle). Not idempotent by design. `422` when the deck has nothing to add.
#[utoipa::path(
    post,
    path = "/api/u/{handle}/decks/{deck_id}/collection",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("handle" = String, Path, description = "The deck owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The (public) deck's id"),
    ),
    responses(
        (status = 200, description = "What was added to the caller's collection: distinct printings, regular + foil copies, and rows skipped because their card left the catalog.", body = CollectionAddSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown handle, or the deck is private/absent."),
        (status = 422, description = "The deck has no cards to add (empty, or only a maybeboard)."),
    ),
)]
pub async fn add_public_deck_to_collection(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((handle, deck_id)): Path<(String, i32)>,
) -> Result<Json<CollectionAddSummary>, AppError> {
    // The same seam the public read and the copy resolve through, so a private deck, an
    // unknown handle and a wrong owner collapse into the one identical 404 body.
    let (_owner, deck) = load_public_deck(&state, &handle, deck_id).await?;
    let (rows, skipped) = deck_rows(&state, deck.id).await?;
    // The holdings written are the CALLER's, for the source deck's game — the owner's
    // collection is never touched, and no per-game path segment is needed: a deck already
    // knows its game.
    Ok(Json(
        add_rows_to_collection(
            &state,
            user.id,
            &deck.game,
            rows,
            skipped,
            "this deck has no cards to add to your collection (its maybeboards don't count)",
        )
        .await?,
    ))
}

/// A deck's rows as the seam takes them, less the maybeboards — a card under consideration
/// was not bought — split into the rows that still have a catalog card and a count of those
/// that don't. Shared by the owner and the public entry points, so both skip the same
/// sections and count a gone card the same way.
///
/// Only the columns the seam reads are selected: the external id is what the engine resolves,
/// the internal id is how a gone card is counted once, and the ~70-column card row would be
/// dead weight on a deck whose row count the caller controls. The LEFT join hands a card gone after a re-import back as `None`, the
/// tolerance every deck read applies.
async fn deck_rows(
    state: &AppState,
    deck_id: i32,
) -> Result<(Vec<CollectionAddRow>, usize), AppError> {
    let rows: Vec<(i32, i32, i32, Option<String>)> = DeckCard::find()
        .select_only()
        .column(deck_card::Column::CardId)
        .column(deck_card::Column::Quantity)
        .column(deck_card::Column::FoilQuantity)
        .column(card::Column::ExternalId)
        .left_join(Card)
        .filter(deck_card::Column::DeckId.eq(deck_id))
        .filter(deck_card::Column::SectionId.not_in_subquery(maybeboard_section_ids(vec![deck_id])))
        .into_tuple()
        .all(&state.db)
        .await?;
    Ok(partition_rows(rows.into_iter().map(
        |(card_id, quantity, foil_quantity, external_id)| {
            (card_id, external_id, quantity, foil_quantity)
        },
    )))
}

/// Split source rows — `(internal card id, its external id if the catalog still has it,
/// regular, foil)` — into the ones that still have a catalog card (kept, as
/// [`CollectionAddRow`]s) and a count of **distinct cards** that don't (skipped, reported):
/// the same LEFT-join-then-skip tolerance every deck and precon read applies to a card gone
/// after a re-import. Counted per card, not per row, because `cards` in the summary is per
/// card too (the engine aggregates a printing held in two sections into one) — a gone card
/// in two sections is one card the user is told about, not two. Shared with the precon entry
/// point so both count a gone card the same way.
pub(crate) fn partition_rows(
    rows: impl IntoIterator<Item = (i32, Option<String>, i32, i32)>,
) -> (Vec<CollectionAddRow>, usize) {
    let mut kept = Vec::new();
    let mut skipped: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for (card_id, external_id, quantity, foil_quantity) in rows {
        match external_id {
            Some(external_id) => kept.push(CollectionAddRow {
                external_id,
                quantity,
                foil_quantity,
            }),
            None => {
                skipped.insert(card_id);
            }
        }
    }
    (kept, skipped.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row whose card is gone is counted, not dropped on the floor — the summary must be able
    /// to say "2 cards couldn't be added" — and counted per CARD: the gone card held in two
    /// sections (two rows, one internal id) is one card, the unit `cards` is stated in.
    #[test]
    fn rows_without_a_catalog_card_are_counted_as_skipped_per_card() {
        let (kept, skipped) = partition_rows(vec![
            (1, Some("a".to_string()), 1, 0),
            (2, None, 4, 0),
            (3, Some("b".to_string()), 0, 2),
            (2, None, 0, 1),
            (4, None, 1, 0),
        ]);
        assert_eq!(skipped, 2, "card 2 twice + card 4 once = two cards");
        let ids: Vec<&str> = kept.iter().map(|r| r.external_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!((kept[1].quantity, kept[1].foil_quantity), (0, 2));
    }
}
