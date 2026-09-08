//! The authenticated deck-analysis reads. Each one proves the deck is the caller's
//! ([`load_deck`]) and then hands off to the same `analyse_*` entry point the public
//! mirror in [`crate::handlers::sharing::decks`] calls, so a shared deck and its owner's
//! copy can never disagree about the deck's own analysis.

use std::hash::{Hash, Hasher};

use axum::{Json, extract::State};

use crate::analytics_cache::json_body_response;
use crate::auth::extractor::AuthUser;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::super::load_deck;
use super::suggestions::deck_side;
use super::{
    DeckAnalysisInput, DeckAnalytics, DeckBracketEstimate, DeckLegality, DeckManaBase, DeckPricing,
    DeckRoles, DeckSuggestions, DeckTokens, GoldfishHand, GoldfishParams, StatsParams,
    analyse_bracket, analyse_goldfish, analyse_legality, analyse_mana, analyse_pricing,
    analyse_roles, analyse_stats, analyse_suggestions, analyse_tokens, load_analysis,
    load_analysis_with_cards,
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

/// Card roles
///
/// `GET /api/decks/{game}/{deck_id}/roles` -> how many pieces of ramp, card draw, removal,
/// board wipes, counterspells, tutors, recursion and protection the deck holds, read off each
/// card's rules text over the deck proper (command zone in, maybeboards out), with the
/// counted cards per role and a per-printing map for filtering a list. Every role is always
/// reported, and a card may fill several. `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/roles",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "The deck's role counts, the cards behind each, and which roles each printing fills.", body = DeckRoles),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_roles(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckRoles>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(analyse_roles(&input)))
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

/// Deck mana base
///
/// `GET /api/decks/{game}/{deck_id}/mana` -> the deck's colour requirements against its
/// sources: per colour, the pips its spells demand (with the most colour-hungry cards), the
/// sources its library produces (lands and nonland producers, each listed), the number Frank
/// Karsten's 2022 tables say a deck this size needs for the hungriest spell, and a plain
/// verdict ("Short 2 black sources"). Demand is the library plus the command zone; supply is
/// the library alone. `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/mana",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "Per-colour pips, sources, Karsten's threshold and the verdict.", body = DeckManaBase),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_mana(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckManaBase>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(analyse_mana(deck.format.as_deref(), &input)))
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

/// Deck pricing breakdown
///
/// `GET /api/decks/{game}/{deck_id}/pricing` -> where the deck's value is (issue #672):
/// every row of the deck proper priced as held, most expensive first, each with the
/// cheapest priced printing of its card **at the row's own finish split** (what the printing
/// swap would land on) and the saving; plus the deck's total (identical to the detail's
/// `summary.total_value_usd`), the total after every known saving, and the saving itself.
/// `null` is "unpriced", never `$0.00`. `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/pricing",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "The deck's rows priced as held, most expensive first, each with its cheapest printing and saving, plus the totals.", body = DeckPricing),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_pricing(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckPricing>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let (input, models) = load_analysis_with_cards(&state, deck.id).await?;
    Ok(Json(analyse_pricing(&state, &game, &input, &models).await?))
}

/// Cards you own that this deck could play
///
/// `GET /api/decks/{game}/{deck_id}/suggestions` -> the cards in the caller's collection the
/// deck could play (issue #684): legal in the deck's format, inside its colour identity (the
/// command zone's when it leads the deck, else the union over the deck proper), not already
/// in the deck, ranked by EDHREC's **global** popularity and grouped by the role each fills,
/// with the deck's own count per role beside them. Honest about what it is: global
/// popularity, not per-commander synergy, and the `caveats` say so. Reads the caller's
/// collection, so it has **no public mirror**. `404` if the deck isn't the caller's.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/suggestions",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 200, description = "Owned cards the deck could play, most popular first, overall and per role, with the filters that were applied.", body = DeckSuggestions),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn deck_suggestions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<axum::response::Response, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let (input, models) = load_analysis_with_cards(&state, deck.id).await?;

    // Memoised in the analytics cache (issues #413/#365): the answer is a function of the
    // caller's holdings (the holdings version), the catalog's prices (the price epoch, since
    // prices ride the cards on the wire), the day, and the deck itself — which those two
    // counters don't cover, so the deck's format and rows are fingerprinted into the params.
    // A deck edit therefore misses rather than serving the pre-edit answer, and the whole
    // collection scan runs once per (deck, holdings, prices, day) rather than per mount.
    // `None` key = cache degraded, compute as normal.
    let fingerprint = deck_fingerprint(deck.format.as_deref(), &input);
    let cache_key = state
        .analytics_cache
        .body_key(
            user.id,
            &game,
            "deck-suggestions",
            &format!("{}:{fingerprint:016x}", deck.id),
        )
        .await;
    let side = deck_side(deck.format.as_deref(), &input, &models);
    let body = state
        .analytics_cache
        .get_or_compute(cache_key, || {
            let (state, game) = (state.clone(), game.clone());
            let side = side.clone();
            async move {
                let payload = analyse_suggestions(&state, user.id, &game, side).await?;
                serde_json::to_vec(&payload)
                    .map_err(|err| AppError::Internal(format!("serialize suggestions: {err}")))
            }
        })
        .await?;
    Ok(json_body_response(body))
}

/// A stable digest of everything about the deck the suggestions read depends on: its format
/// and every row's printing, section, maybeboard flag and counts. Section names matter too
/// (they decide the zone split), so they ride along.
fn deck_fingerprint(format: Option<&str>, input: &DeckAnalysisInput) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    format.hash(&mut hasher);
    for section in &input.sections {
        (section.id, section.name.as_str(), section.is_maybeboard).hash(&mut hasher);
    }
    for entry in &input.entries {
        (
            entry.facts.id.as_str(),
            entry.section_id,
            entry.quantity,
            entry.foil_quantity,
        )
            .hash(&mut hasher);
    }
    hasher.finish()
}
