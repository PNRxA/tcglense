//! Authenticated, per-user wish-list endpoints.
//!
//! A wish list records how many copies of each card a signed-in user wants to buy, per
//! game (`/api/wishlist/{game}/...`) — the collection's "want" twin, with the same
//! holding shape (`(user, game, card) -> { quantity, foil_quantity }`, both-zero
//! deletes the row) but its own `wishlist_items` table, so a card can be owned and
//! wanted independently. Every route requires a valid access token (via
//! [`AuthUser`](crate::auth::extractor::AuthUser)) and is wired into the router's
//! `private` group, so responses are `Cache-Control: no-store` — per-user data must
//! never be shared-cached.
//!
//! Card ids in the path are the provider's **external** id (the same id the public
//! catalog exposes); each is resolved to the internal `cards.id` before storage, so a
//! wish-list row survives a catalog re-import. Rows are always scoped by `user.id`
//! from the token, so one user can never read or mutate another's wish list.
//!
//! The handlers are split across submodules by concern — [`read`] (list / summary /
//! wanted-count reads), [`sets`] (per-set landing + by-drop), and [`write`] (the
//! wanted-count upsert) — mirroring `handlers::collection` minus its import/sync (a
//! wish list has nothing to import). The wire DTOs and params are the collection's
//! own, reused from [`crate::handlers::shared::holdings`] so the wish list needs no
//! new generated TS types; on this side of the API their "owned" fields simply read
//! as "wanted".

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::entities::prelude::WishlistItem;
use crate::entities::wishlist_item;
use crate::error::AppError;
use crate::state::AppState;

mod read;
mod sets;
mod write;

#[cfg(test)]
mod tests;

pub use read::{get_wishlist_entry, list_wishlist, wishlist_counts, wishlist_summary};
pub use sets::{wishlist_set_drops, wishlist_set_subtypes, wishlist_sets};
pub use write::set_wishlist_entry;

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::wishlist::__path_<fn>`
// (see the note in `crate::handlers::catalog`).
pub use read::{__path_get_wishlist_entry, __path_list_wishlist, __path_wishlist_summary};
pub use write::__path_set_wishlist_entry;

/// The user's wish-list row for a card, if any. Shared by the get/set entry handlers.
async fn find_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    card_id: i32,
) -> Result<Option<wishlist_item::Model>, AppError> {
    Ok(WishlistItem::find()
        .filter(wishlist_item::Column::UserId.eq(user_id))
        .filter(wishlist_item::Column::Game.eq(game))
        .filter(wishlist_item::Column::CardId.eq(card_id))
        .one(&state.db)
        .await?)
}
