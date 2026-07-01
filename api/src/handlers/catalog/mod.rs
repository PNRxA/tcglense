//! Public, game-agnostic card-catalog endpoints.
//!
//! All routes are unauthenticated reads of card data, namespaced by `game`
//! (`/api/games/{game}/...`) so every supported TCG shares one URL shape and one
//! set of handlers. The image route is a lazy caching proxy (see
//! [`crate::catalog::images`]).
//!
//! The handlers are split across submodules by concern — [`status`] (game list +
//! import status), [`sets`] (sets, set cards, by-drop), [`cards`] (card lists +
//! detail + other printings), [`prices`] (price history), and [`image`] (the image
//! proxy) — with the shared query params and card helpers kept here.

use sea_orm::{
    ColumnTrait, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect, Select,
    sea_query::{Expr, LikeExpr, NullOrdering},
};
use serde::Deserialize;

use crate::catalog::Game;
use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::handlers::shared::{
    DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, MAX_DROP_PAGE_SIZE, MAX_PAGE_SIZE, SortDir, SortField,
    resolve_page, search_condition, trim_query,
};
use crate::scryfall::search::escape_like;

mod cards;
mod image;
mod prices;
mod sets;
mod status;

#[cfg(test)]
mod tests;

pub use cards::{card_names, card_prints, get_card, list_cards};
pub use image::card_image;
pub use prices::card_prices;
pub use sets::{get_set, list_set_cards, list_set_drops, list_sets, set_icon};
pub use status::{ingest_status, list_games};

/// Card art for a given id is immutable, so it is safe to cache aggressively.
const IMAGE_CACHE_CONTROL: &str = "public, max-age=2592000, immutable";

// ---------- Query params ----------

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    #[serde(default)]
    pub q: Option<String>,
    /// Set-cards only: when `true`, span the set's whole group (its top-level
    /// root plus every related sub-set) instead of just the one set. Ignored by
    /// the all-cards endpoint.
    #[serde(default)]
    pub include_related: Option<bool>,
    /// Sort key (`number`/`name`/`rarity`/`released`/`cmc`/`price`). Absent =
    /// the endpoint's natural default. Unknown values are a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`). Absent = the sort field's natural
    /// direction. Unknown values are a 422.
    #[serde(default)]
    pub dir: Option<String>,
    /// All-cards endpoint only: filter to the printings whose name matches this
    /// **exactly** (bound as a parameter, so any punctuation/quotes are literal).
    /// Powers the collection quick-add's "pick a printing of this name" step; a
    /// blank/absent value is ignored. Not honoured by the set-cards endpoint.
    #[serde(default)]
    pub name: Option<String>,
}

impl ListParams {
    fn page_and_size(&self) -> (u64, u64) {
        resolve_page(self.page, self.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE)
    }

    /// Page + page size for the by-drop listing, which paginates over drops
    /// (not cards) and so has its own smaller bounds.
    fn drop_page_and_size(&self) -> (u64, u64) {
        resolve_page(
            self.page,
            self.page_size,
            DEFAULT_DROP_PAGE_SIZE,
            MAX_DROP_PAGE_SIZE,
        )
    }

    fn search(&self) -> Option<&str> {
        trim_query(self.q.as_deref())
    }

    /// The trimmed exact-name filter, or `None` when absent/blank.
    fn exact_name(&self) -> Option<&str> {
        trim_query(self.name.as_deref())
    }

    /// Resolve the `sort`/`dir` params into a validated `(field, direction)`,
    /// falling back to `default` (and the field's natural direction) when a value
    /// is absent. An unrecognised value is a 422 — consistent with a malformed `q`
    /// — rather than being silently ignored.
    fn sort_spec(&self, default: SortField) -> Result<(SortField, SortDir), AppError> {
        let field = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => default,
            Some(value) => SortField::parse(value)?,
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => field.default_dir(),
            Some(value) => SortDir::parse(value)?,
        };
        Ok((field, dir))
    }
}

