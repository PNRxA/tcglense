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
    sea_query::{Expr, Func, NullOrdering, SimpleExpr},
};
use serde::Deserialize;

use crate::catalog::Game;
use crate::catalog::images::ImageError;
use crate::db::Dialect;
use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::handlers::shared::{
    CardExportFormat, DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, MAX_DROP_PAGE_SIZE, MAX_PAGE_SIZE,
    SortDir, SortField, every_word_matches_with, resolve_page, search_condition, starts_with_rank,
    trim_query,
};
use crate::scryfall::search::{cust_vals, escape_like};

mod art_tags;
mod cards;
mod export;
mod image;
mod keywords;
mod prices;
mod products;
mod rulings;
mod scan;
mod sets;
mod status;

#[cfg(test)]
mod tests;

pub use art_tags::{card_art_tags, list_art_tags};
pub(crate) use cards::search_cards;
pub use cards::{card_names, card_prints, get_card, list_cards};
pub use export::{export_cards, export_set_cards};
pub use image::card_image;
pub use keywords::list_keywords;
pub use prices::card_prices;
pub(crate) use products::search_products;
pub use products::{
    card_sealed, get_product, list_products, product_card_sections, product_cards,
    product_containers, product_contents, product_facets, product_image, product_prices,
};
pub use rulings::card_rulings;
pub use scan::scan_cards;
pub use sets::{get_set, list_set_cards, list_set_drops, list_set_subtypes, list_sets, set_icon};
pub use status::{ingest_status, list_games};

// The `#[utoipa::path]`-generated route metadata structs, re-exported alongside the
// handlers they document so `crate::openapi::ApiDoc`'s `paths(...)` list can name them
// at `crate::handlers::catalog::__path_<fn>` (utoipa rewrites each handler path to its
// sibling `__path_` struct, which lives in the private submodule where the handler is
// defined). See `crate::openapi`.
pub use art_tags::{__path_card_art_tags, __path_list_art_tags};
pub use cards::{__path_card_names, __path_card_prints, __path_get_card, __path_list_cards};
pub use export::{__path_export_cards, __path_export_set_cards};
pub use keywords::__path_list_keywords;
pub use prices::__path_card_prices;
pub use products::{
    __path_card_sealed, __path_get_product, __path_list_products, __path_product_card_sections,
    __path_product_cards, __path_product_containers, __path_product_contents,
    __path_product_facets, __path_product_prices,
};
pub use rulings::__path_card_rulings;
pub use scan::__path_scan_cards;
pub use sets::{
    __path_get_set, __path_list_set_cards, __path_list_set_drops, __path_list_set_subtypes,
    __path_list_sets,
};
pub use status::{__path_ingest_status, __path_list_games};

/// Card art for a given id is immutable, so it is safe to cache aggressively.
const IMAGE_CACHE_CONTROL: &str = "public, max-age=2592000, immutable";

