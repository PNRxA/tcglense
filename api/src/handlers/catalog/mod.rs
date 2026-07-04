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
    ColumnTrait, Condition, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    Select,
    sea_query::{Expr, Func, LikeExpr, NullOrdering, SimpleExpr},
};
use serde::Deserialize;

use crate::catalog::Game;
use crate::db::Dialect;
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
mod pricing;
mod products;
mod sets;
mod status;

#[cfg(test)]
mod tests;

pub use cards::{card_names, card_prints, get_card, list_cards};
pub use image::card_image;
pub use prices::card_prices;
pub use products::{
    get_product, list_products, product_facets, product_image, product_prices,
};
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

    /// Resolve the `(field, direction)` sort from the URL `sort`/`dir` params, an
    /// in-query `order:`/`direction:` directive, and the endpoint default, in that
    /// precedence order (URL param > in-query directive > default). An unrecognised
    /// value is a 422, consistent with a malformed `q` rather than silently ignored.
    fn sort_spec_with(
        &self,
        default: SortField,
        q_order: Option<SortField>,
        q_dir: Option<SortDir>,
    ) -> Result<(SortField, SortDir), AppError> {
        let field = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(value) => SortField::parse(value)?,
            None => q_order.unwrap_or(default),
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(value) => SortDir::parse(value)?,
            None => q_dir.unwrap_or(field.default_dir()),
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
/// (case-insensitively), capped at `limit`. Names that *start* with `term` are
/// ordered first, then alphabetically. `term`'s LIKE metacharacters are escaped so
/// they match literally. Selects the `name` column only, so callers finish with
/// `.into_tuple::<String>()`. Powers the collection quick-add autocomplete.
///
/// Portable across SQLite and Postgres without a dialect param: distinct names come
/// from `GROUP BY name` (Postgres rejects `ORDER BY <expr>` alongside `SELECT
/// DISTINCT` when the expr isn't in the select list); case-folding is LOWER-both
/// (`to_ascii_lowercase` matches SQLite's ASCII `LOWER()` → byte-identical results);
/// and the starts-with-first rank is `MAX(CASE … THEN 1 ELSE 0 END)` (an integer, so
/// it works on Postgres, which has no `max(boolean)`). All name-group rows share the
/// rank, so `MAX` equals the rank and the ordering matches the old DISTINCT form.
fn name_suggestions_query(game: &str, term: &str, limit: u64) -> Select<card::Entity> {
    let escaped = escape_like(term).to_ascii_lowercase();
    let name_lower = Expr::expr(Func::lower(Expr::col((card::Entity, card::Column::Name))));

    let contains = name_lower
        .clone()
        .like(LikeExpr::new(format!("%{escaped}%")).escape('\\'));
    // 0/1 rank so MAX() is valid on Postgres (no max(boolean)).
    let starts_with_rank = Expr::case(
        name_lower.like(LikeExpr::new(format!("{escaped}%")).escape('\\')),
        1,
    )
    .finally(0);
    let starts_with_rank = SimpleExpr::from(Func::max(starts_with_rank));

    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(contains)
        .select_only()
        .column(card::Column::Name)
        .group_by(card::Column::Name)
        .order_by(starts_with_rank, Order::Desc)
        .order_by_asc(card::Column::Name)
        .limit(limit)
}

/// The result-shaping directives a `q` may carry (`order:`/`direction:`/`unique:`),
/// resolved into the catalog's own sort/unique types.
#[derive(Default)]
struct SearchShape {
    order: Option<SortField>,
    direction: Option<SortDir>,
    unique: Option<crate::scryfall::search::UniqueMode>,
}

/// Apply the optional `q` search filter and return the filtered query plus any
/// result-shaping directives the query carried. A blank/absent `q` leaves the query
/// unchanged; a malformed query surfaces as a 422.
fn apply_search(
    query: Select<card::Entity>,
    game: &Game,
    params: &ListParams,
    dialect: Dialect,
) -> Result<(Select<card::Entity>, SearchShape), AppError> {
    match params.search() {
        Some(search) => {
            let (condition, shape) = parse_search(game, search, dialect)?;
            Ok((query.filter(condition), shape))
        }
        None => Ok((query, SearchShape::default())),
    }
}

/// Parse an MTG `q` into its row condition plus result-shaping directives; other
/// games fall back to a plain name substring with no directives.
fn parse_search(
    game: &Game,
    search: &str,
    dialect: Dialect,
) -> Result<(Condition, SearchShape), AppError> {
    match game.id {
        crate::scryfall::GAME => {
            let q = crate::scryfall::search::parse_query(search, dialect)?;
            Ok((
                q.condition,
                SearchShape {
                    order: q.order.map(SortField::from),
                    direction: q.direction.map(SortDir::from),
                    unique: q.unique,
                },
            ))
        }
        _ => Ok((search_condition(game, search, dialect)?, SearchShape::default())),
    }
}

/// Apply a `unique:` de-duplication mode by collapsing to one row per de-dup key
/// (`'#'||id` keeps NULL-key rows distinct so they don't collapse together).
/// `prints`/absent leaves the per-printing rows untouched.
///
/// Per-backend: SQLite keeps its exact `GROUP BY` (an arbitrary representative row
/// per group — its historical, unpinned behaviour, preserved byte-for-byte).
/// Postgres — which rejects a bare `GROUP BY` over `SELECT *` — instead filters to
/// each group's `MIN(id)` member via an `IN`-subquery. The subquery is built by
/// cloning the fully-filtered query (it already carries every WHERE filter: game,
/// search, exact-name, set-scope, include-related), so no group whose min-id row
/// fails a filter can wrongly vanish. `apply_unique` runs *before* sort/pagination,
/// so the clone captures only the row filters. Pagination `COUNT(*)` wraps the outer
/// query on both arms, yielding the group count.
fn apply_unique(
    query: Select<card::Entity>,
    unique: Option<crate::scryfall::search::UniqueMode>,
    dialect: Dialect,
) -> Select<card::Entity> {
    use crate::scryfall::search::UniqueMode;
    let key_col = match unique {
        Some(UniqueMode::Cards) => "oracle_id",
        Some(UniqueMode::Art) => "illustration_id",
        // prints / absent: no de-duplication.
        _ => return query,
    };
    match dialect {
        // Unchanged from the pre-Postgres compiler — SQLite picks an arbitrary row
        // per group.
        Dialect::Sqlite => {
            query.group_by(Expr::cust(format!("COALESCE(cards.{key_col}, '#' || cards.id)")))
        }
        Dialect::Postgres => {
            let group_key =
                Expr::cust(format!("COALESCE(cards.{key_col}, '#' || CAST(cards.id AS TEXT))"));
            let min_ids = query
                .clone()
                .select_only()
                .expr(Func::min(Expr::col((card::Entity, card::Column::Id))))
                .group_by(group_key)
                .into_query();
            query.filter(Expr::col((card::Entity, card::Column::Id)).in_subquery(min_ids))
        }
    }
}
