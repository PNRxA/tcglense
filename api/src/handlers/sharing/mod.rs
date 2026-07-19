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

pub mod decks;
pub mod public;
pub mod visibility;
pub mod wishlist_visibility;

pub use decks::{public_deck, public_decks};
pub use public::{
    public_list, public_owned_counts, public_product_sets, public_product_summary, public_products,
    public_profile, public_set_drops, public_set_subtypes, public_sets, public_summary,
    public_wishlist_list, public_wishlist_owned_counts, public_wishlist_product_sets,
    public_wishlist_product_summary, public_wishlist_products, public_wishlist_set_drops,
    public_wishlist_set_subtypes, public_wishlist_sets, public_wishlist_summary,
};
pub use visibility::{get_collection_visibility, set_collection_visibility};
pub use wishlist_visibility::{get_wishlist_visibility, set_wishlist_visibility};

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::sharing::__path_<fn>`.
pub use decks::{__path_public_deck, __path_public_decks};
pub use public::{
    __path_public_list, __path_public_owned_counts, __path_public_product_sets,
    __path_public_product_summary, __path_public_products, __path_public_profile,
    __path_public_set_drops, __path_public_set_subtypes, __path_public_sets, __path_public_summary,
    __path_public_wishlist_list, __path_public_wishlist_owned_counts,
    __path_public_wishlist_product_sets, __path_public_wishlist_product_summary,
    __path_public_wishlist_products, __path_public_wishlist_set_drops,
    __path_public_wishlist_set_subtypes, __path_public_wishlist_sets,
    __path_public_wishlist_summary,
};
pub use visibility::{__path_get_collection_visibility, __path_set_collection_visibility};
pub use wishlist_visibility::{__path_get_wishlist_visibility, __path_set_wishlist_visibility};

// ---------- Request / response DTOs ----------

/// Body of `PUT /api/collection/{game}/visibility` — a partial patch of the (user, game)
/// row. Every field is optional (serde absent -> `None`); only the ones present are
/// written, so the sharing toggle and each display toggle can PATCH just their own field
/// without clobbering the others. An all-absent body is a no-op that echoes the current
/// state.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetVisibilityRequest {
    /// Enable/disable public sharing. Enabling requires a username first (409 otherwise).
    pub public: Option<bool>,
    /// Show/hide the value-over-time chart on the owner's collection landing.
    pub show_value_chart: Option<bool>,
    /// Show/hide the biggest-movers panel on the owner's collection landing.
    pub show_movers: Option<bool>,
}

/// The current visibility + display state for one (user, game). The wire field is `public`
/// (the DB column is `is_public`); `show_value_chart` / `show_movers` are the owner's
/// collection-landing display prefs (issue #381), both default true. `handle` is the
/// owner's public handle (`alice-0001`), or null until they choose a username — the SPA
/// links the live view at `/u/{handle}/{game}` when `public` is true.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionVisibility {
    pub public: bool,
    pub show_value_chart: bool,
    pub show_movers: bool,
    pub handle: Option<String>,
}

/// Body of `PUT /api/wishlist/{game}/visibility` (issue #493) — a partial patch of the
/// (user, game) wish-list sharing flag. A wish list is a shopping list, so — unlike a
/// collection — it carries no landing display prefs; the only knob is public/private. Absent
/// = a no-op echoing the current state.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetWishlistVisibilityRequest {
    /// Enable/disable public sharing of this game's wish list. Enabling requires a username
    /// first (409 otherwise), exactly like the collection toggle.
    pub public: Option<bool>,
}

/// The current wish-list visibility state for one (user, game) — the wire field is `public`
/// (the DB column is `wishlist_is_public`). `handle` is the owner's public handle
/// (`alice-0001`), or null until they choose a username — the SPA links the live view at
/// `/u/{handle}/wishlist/{game}` when `public` is true.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct WishlistVisibility {
    pub public: bool,
    pub handle: Option<String>,
}

/// One public game on a profile: the game slug plus its collection summary.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PublicGameSummary {
    pub game: String,
    pub summary: crate::handlers::shared::CollectionSummary,
}

/// A user's public profile landing: their handle + every game whose collection and/or wish
/// list they've made public. Deliberately carries **no** email or other PII — only the public
/// username/handle, the account age, and the per-game summaries. `games` lists the public
/// **collections** (each links to `/u/{handle}/{game}`); `wishlists` the public **wish lists**
/// (each links to `/u/{handle}/wishlist/{game}`, issue #493) — the two are shared independently,
/// so a game can appear in one, both, or neither.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PublicProfile {
    pub username: String,
    pub discriminator: i32,
    pub handle: String,
    #[schema(value_type = String, format = DateTime)]
    pub member_since: DateTimeUtc,
    pub games: Vec<PublicGameSummary>,
    pub wishlists: Vec<PublicGameSummary>,
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

/// Whether this (user, game) **wish list** is currently public (issue #493). The wish-list
/// twin of [`is_public`], reading the independent `wishlist_is_public` column on the same row.
pub(crate) async fn wishlist_is_public(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<bool, AppError> {
    Ok(VisEntity::find()
        .filter(vis::Column::UserId.eq(user_id))
        .filter(vis::Column::Game.eq(game))
        .filter(vis::Column::WishlistIsPublic.eq(true))
        .one(db)
        .await?
        .is_some())
}

/// The visibility + display state for one (user, game): the row's values, or the defaults
/// (private, both landing sections shown) when no row exists yet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VisibilityState {
    pub is_public: bool,
    pub show_value_chart: bool,
    pub show_movers: bool,
    pub wishlist_is_public: bool,
}

impl Default for VisibilityState {
    fn default() -> Self {
        // No row yet: private (both collection and wish list), and both optional collection-
        // landing sections shown.
        Self {
            is_public: false,
            show_value_chart: true,
            show_movers: true,
            wishlist_is_public: false,
        }
    }
}

/// Load the (user, game) visibility/display state, or the defaults when no row exists.
pub(crate) async fn visibility_state(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<VisibilityState, AppError> {
    Ok(VisEntity::find()
        .filter(vis::Column::UserId.eq(user_id))
        .filter(vis::Column::Game.eq(game))
        .one(db)
        .await?
        .map(|r| VisibilityState {
            is_public: r.is_public,
            show_value_chart: r.show_value_chart,
            show_movers: r.show_movers,
            wishlist_is_public: r.wishlist_is_public,
        })
        .unwrap_or_default())
}

/// Apply a partial patch to the (user, game) visibility/display row, returning the
/// resulting state. Absent fields are left untouched: the INSERT seeds them from the
/// defaults (so a first-ever write of a single pref still creates a complete row), and the
/// ON CONFLICT updates ONLY the columns actually provided — so two concurrent single-field
/// toggles (e.g. flipping sharing and a display pref at once) can't clobber each other the
/// way a read-modify-write would. The row is retained (never deleted) so prefs survive a
/// private -> public -> private toggle.
pub(crate) async fn apply_visibility_patch(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
    public: Option<bool>,
    show_value_chart: Option<bool>,
    show_movers: Option<bool>,
    wishlist_public: Option<bool>,
) -> Result<VisibilityState, AppError> {
    // Nothing to change: echo the current state without touching the row.
    if public.is_none()
        && show_value_chart.is_none()
        && show_movers.is_none()
        && wishlist_public.is_none()
    {
        return visibility_state(db, user_id, game).await;
    }

    let now = Utc::now();
    let defaults = VisibilityState::default();
    let active = vis::ActiveModel {
        user_id: Set(user_id),
        game: Set(game.to_string()),
        is_public: Set(public.unwrap_or(defaults.is_public)),
        show_value_chart: Set(show_value_chart.unwrap_or(defaults.show_value_chart)),
        show_movers: Set(show_movers.unwrap_or(defaults.show_movers)),
        wishlist_is_public: Set(wishlist_public.unwrap_or(defaults.wishlist_is_public)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let mut update_columns = vec![vis::Column::UpdatedAt];
    if public.is_some() {
        update_columns.push(vis::Column::IsPublic);
    }
    if show_value_chart.is_some() {
        update_columns.push(vis::Column::ShowValueChart);
    }
    if show_movers.is_some() {
        update_columns.push(vis::Column::ShowMovers);
    }
    if wishlist_public.is_some() {
        update_columns.push(vis::Column::WishlistIsPublic);
    }

    VisEntity::insert(active)
        .on_conflict(
            OnConflict::columns([vis::Column::UserId, vis::Column::Game])
                .update_columns(update_columns)
                .to_owned(),
        )
        .exec(db)
        .await?;

    // Re-read so the response reflects the untouched columns (and any concurrent write).
    visibility_state(db, user_id, game).await
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

/// The games whose **wish list** this user has made public (issue #493), sorted for a stable
/// profile listing — the wish-list twin of [`public_games`].
pub(crate) async fn public_wishlist_games(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<Vec<String>, AppError> {
    Ok(VisEntity::find()
        .filter(vis::Column::UserId.eq(user_id))
        .filter(vis::Column::WishlistIsPublic.eq(true))
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

/// Resolve a handle and confirm `(user, game)` has a **public wish list** (issue #493);
/// yields the owner's `user_id` for the wish-list read cores. 404 for an unknown handle OR a
/// non-public wish list (same body) — the wish-list twin of [`require_public_handle`], gating
/// on the independent `wishlist_is_public` flag so a public collection never leaks the wish
/// list and vice versa.
pub(crate) async fn require_public_wishlist_handle(
    state: &AppState,
    handle: &str,
    game: &str,
) -> Result<i32, AppError> {
    let user = resolve_public_user(state, handle).await?;
    if !wishlist_is_public(&state.db, user.id, game).await? {
        return Err(not_here());
    }
    Ok(user.id)
}
