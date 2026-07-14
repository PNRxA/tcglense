//! Deck read endpoints: the deck list (headers + card counts) and the full single-deck
//! view (metadata + sections + every card + a value summary).

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{Card, Deck, DeckCard, DeckSection};
use crate::entities::{card, deck, deck_card, deck_section};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::valuation::resolve_bulk_threshold_cents;
use crate::handlers::shared::{CardResponse, DataBody, require_game, summarize_holdings};
use crate::state::AppState;

use super::{
    DeckCardEntry, DeckDetail, DeckResponse, DeckSectionResponse, card_counts_by_deck, load_deck,
};

/// `GET /api/decks/{game}` -> the signed-in user's decks for a game, most-recently-updated
/// first, each with its total card count. Not paginated (a user has few decks); returns
/// `{ data: Deck[] }`.
pub async fn list_decks(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<DeckResponse>>>, AppError> {
    require_game(&game)?;

    let decks = Deck::find()
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game))
        .order_by_desc(deck::Column::UpdatedAt)
        .order_by_desc(deck::Column::Id)
        .all(&state.db)
        .await?;

    let ids: Vec<i32> = decks.iter().map(|d| d.id).collect();
    let counts = card_counts_by_deck(&state.db, &ids).await?;

    let data = decks
        .iter()
        .map(|d| DeckResponse::from_model(d, counts.get(&d.id).copied().unwrap_or(0)))
        .collect();
    Ok(Json(DataBody { data }))
}

/// `GET /api/decks/{game}/{deck_id}` -> the full deck: metadata, sections in order, every
/// card, and the value/copy summary. `404` if the deck isn't the caller's.
pub async fn get_deck(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckDetail>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let handle = crate::auth::username::handle_of(&user);
    Ok(Json(deck_detail(&state, &deck, handle).await?))
}

/// Build the full [`DeckDetail`] for a deck, parameterised by the owner's `handle` so the
/// authed reader ([`get_deck`]) and the public reader
/// ([`crate::handlers::sharing::decks::public_deck`]) share the exact query + shaping.
///
/// A deck card whose catalog row is gone (a re-import) is LEFT-joined to `None` and
/// skipped — for the card list *and* the summary fold — exactly as the collection reads do.
pub(crate) async fn deck_detail(
    state: &AppState,
    deck: &deck::Model,
    handle: Option<String>,
) -> Result<DeckDetail, AppError> {
    let sections: Vec<DeckSectionResponse> = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?
        .into_iter()
        .map(DeckSectionResponse::from)
        .collect();

    let rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .order_by_asc(card::Column::Name)
        .order_by_asc(deck_card::Column::Id)
        .all(&state.db)
        .await?;

    // Value/copy aggregates reuse the shared fold (the bulk slice is unused by the deck UI).
    let summary = summarize_holdings(&rows, resolve_bulk_threshold_cents(None));

    let cards: Vec<DeckCardEntry> = rows
        .into_iter()
        .filter_map(|(item, card)| {
            card.map(|c| DeckCardEntry {
                card: CardResponse::from(c),
                section_id: item.section_id,
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();

    Ok(DeckDetail {
        id: deck.id,
        game: deck.game.clone(),
        name: deck.name.clone(),
        description: deck.description.clone(),
        format: deck.format.clone(),
        folder_id: deck.folder_id,
        is_public: deck.is_public,
        handle,
        summary,
        sections,
        cards,
        created_at: deck.created_at,
        updated_at: deck.updated_at,
    })
}