/// Map an image-cache error to the right HTTP response for the two image proxies
/// (card art and sealed-product images), logging at a level that matches the cause.
///
/// - [`ImageError::Unavailable`] — the provider says this asset has no image (a
///   definitive `4xx`, e.g. the TCGplayer CDN `403`ing a product with no art). A routine
///   **404** logged at debug: the frontend already falls back to a placeholder, and the
///   miss is negatively cached (see [`crate::catalog::images`]) so it isn't re-fetched or
///   re-logged on the next view — the fix for the 500-per-view log spam of issue #214.
/// - [`ImageError::Http`] — a transient upstream fetch failure (`5xx` / rate-limit /
///   network). A **502** at warn; not cached, so it's retried next request.
/// - [`ImageError::Io`] — our own cache disk write failed. A **500** at error.
///
/// `subject` (`"product"` / `"card"`) and `id` name the asset in the log line.
pub(super) fn image_error_response(err: ImageError, subject: &str, id: &str) -> AppError {
    match err {
        ImageError::Unavailable(status) => {
            tracing::debug!(subject = %subject, id = %id, status = %status, "image unavailable upstream");
            AppError::NotFound("no image available".to_string())
        }
        ImageError::Http(source) => {
            tracing::warn!(subject = %subject, id = %id, error = %source, "image upstream fetch failed");
            AppError::BadGateway("image temporarily unavailable".to_string())
        }
        ImageError::Io(source) => {
            tracing::error!(subject = %subject, id = %id, error = %source, "image cache io error");
            AppError::Internal(format!("image cache error: {source}"))
        }
    }
}

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
    /// By-drop endpoint only: narrow the *drops* to those whose curated Secret Lair
    /// title contains this text (case-insensitive), applied after grouping and before
    /// drop pagination. Powers the by-drop view's "filter drops by name" box; a
    /// blank/absent value is ignored. Orthogonal to `q`, which narrows the cards
    /// within each drop — the two filters compose.
    #[serde(default)]
    pub drop: Option<String>,
    /// Export endpoints only: which plain-text shape to produce (`text`, the
    /// default, or `names`). Ignored by every listing endpoint — they answer JSON.
    #[serde(default)]
    pub format: Option<String>,
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

    /// The trimmed drop-title filter for the by-drop view, or `None` when absent/blank.
    fn drop_title_filter(&self) -> Option<&str> {
        trim_query(self.drop.as_deref())
    }

    /// The export shape from `?format=`, or a 422 when it names one we don't emit.
    /// Only the export endpoints call this; a stray `?format=` elsewhere is inert.
    fn export_format(&self) -> Result<CardExportFormat, AppError> {
        CardExportFormat::parse(self.format.as_deref())
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
        let field = match self
            .sort
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
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

/// Query params for the suggestion-style reads — the card-name autocomplete and the
/// universal search (`handlers::search`): a text to match and a cap on the answer.
#[derive(Debug, Deserialize)]
pub struct NameSuggestParams {
    /// The text to match names against (case-insensitively). Absent/blank yields an empty
    /// result — there's nothing to suggest yet.
    #[serde(default)]
    pub q: Option<String>,
    /// How many suggestions to return, clamped to each endpoint's own `[1, max]`. Absent =
    /// that endpoint's default.
    #[serde(default)]
    pub limit: Option<u64>,
}

// ---------- Shared card helpers ----------

/// The base query **every public card listing** starts from: the game's cards, minus the
/// foil-★ variants folded onto their nonfoil base.
///
/// Some printings are two Scryfall objects — a nonfoil `1587` and a foil `1587★` sharing one
/// gameplay identity — and [`crate::scryfall::enrich_foil_variant_prices`] already copies the
/// star's foil price onto the base, so listing both puts two near-identical tiles in the grid
/// where one card exists (the "Hatsune Miku: Sakura Superstar" drop showed six such pairs).
/// The fold is the catalog-side twin of the one the collection has always done
/// (`collection_import::consolidate`), and it is deliberately a **presentation** fold: the star
/// row stays in `cards`, so its Scryfall id keeps resolving on the card detail route, in
/// holdings/deck/alert links, and in provider imports.
///
/// Which stars qualify is decided once per sync tick by
/// [`crate::scryfall::refresh_foil_variant_folds`] and persisted on `cards.folded_onto_id`, so
/// this is one indexed `IS NULL` test rather than a per-row re-derivation of the pairing rule.
/// It is **narrower than the pairing rule the price enrichment uses**: a star that differs from
/// its base in anything a visitor can see or search on — border colour, watermark, art, frame,
/// flavour text, a non-foil promo type — keeps its own tile, as do an orphan `…★` promo, an
/// etched star, and a star whose base is itself foilable.
///
/// Every card grid must build from here rather than a bare `Card::find()`, or that surface
/// starts showing the duplicate again: the all-cards search, a set's cards, the by-drop and
/// by-sub-type groupings, the `.txt` exports built from those same builders, and a card's
/// other printings. Deliberately **not** applied to:
///
/// - **Anything that resolves a card by id**, which is what keeps this a presentation fold —
///   [`crate::handlers::shared::load_card`] (card detail, prices, image, rulings, art tags,
///   holdings GET/PUT, deck-card writes, alert creation), the holdings listings' joins, and
///   the life counter's commander lookups. A star's Scryfall id is live in URLs, bookmarks,
///   API-key clients and existing rows, and must keep resolving.
/// - **Sealed-product contents** — a product's membership rows name the foil printing in its
///   own right, with its own `foil` flag, so folding there would lose real information.
/// - **The name autocomplete** — it returns distinct *names*, which a star never adds to.
/// - **The scanner's fingerprint index** — a photographed foil legitimately matches the star.
/// - **The sitemap** — the star's detail page still exists and is still worth indexing (and
///   its chunking is a keyset walk over `cards.id`, so filtering would renumber every chunk).
///
/// Two more halves live elsewhere: a drop whose snapshot entry names only the star still claims
/// its base ([`crate::scryfall::drops::DropTable::drop_for`]), and every published set
/// `card_count` is reduced by what the fold hid — the set list ([`sets::list_sets`]), one set's
/// own read ([`sets::get_set`]) and the collection/wish-list tiles
/// ([`crate::handlers::shared::build_collection_sets`]), all through
/// [`crate::scryfall::FoldedSetCounts`] — so neither the grouping nor any header disagrees with
/// the grid.
fn catalog_cards(game: &str) -> Select<card::Entity> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(crate::scryfall::not_folded_foil_variant())
}

