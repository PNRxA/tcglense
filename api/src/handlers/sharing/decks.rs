//! Unauthenticated public **deck** reads, addressed by handle: `/api/u/{handle}/decks...`.
//!
//! Per-deck sharing (issue #363) mirrors the per-collection sharing model (#361), but the
//! shareable unit is a single deck, so the flag is an `is_public` **column on the deck row**
//! (no separate visibility table): `public_deck` just loads the deck filtered on
//! `is_public`. Identity resolution reuses `resolve_public_user` verbatim. Every miss —
//! unknown handle, private/absent deck — is the same `404` (no existence oracle). Lives in
//! the router's `public_holdings` group (CDN-cacheable, ETag'd).

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::entities::deck;
use crate::entities::prelude::Deck;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::decks::{
    DeckAnalytics, DeckDetail, DeckLegality, DeckResponse, GoldfishHand, GoldfishParams,
    StatsParams, analyse_goldfish, analyse_legality, analyse_stats, card_counts_by_deck,
    deck_detail, load_analysis, load_analysis_with_cards,
};
use crate::handlers::shared::DataBody;
use crate::state::AppState;

use super::resolve_public_user;

/// A single 404 for every "no public deck here" case — unknown handle/user, or a
/// private/absent deck — so the surface never confirms a deck id or handle exists.
fn not_here() -> AppError {
    AppError::NotFound("deck not found".to_string())
}

/// Resolve a public handle, collapsing its "unknown handle" 404 into the same
/// [`not_here`] body a private/absent deck returns — so an unknown handle and a real
/// handle-with-nothing-public are indistinguishable (no username-enumeration oracle).
/// A genuine (non-404) error is preserved.
async fn resolve_or_not_here(
    state: &AppState,
    handle: &str,
) -> Result<crate::entities::user::Model, AppError> {
    resolve_public_user(state, handle)
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => not_here(),
            other => other,
        })
}

/// Resolve a public deck by its owner's handle + id, or a uniform [`not_here`] 404. Shared by
/// the public read ([`public_deck`]) and the authenticated copy
/// ([`copy_public_deck`](crate::handlers::decks::copy_public_deck)) so both collapse "unknown
/// handle" and "private/absent deck" into the identical 404 body — no existence oracle over
/// handles or deck ids. Returns the owner (for `handle_of`) alongside the deck.
pub(crate) async fn load_public_deck(
    state: &AppState,
    handle: &str,
    deck_id: i32,
) -> Result<(crate::entities::user::Model, deck::Model), AppError> {
    let user = resolve_or_not_here(state, handle).await?;
    let deck = Deck::find_by_id(deck_id)
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::IsPublic.eq(true))
        .one(&state.db)
        .await?
        .ok_or_else(not_here)?;
    Ok((user, deck))
}

/// Whether the user has at least one publicly-shared deck. Lets the public profile
/// (`public_profile`) still resolve for someone who has shared **only** decks and no
/// collection — without inventing a new oracle (their decks are already listable at
/// `/api/u/{handle}/decks`).
pub(super) async fn user_has_public_deck(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<bool, AppError> {
    Ok(Deck::find()
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::IsPublic.eq(true))
        .one(db)
        .await?
        .is_some())
}

/// List public decks
///
/// `GET /api/u/{handle}/decks` -> the owner's public decks (across games), newest first.
/// `404` when the handle is unknown **or** the user has no public deck — the same
/// non-oracle stance as the public profile (a valid handle with nothing public is
/// indistinguishable from an unknown one).
#[utoipa::path(
    get,
    path = "/api/u/{handle}/decks",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
    ),
    responses(
        (status = 200, description = "The owner's public decks across games, newest first.", body = DataBody<Vec<DeckResponse>>),
        (status = 404, description = "Unknown handle, or the user has no public deck."),
    ),
)]
pub async fn public_decks(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> Result<Json<DataBody<Vec<DeckResponse>>>, AppError> {
    let user = resolve_or_not_here(&state, &handle).await?;
    let decks = Deck::find()
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::IsPublic.eq(true))
        .order_by_desc(deck::Column::UpdatedAt)
        .order_by_desc(deck::Column::Id)
        .all(&state.db)
        .await?;
    if decks.is_empty() {
        return Err(not_here());
    }

    let ids: Vec<i32> = decks.iter().map(|d| d.id).collect();
    let counts = card_counts_by_deck(&state.db, &ids).await?;
    let data = decks
        .iter()
        .map(|d| DeckResponse::from_model(d, counts.get(&d.id).copied().unwrap_or(0)))
        .collect();
    Ok(Json(DataBody { data }))
}

