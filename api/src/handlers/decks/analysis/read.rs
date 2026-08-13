//! The authenticated deck-analysis reads. Each one proves the deck is the caller's
//! ([`load_deck`]) and then hands off to the same `analyse_*` entry point the public
//! mirror in [`crate::handlers::sharing::decks`] calls, so a shared deck and its owner's
//! copy can never disagree about the deck's own analysis.

use axum::{Json, extract::State};

use crate::auth::extractor::AuthUser;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::super::load_deck;
use super::{
    DeckAnalytics, DeckBracketEstimate, DeckLegality, DeckTokens, GoldfishHand, GoldfishParams,
    StatsParams, analyse_bracket, analyse_goldfish, analyse_legality, analyse_stats,
    analyse_tokens, load_analysis, load_analysis_with_cards,
};

/// Deck analytics
///
/// `GET /api/decks/{game}/{deck_id}/stats` -> the deck's copy-weighted composition (mana
/// curve, colour identity, card types, copies, lands, average mana value), the same fold
/// over the shuffled library alone, and the hypergeometric draw-odds curve for one card.
/// `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/stats",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
        StatsParams,
    ),
    responses(
        (status = 200, description = "Composition of the deck and of its library, plus draw odds.", body = DeckAnalytics),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 422, description = "A section id in `sections` is not a number."),
    ),
)]
pub async fn deck_stats(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
    Query(params): Query<StatsParams>,
) -> Result<Json<DeckAnalytics>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(analyse_stats(&input, &params)?))
}

/// Deck legality
///
/// `GET /api/decks/{game}/{deck_id}/legality` -> the deck's verdict against its own
/// format: offending cards (banned / not legal / restricted / commander-only / off-colour /
/// over the copy limit), the deck-wide construction breaches, and whether it's legal.
/// `data` is **null** when the deck's format isn't one legality is tracked for — that means
/// "nothing to evaluate", never "illegal". `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/legality",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "The deck's legality verdict, or null when its format isn't tracked.", body = DataBody<Option<DeckLegality>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_legality(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DataBody<Option<DeckLegality>>>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(DataBody {
        data: analyse_legality(deck.format.as_deref(), &input),
    }))
}

/// Estimated Commander bracket
///
/// `GET /api/decks/{game}/{deck_id}/bracket` -> where the deck sits on Wizards' 1–5
/// Commander bracket ladder, estimated from its cards: the Game Changers, mass land denial,
/// extra turns, and tutors it holds, the reasons the estimate landed where it did, and what
/// it could not check. `data` is **null** unless the deck's format is Commander — the one
/// format the ladder is defined for. `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/bracket",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "The estimated bracket, or null when the deck isn't a Commander deck.", body = DataBody<Option<DeckBracketEstimate>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_bracket(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DataBody<Option<DeckBracketEstimate>>>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(DataBody {
        data: analyse_bracket(deck.format.as_deref(), &input),
    }))
}

/// Tokens the deck makes
///
/// `GET /api/decks/{game}/{deck_id}/tokens` -> the tokens and emblems the deck's cards make
/// — what a player has to bring to a game besides the deck — each with a printing of the
/// token and the cards that make it. Read off the catalog's per-card token relations, never
/// inferred from rules text, and scoped to the deck proper (a maybeboard card sends you
/// looking for nothing). `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/tokens",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "The tokens the deck makes, most-made first.", body = DeckTokens),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_tokens(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckTokens>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(analyse_tokens(&state, &game, &input).await?))
}

/// Goldfish a sample hand
///
/// `GET /api/decks/{game}/{deck_id}/goldfish` -> shuffle the deck's library and deal an
/// opening hand, optionally after London mulligans and a draw step. Deterministic: the
/// whole hand is a function of the query string, and the response echoes the `seed` back,
/// so the same URL always deals the same cards and a hand can be shared verbatim.
/// `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/goldfish",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
        GoldfishParams,
    ),
    responses(
        (status = 200, description = "The dealt hand, what was bottomed, and what's left in the library.", body = GoldfishHand),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 422, description = "A parameter is out of range, the library is too large to shuffle, or a bottomed card isn't in the hand."),
    ),
)]
pub async fn deck_goldfish(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
    Query(params): Query<GoldfishParams>,
) -> Result<Json<GoldfishHand>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let (input, models) = load_analysis_with_cards(&state, deck.id).await?;
    Ok(Json(analyse_goldfish(&input, &models, &params)?))
}
