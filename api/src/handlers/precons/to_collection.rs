//! Add a preconstructed deck's cards to the caller's collection — the write for "I bought
//! this precon".
//!
//! The precon page's second bridge back to the user's surfaces: [`copy`](super::copy) turns
//! the published list into a deck of theirs; this puts its cards into their collection. It is
//! the twin of the deck entry point in [`crate::handlers::decks::to_collection`] and writes
//! through that module's `add_rows_to_collection` seam — same additive `merge`, same foil-★
//! fold, same one-transaction apply — differing only in where the rows come from.
//!
//! Every board goes in. A precon has no maybeboard: its command zone, mainboard and sideboard
//! are all cards in the box, so unlike the deck entry point nothing is filtered out. A row is
//! a **single finish** (that is how a decklist reads), so it maps onto exactly one of the two
//! counts; a printing listed in both finishes (two rows by design — every Jumpstart theme and
//! bundle land pack) is two holdings the engine aggregates into one owned row, which is what
//! makes the collection say "3 regular + 1 foil" rather than showing the card twice.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use crate::auth::extractor::WritableUser;
use crate::entities::prelude::{Card, PreconDeckCard};
use crate::entities::{card, precon_deck_card};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::decks::{CollectionAddSummary, add_rows_to_collection, partition_rows};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::load_precon;

/// Add preconstructed deck to collection
///
/// `POST /api/decks/{game}/precons/{slug}/collection` -> add every card the precon ships
/// (command zone, mainboard and sideboard) to the caller's collection, **on top of** what
/// they already own. Returns what was added. Not idempotent by design: a second call records
/// a second copy of the deck. `404` for an unknown game or precon; `422` when none of its
/// cards is still in the catalog.
#[utoipa::path(
    post,
    path = "/api/decks/{game}/precons/{slug}/collection",
    tag = "Preconstructed decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
    ),
    responses(
        (status = 200, description = "What was added: distinct printings, regular + foil copies, and rows skipped because their card left the catalog.", body = CollectionAddSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game or preconstructed deck."),
        (status = 422, description = "None of the precon's cards is still in the catalog."),
    ),
)]
pub async fn add_precon_to_collection(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, slug)): Path<(String, String)>,
) -> Result<Json<CollectionAddSummary>, AppError> {
    require_game(&game)?;
    let precon = load_precon(&state, &game, &slug).await?;

    // Every board, LEFT-joined to the catalog card whose external id the engine resolves —
    // only that column, the internal id (so a gone card is counted once) and the row's own
    // count + finish, as the deck entry point selects. A row whose card is gone joins to None
    // and is counted as skipped, as the detail read and the copy tolerate it.
    let rows: Vec<(i32, i32, bool, Option<String>)> = PreconDeckCard::find()
        .select_only()
        .column(precon_deck_card::Column::CardId)
        .column(precon_deck_card::Column::Quantity)
        .column(precon_deck_card::Column::Foil)
        .column(card::Column::ExternalId)
        .left_join(Card)
        .filter(precon_deck_card::Column::PreconDeckId.eq(precon.id))
        .into_tuple()
        .all(&state.db)
        .await?;

    // A precon row states one finish; the seam's row carries both counts.
    let (rows, skipped) = partition_rows(rows.into_iter().map(
        |(card_id, quantity, foil, external_id)| {
            let counts = if foil { (0, quantity) } else { (quantity, 0) };
            (card_id, external_id, counts.0, counts.1)
        },
    ));

    Ok(Json(
        add_rows_to_collection(
            &state,
            user.id,
            &game,
            rows,
            skipped,
            "this preconstructed deck has no cards in the catalog to add to your collection",
        )
        .await?,
    ))
}