/// Get public deck
///
/// `GET /api/u/{handle}/decks/{deck_id}` -> a public deck's full detail (the shareable
/// view). `404` when the handle is unknown or the deck is private/absent. Carries the owner
/// handle (so the SPA can link the author) but no other PII.
#[utoipa::path(
    get,
    path = "/api/u/{handle}/decks/{deck_id}",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The deck's id"),
    ),
    responses(
        (status = 200, description = "The public deck's full detail (the shareable view).", body = DeckDetail),
        (status = 404, description = "Unknown handle, or the deck is private/absent."),
    ),
)]
pub async fn public_deck(
    State(state): State<AppState>,
    Path((handle, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckDetail>, AppError> {
    let (user, deck) = load_public_deck(&state, &handle, deck_id).await?;
    let handle = crate::auth::username::handle_of(&user);
    Ok(Json(deck_detail(&state, &deck, handle).await?))
}

// ---------- Analysis mirrors (issue #596) ----------
//
// The three deck-analysis reads, for a deck whose owner shared it. Each resolves the deck
// through the same `load_public_deck` gate as `public_deck` — so an unknown handle and a
// private deck stay the one indistinguishable 404 — and then calls the identical
// `analyse_*` entry point the owner's own read uses. There is deliberately no second
// implementation here: a public deck and its owner's copy must never disagree about the
// deck's composition, its legality, or the hand a seed deals.

/// Public deck analytics
///
/// `GET /api/u/{handle}/decks/{deck_id}/stats` -> the composition and draw odds of a public
/// deck, identical to what its owner sees. `404` when the handle is unknown or the deck is
/// private/absent.
#[utoipa::path(
    get,
    path = "/api/u/{handle}/decks/{deck_id}/stats",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The deck's id"),
        StatsParams,
    ),
    responses(
        (status = 200, description = "Composition of the deck and of its library, plus draw odds.", body = DeckAnalytics),
        (status = 404, description = "Unknown handle, or the deck is private/absent."),
        (status = 422, description = "A section id in `sections` is not a number."),
    ),
)]
pub async fn public_deck_stats(
    State(state): State<AppState>,
    Path((handle, deck_id)): Path<(String, i32)>,
    Query(params): Query<StatsParams>,
) -> Result<Json<DeckAnalytics>, AppError> {
    let (_, deck) = load_public_deck(&state, &handle, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(analyse_stats(&input, &params)?))
}

/// Public deck legality
///
/// `GET /api/u/{handle}/decks/{deck_id}/legality` -> a public deck's legality verdict
/// against its own format. `data` is null when that format isn't one legality is tracked
/// for. `404` when the handle is unknown or the deck is private/absent.
#[utoipa::path(
    get,
    path = "/api/u/{handle}/decks/{deck_id}/legality",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The deck's id"),
    ),
    responses(
        (status = 200, description = "The deck's legality verdict, or null when its format isn't tracked.", body = DataBody<Option<DeckLegality>>),
        (status = 404, description = "Unknown handle, or the deck is private/absent."),
    ),
)]
pub async fn public_deck_legality(
    State(state): State<AppState>,
    Path((handle, deck_id)): Path<(String, i32)>,
) -> Result<Json<DataBody<Option<DeckLegality>>>, AppError> {
    let (_, deck) = load_public_deck(&state, &handle, deck_id).await?;
    let input = load_analysis(&state, deck.id).await?;
    Ok(Json(DataBody {
        data: analyse_legality(deck.format.as_deref(), &input),
    }))
}

/// Goldfish a public deck
///
/// `GET /api/u/{handle}/decks/{deck_id}/goldfish` -> deal a sample hand from a public deck,
/// with the same seeded, stateless engine the owner's own read uses — so a hand can be
/// shared as a URL by anyone who can see the deck. `404` when the handle is unknown or the
/// deck is private/absent.
#[utoipa::path(
    get,
    path = "/api/u/{handle}/decks/{deck_id}/goldfish",
    tag = "Public sharing",
    params(
        ("handle" = String, Path, description = "The owner's public handle, e.g. `alice-0001`"),
        ("deck_id" = i32, Path, description = "The deck's id"),
        GoldfishParams,
    ),
    responses(
        (status = 200, description = "The dealt hand, what was bottomed, and what's left in the library.", body = GoldfishHand),
        (status = 404, description = "Unknown handle, or the deck is private/absent."),
        (status = 422, description = "A parameter is out of range, or a bottomed card isn't in the hand."),
    ),
)]
pub async fn public_deck_goldfish(
    State(state): State<AppState>,
    Path((handle, deck_id)): Path<(String, i32)>,
    Query(params): Query<GoldfishParams>,
) -> Result<Json<GoldfishHand>, AppError> {
    let (_, deck) = load_public_deck(&state, &handle, deck_id).await?;
    let (input, models) = load_analysis_with_cards(&state, deck.id).await?;
    Ok(Json(analyse_goldfish(&input, &models, &params)?))
}