#[derive(Debug, Deserialize)]
pub struct ImageParams {
    pub size: Option<String>,
    pub face: Option<usize>,
}

/// Query params for the price-history endpoint.
#[derive(Debug, Deserialize)]
pub struct PriceParams {
    /// Window + resolution (`7d`/`30d`/`1y`/`2y`/`3y`/`all`). Absent/blank = the
    /// full daily series; an unknown value is a 422.
    #[serde(default)]
    pub range: Option<String>,
}

/// Query params for the card-name autocomplete endpoint.
#[derive(Debug, Deserialize)]
pub struct NameSuggestParams {
    /// The substring to match card names against (case-insensitively). Absent/blank
    /// yields an empty result — there's nothing to suggest yet.
    #[serde(default)]
    pub q: Option<String>,
    /// How many suggestions to return, clamped to `[1, MAX_NAME_SUGGESTIONS]`.
    /// Absent = `DEFAULT_NAME_SUGGESTIONS`.
    #[serde(default)]
    pub limit: Option<u64>,
}

// ---------- Shared card helpers ----------

/// Query a card's **other** printings: same game and `oracle_id`, excluding the
/// card itself (`exclude_id`). Ordered newest printing first (released date desc,
/// nulls last), then set code and collector number, with a stable `id` tiebreaker
/// so the order is deterministic. `oracle_id` is the gameplay-identity key shared
/// across all printings of a card.
///
/// Capped at `MAX_PAGE_SIZE` results: a handful of cards (e.g. basic lands) share
/// one `oracle_id` across hundreds-to-thousands of printings, and this is a "see
/// also" aid rather than an exhaustive listing, so it returns at most the newest
/// `MAX_PAGE_SIZE` rather than an unbounded response.
fn prints_query(game: &str, oracle_id: &str, exclude_id: i32) -> Select<card::Entity> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::OracleId.eq(oracle_id))
        .filter(card::Column::Id.ne(exclude_id))
        .order_by_with_nulls(card::Column::ReleasedAt, Order::Desc, NullOrdering::Last)
        .order_by_asc(card::Column::SetCode)
        .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last)
        .order_by_asc(card::Column::CollectorNumber)
        .order_by_asc(card::Column::Id)
        .limit(MAX_PAGE_SIZE)
}

/// Query the game's **distinct** card names whose name contains `term`
/// (case-insensitively, via SQLite's ASCII-case-insensitive `LIKE`), capped at
/// `limit`. Names that *start* with `term` are ordered first (a boolean
/// "starts-with" expression sorted descending — SQLite puts the `1`s before the
/// `0`s), then alphabetically. `term`'s LIKE metacharacters are escaped so they
/// match literally. Selects the `name` column only, so callers finish with
/// `.into_tuple::<String>()`. Powers the collection quick-add autocomplete.
fn name_suggestions_query(game: &str, term: &str, limit: u64) -> Select<card::Entity> {
    let escaped = escape_like(term);
    let name_col = Expr::col((card::Entity, card::Column::Name));
    let contains = name_col
        .clone()
        .like(LikeExpr::new(format!("%{escaped}%")).escape('\\'));
    let starts_with = name_col.like(LikeExpr::new(format!("{escaped}%")).escape('\\'));

    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(contains)
        .select_only()
        .column(card::Column::Name)
        .distinct()
        .order_by(starts_with, Order::Desc)
        .order_by_asc(card::Column::Name)
        .limit(limit)
}

/// Apply the optional `q` search filter to a card query. A blank/absent `q` leaves
/// the query unchanged; a malformed query surfaces as a 422 via
/// [`search_condition`](crate::handlers::shared::search_condition).
fn apply_search(
    query: Select<card::Entity>,
    game: &Game,
    params: &ListParams,
) -> Result<Select<card::Entity>, AppError> {
    match params.search() {
        Some(search) => Ok(query.filter(search_condition(game, search)?)),
        None => Ok(query),
    }
}