/// Query a card's **other** printings: same game and `oracle_id`, excluding the
/// card itself (`exclude_id`). Ordered newest printing first (released date desc,
/// nulls last), then set code and collector number, with a stable `id` tiebreaker
/// so the order is deterministic. `oracle_id` is the gameplay-identity key shared
/// across all printings of a card.
///
/// Capped at `MAX_PAGE_SIZE` results: a handful of cards (e.g. basic lands) share
/// one `oracle_id` across hundreds-to-thousands of printings, and this is a "see
/// also" aid rather than an exhaustive listing, so it returns at most the newest
/// `MAX_PAGE_SIZE` rather than an unbounded response. Built on [`catalog_cards`], so a
/// folded foil-★ variant isn't offered as a separate printing of its own base.
fn prints_query(game: &str, oracle_id: &str, exclude_id: i32) -> Select<card::Entity> {
    catalog_cards(game)
        .filter(card::Column::OracleId.eq(oracle_id))
        .filter(card::Column::Id.ne(exclude_id))
        .order_by_with_nulls(card::Column::ReleasedAt, Order::Desc, NullOrdering::Last)
        .order_by_asc(card::Column::SetCode)
        .order_by_with_nulls(
            card::Column::CollectorNumberInt,
            Order::Asc,
            NullOrdering::Last,
        )
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
/// Portable across SQLite and Postgres: distinct names come from `GROUP BY name`
/// (Postgres rejects `ORDER BY <expr>` alongside `SELECT DISTINCT` when the expr
/// isn't in the select list); case-folding is LOWER-both (`to_ascii_lowercase`
/// matches SQLite's ASCII `LOWER()` → byte-identical results); and the
/// starts-with-first rank is `MAX(CASE … THEN 1 ELSE 0 END)` (an integer, so it
/// works on Postgres, which has no `max(boolean)`). All name-group rows share the
/// rank, so `MAX` equals the rank and the ordering matches the old DISTINCT form.
///
/// The LIKE sides compile to `LOWER(COALESCE(name, ''))` — the **exact** expression
/// `m..027`'s `idx_cards_name_trgm` trigram index is built on (with the `''` inline,
/// not bound, so the Postgres planner can match the expression index) — turning the
/// per-keystroke full scan of the wide `cards` table into a trgm bitmap scan for
/// terms of ≥ 3 chars (shorter needles deliberately keep the seq scan; see `m..027`).
/// `name` is `NOT NULL`, so the `COALESCE` is an identity and the SQLite result set
/// is byte-identical to the previous bare-`LOWER(name)` form.
fn name_suggestions_query(
    game: &str,
    term: &str,
    limit: u64,
    dialect: Dialect,
) -> Select<card::Entity> {
    let escaped = escape_like(term).to_ascii_lowercase();
    let name_like = |pattern: String| indexed_name_like(dialect, pattern);

    let contains = name_like(format!("%{escaped}%"));
    // 0/1 rank so MAX() is valid on Postgres (no max(boolean)).
    let starts_with_rank = Expr::case(name_like(format!("{escaped}%")), 1).finally(0);
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

/// `LOWER(COALESCE(name, '')) LIKE pattern ESCAPE '\'` — the **one** spelling of a card-name
/// `LIKE` that Postgres's `idx_cards_name_trgm` expression index (`m..027`) matches, so the
/// per-keystroke name reads ([`name_suggestions_query`] and the universal search's
/// [`card_name_search_query`]) become a trigram bitmap scan rather than a scan of the wide
/// `cards` table. The `''` is inline, not bound, for the same reason (the planner matches the
/// index expression textually); `name` is `NOT NULL`, so the `COALESCE` is an identity and
/// the SQLite result set is byte-identical to a bare `LOWER(name)`. `pattern` arrives
/// ready-made: `LIKE`-escaped, ASCII lower-cased, with its `%` wildcards in place.
fn indexed_name_like(dialect: Dialect, pattern: String) -> SimpleExpr {
    cust_vals(
        dialect,
        "LOWER(COALESCE(name, '')) LIKE ? ESCAPE '\\'",
        [pattern],
    )
}

/// The universal search's card leg (`handlers::search`): the game's cards whose name
/// contains every whitespace-separated word of `term`, **one row per distinct name**,
/// prefix matches first and then by name, capped at `limit`.
///
/// Three seams, deliberately none of them new: the name match is
/// [`every_word_matches_with`] — the same all-words rule the sealed-product and precon
/// listings answer with, so every leg of the universal search reads "commander tarkir"
/// identically — spelled through [`indexed_name_like`] so it rides the trigram index; the
/// one-per-name fold is [`fold_unique_by`], the engine behind the listing's `unique:cards`,
/// because a suggestion list filled with eight printings of the one card the visitor typed
/// hides every other card; and the ranking is [`starts_with_rank`], the autocomplete's own.
/// Built on [`catalog_cards`] like every card grid, so a folded foil-★ variant can't be the
/// printing that represents its name.
///
/// Folds by **name** rather than the listing's `oracle_id`: a name is what the visitor typed
/// and what a row shows, and — unlike an `oracle_id`, which a reversible printing lacks —
/// it is never NULL, so one card is always one row. Which printing represents the name is
/// the fold's pick (`MIN(id)` on Postgres, SQLite's group representative); the row's own
/// page lists the rest. `limit` is applied as given — a caller that wants a `has_more` asks
/// for one extra row.
pub(crate) fn card_name_search_query(
    game: &str,
    term: &str,
    limit: u64,
    dialect: Dialect,
) -> Result<Select<card::Entity>, AppError> {
    let matches = every_word_matches_with(term, |pattern| indexed_name_like(dialect, pattern))?;
    let query = fold_unique_by(catalog_cards(game).filter(matches), "name", dialect);
    Ok(query
        .order_by_asc(starts_with_rank((card::Entity, card::Column::Name), term))
        .order_by_asc(card::Column::Name)
        .order_by_asc(card::Column::Id)
        .limit(limit))
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
        _ => Ok((
            search_condition(game, search, dialect)?,
            SearchShape::default(),
        )),
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
    fold_unique_by(query, key_col, dialect)
}

/// The engine behind [`apply_unique`]: collapse `query` to one row per distinct value of
/// the `cards` column `key_col`, with the per-backend strategy the doc above describes.
/// `key_col` is one of a fixed set of column names spliced into SQL — never caller text.
/// Shared with the universal search's card leg ([`card_name_search_query`]), which folds
/// by `name`.
fn fold_unique_by(
    query: Select<card::Entity>,
    key_col: &str,
    dialect: Dialect,
) -> Select<card::Entity> {
    match dialect {
        // Unchanged from the pre-Postgres compiler — SQLite picks an arbitrary row
        // per group.
        Dialect::Sqlite => query.group_by(Expr::cust(format!(
            "COALESCE(cards.{key_col}, '#' || cards.id)"
        ))),
        Dialect::Postgres => {
            let group_key = Expr::cust(format!(
                "COALESCE(cards.{key_col}, '#' || CAST(cards.id AS TEXT))"
            ));
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
