//! Public collection sharing (issues #361/#362): per-(user, game) visibility, the
//! authed toggle, and the unauthenticated `/api/u/{handle}` read surface. The reads
//! reuse the collection's `user_id`-parameterised cores verbatim (no query is
//! duplicated); only the identity resolution differs — a public handle + a visibility
//! gate in place of an `AuthUser` extractor.

use chrono::Utc;
use sea_orm::sea_query::{Expr, Func, OnConflict};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, prelude::DateTimeUtc};
use serde::{Deserialize, Serialize};

use crate::db::Dialect;
use crate::entities::prelude::{CollectionVisibility as VisEntity, User};
use crate::entities::{collection_visibility as vis, user};
use crate::error::AppError;
use crate::state::AppState;

pub mod public;
pub mod visibility;

pub use public::{
    public_list, public_profile, public_set_drops, public_set_subtypes, public_sets, public_summary,
};
pub use visibility::{get_collection_visibility, set_collection_visibility};

// ---------- Request / response DTOs ----------

/// Body of `PUT /api/collection/{game}/visibility`.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetVisibilityRequest {
    pub public: bool,
}

/// The current public-visibility state for one (user, game). The wire field is `public`
/// (the DB column is `is_public`); `handle` is the owner's public handle (`alice-0001`),
/// or null until they choose a username — the SPA links the live view at
/// `/u/{handle}/{game}` when `public` is true.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionVisibility {
    pub public: bool,
    pub handle: Option<String>,
}

/// One public game on a profile: the game slug plus its collection summary.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PublicGameSummary {
    pub game: String,
    pub summary: crate::handlers::shared::CollectionSummary,
}

/// A user's public profile landing: their handle + every game they've made public.
/// Deliberately carries **no** email or other PII — only the public username/handle, the
/// account age, and the per-game summaries.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PublicProfile {
    pub username: String,
    pub discriminator: i32,
    pub handle: String,
    pub member_since: DateTimeUtc,
    pub games: Vec<PublicGameSummary>,
}

// ---------- Visibility DB helpers ----------

/// Whether this (user, game) collection is currently public.
pub(crate) async fn is_public(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<bool, AppError> {
    Ok(VisEntity::find()
        .filter(vis::Column::UserId.eq(user_id))
        .filter(vis::Column::Game.eq(game))
        .filter(vis::Column::IsPublic.eq(true))
        .one(db)
        .await?
        .is_some())
}

/// Upsert the (user, game) visibility row to `public`. The row is retained (flag flipped)
/// rather than deleted, so future per-collection display prefs survive a toggle.
pub(crate) async fn set_visibility(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
    public: bool,
) -> Result<(), AppError> {
    let now = Utc::now();
    let active = vis::ActiveModel {
        user_id: Set(user_id),
        game: Set(game.to_string()),
        is_public: Set(public),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    VisEntity::insert(active)
        .on_conflict(
            OnConflict::columns([vis::Column::UserId, vis::Column::Game])
                .update_columns([vis::Column::IsPublic, vis::Column::UpdatedAt])
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

/// The games this user has made public, sorted for a stable profile listing.
pub(crate) async fn public_games(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<Vec<String>, AppError> {
    Ok(VisEntity::find()
        .filter(vis::Column::UserId.eq(user_id))
        .filter(vis::Column::IsPublic.eq(true))
        .order_by_asc(vis::Column::Game)
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.game)
        .collect())
}

// ---------- Public-handle resolution ----------

/// A single 404 for every "this public thing isn't here" case — an unparseable handle,
/// an unknown user, or a game the user hasn't shared. Deliberately **404, not 403**, so
/// the surface never confirms that a handle exists or that a private game is merely
/// hidden (no existence oracle over `/api/u/...`).
fn not_here() -> AppError {
    AppError::NotFound("collection not found".to_string())
}

/// Parse `handle` → (username, discriminator) and load that user, or 404. Usernames are
/// stored case-preserving with case-insensitive uniqueness, so the lookup is
/// dialect-aware — mirroring the email precedent: SQLite matches the `COLLATE NOCASE`
/// column directly (case-insensitive + index-served), Postgres matches the functional
/// `lower(username)` index. Both hit the `(username, discriminator)` unique index.
pub(crate) async fn resolve_public_user(
    state: &AppState,
    handle: &str,
) -> Result<user::Model, AppError> {
    let (name, disc) = crate::auth::username::parse_handle(handle).ok_or_else(not_here)?;
    let query = User::find().filter(user::Column::Discriminator.eq(disc));
    let query = match state.dialect() {
        Dialect::Postgres => query.filter(
            Expr::expr(Func::lower(Expr::col(user::Column::Username)))
                .eq(crate::auth::username::normalize(&name)),
        ),
        Dialect::Sqlite => query.filter(user::Column::Username.eq(name)),
    };
    query.one(&state.db).await?.ok_or_else(not_here)
}

/// Resolve a handle and confirm `(user, game)` is public; yields the owner's `user_id`
/// for the read cores. 404 for an unknown handle OR a non-public game (same body).
pub(crate) async fn require_public_handle(
    state: &AppState,
    handle: &str,
    game: &str,
) -> Result<i32, AppError> {
    let user = resolve_public_user(state, handle).await?;
    if !is_public(&state.db, user.id, game).await? {
        return Err(not_here());
    }
    Ok(user.id)
}
