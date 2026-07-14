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
use crate::extract::Path;
use crate::handlers::decks::{DeckDetail, DeckResponse, card_counts_by_deck, deck_detail};
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

/// `GET /api/u/{handle}/decks` -> the owner's public decks (across games), newest first.
/// `404` when the handle is unknown **or** the user has no public deck — the same
/// non-oracle stance as the public profile (a valid handle with nothing public is
/// indistinguishable from an unknown one).
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

/// `GET /api/u/{handle}/decks/{deck_id}` -> a public deck's full detail (the shareable
/// view). `404` when the handle is unknown or the deck is private/absent. Carries the owner
/// handle (so the SPA can link the author) but no other PII.
pub async fn public_deck(
    State(state): State<AppState>,
    Path((handle, deck_id)): Path<(String, i32)>,
) -> Result<Json<DeckDetail>, AppError> {
    let user = resolve_or_not_here(&state, &handle).await?;
    let deck = Deck::find_by_id(deck_id)
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::IsPublic.eq(true))
        .one(&state.db)
        .await?
        .ok_or_else(not_here)?;
    let handle = crate::auth::username::handle_of(&user);
    Ok(Json(deck_detail(&state, &deck, handle).await?))
}
