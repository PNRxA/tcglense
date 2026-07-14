//! The entity-agnostic half of the per-user holdings features (the collection and
//! the wish list): the wire DTOs, query params, and pure validation/aggregation
//! helpers `handlers::collection` and `handlers::wishlist` share. Both features hold
//! the same shape — `(user, game, card) -> { quantity, foil_quantity }`, no row at
//! both-zero — so everything here folds either entity's rows via [`HoldingCounts`];
//! only the SeaORM queries (which name a concrete entity) stay duplicated in the two
//! handler modules.
//!
//! The wire types keep their `Collection*` names on both features so the wish list
//! reuses the exact same ts-rs-generated TS types (the doc comments below are part of
//! that generated output, so they keep their collection wording verbatim).

use std::collections::{HashMap, HashSet};

use sea_orm::sea_query::{Expr, SimpleExpr};
use serde::{Deserialize, Serialize};

use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::{card, card_set, collection_item, deck_card, wishlist_item};
use crate::error::AppError;
use crate::state::AppState;

use crate::scryfall::drops::DropTable;

use super::dto::CardResponse;
use super::grouping::{group_into_drops, group_into_subtypes, paginate_buckets};
use super::lookup::load_group_set_codes;
use super::pagination::{
    DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE, MAX_PAGE_SIZE, Page,
    resolve_page, trim_query,
};
use super::sort::{SortDir, SortField};
use super::valuation::{Valuation, resolve_bulk_threshold_cents};

/// Cap on how many card ids one batch owned-counts lookup may request. A browse page
/// shows at most a few hundred cards, so this bounds the two `IN (...)` queries well
/// above any real page while staying under SQLite's bound-variable limit and refusing
/// an abusive request.
pub(crate) const MAX_OWNED_IDS: usize = 500;

/// The per-finish copy counts a holdings row carries, implemented by both holdings
/// entities (`collection_items` owns, `wishlist_items` wants) so the aggregation
/// helpers below can fold either entity's rows into the shared wire shapes.
pub(crate) trait HoldingCounts {
    /// Regular (non-foil) copies.
    fn quantity(&self) -> i32;
    /// Foil copies.
    fn foil_quantity(&self) -> i32;
}

impl HoldingCounts for collection_item::Model {
    fn quantity(&self) -> i32 {
        self.quantity
    }

    fn foil_quantity(&self) -> i32 {
        self.foil_quantity
    }
}

impl HoldingCounts for wishlist_item::Model {
    fn quantity(&self) -> i32 {
        self.quantity
    }

    fn foil_quantity(&self) -> i32 {
        self.foil_quantity
    }
}

impl HoldingCounts for deck_card::Model {
    fn quantity(&self) -> i32 {
        self.quantity
    }

    fn foil_quantity(&self) -> i32 {
        self.foil_quantity
    }
}

// ---------- Response / request DTOs ----------

/// One owned card: the full public card payload plus how many copies are owned.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionEntry {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// One Secret Lair drop with the signed-in user's owned cards in it — the collection
/// mirror of the catalog's `DropGroupResponse`, but each card carries its owned counts.
/// The enclosing [`Page`](crate::handlers::shared::Page) paginates over these (so `total`
/// is a drop count, not cards).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionDropGroup {
    /// Stable slug for anchors/links; `None` for the catch-all "Other" group of owned
    /// cards the snapshot doesn't place in a drop.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CollectionEntry>,
}

/// One set sub-type (card treatment) with the signed-in user's owned cards in it — the
/// collection/wish-list mirror of the catalog's `SubtypeGroupResponse`, each card carrying
/// its owned counts. Same shape as [`CollectionDropGroup`]; the enclosing
/// [`Page`](crate::handlers::shared::Page) paginates over these (`total` is a sub-type count).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSubtypeGroup {
    /// Stable slug (`normal`/`borderless`/`showcase`/…) for anchors/links.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CollectionEntry>,
}

/// Just the owned counts for one card — what the card-detail controls read and write.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionQuantities {
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Batch owned-counts response: external card id -> owned counts, for owned cards
/// only. Cards the user doesn't own are simply absent (never a zero entry), so a page
/// with nothing owned serialises to `{ "data": {} }`.
pub type OwnedCountsResponse = DataBody<HashMap<String, CollectionQuantities>>;

/// Aggregate stats for a user's per-game collection (the collection landing header).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSummary {
    /// Distinct cards owned (one per collection row).
    pub unique_cards: i64,
    /// Total copies owned (regular + foil) across every card.
    pub total_cards: i64,
    /// Estimated USD value: regular copies at the card's `usd`, foil copies at
    /// `usd_foil`, as a 2-dp decimal string. `null` when nothing owned is priced.
    pub total_value_usd: Option<String>,
    /// The "bulk" portion of the total: the value of just the finishes priced under the
    /// request's bulk threshold each (default $1 — the low-value commons/uncommons), a
    /// 2-dp decimal string. `"0.00"` when something is priced but none of it is bulk;
    /// `null` when nothing owned is priced.
    pub bulk_value_usd: Option<String>,
}

/// One set the user owns cards in, for the collection's per-set landing. Carries the
/// same catalog set metadata a set tile needs (so the SPA can reuse `SetTile`) plus how
/// much of it the user owns.
#[derive(Debug, Serialize, PartialEq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSet {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i32,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
    pub has_drops: bool,
    /// Whether the user's owned cards in this set include any special treatment, so the
    /// tile can offer the by-sub-type view (mirrors the catalog set's `has_subtypes`).
    pub has_subtypes: bool,
    /// Distinct cards owned in this set.
    pub owned_cards: i64,
    /// Total copies owned (regular + foil) in this set.
    pub owned_copies: i64,
    /// Estimated USD value of the owned cards in this set (regular copies at `usd`,
    /// foil at `usd_foil`), a 2-dp decimal string. `null` when nothing owned is priced —
    /// same semantics as the summary's `total_value_usd`, scoped to the one set.
    pub owned_value_usd: Option<String>,
    /// The "bulk" portion of `owned_value_usd`: the value of just the finishes priced
    /// under the request's bulk threshold each (default $1), a 2-dp decimal string.
    /// `"0.00"` when the set's owned cards are priced but none are bulk; `null` when
    /// nothing owned in the set is priced.
    pub owned_bulk_value_usd: Option<String>,
}

/// The sets a user owns cards in, newest set first.
pub type CollectionSetsResponse = DataBody<Vec<CollectionSet>>;

/// Body of `PUT .../cards/{id}`: the desired absolute counts (not a delta). Setting
/// both to zero removes the card from the collection.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetQuantitiesRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Body of `POST .../owned`: the external card ids to look up owned counts for. Sent
/// as a POST body rather than a GET query so a browse page's (potentially few-hundred)
/// id list can't blow the request-line length behind a proxy.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct OwnedCountsRequest {
    pub ids: Vec<String>,
}

// ---------- Query params ----------

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Optional search query — the same Scryfall-style syntax the public catalog
    /// card lists accept (parsed by [`crate::scryfall::search`]); a malformed query
    /// is a 422. Absent/blank means no filter.
    #[serde(default)]
    pub q: Option<String>,
    /// Sort key. `updated` (the default) orders by most-recently-changed; `quantity`
    /// orders by total copies held (regular + foil); every other key
    /// (`name`/`rarity`/`released`/`cmc`/`price`) reuses the catalog card sorts. An
    /// unknown value is a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`); absent = the sort key's natural direction. An
    /// unknown value is a 422.
    #[serde(default)]
    pub dir: Option<String>,
    /// Optional set-code scope: when present, only cards from that set are returned,
    /// ANDed with any `q`. Powers the per-set collection view. Absent/blank = every set.
    #[serde(default)]
    pub set: Option<String>,
    /// When `true` *and* a `set` scope is present, span the set's whole **group** (its
    /// top-level root plus every related sub-set) instead of just the one set — the
    /// collection mirror of the catalog's `include_related`. Ignored without a `set`.
    #[serde(default)]
    pub include_related: Option<bool>,
}

/// Query params for the (optionally set-scoped) collection summary.
#[derive(Debug, Deserialize)]
pub struct SummaryParams {
    /// Optional set-code scope — the summary is computed over just that set's owned
    /// cards. Absent/blank = the whole collection.
    #[serde(default)]
    pub set: Option<String>,
    /// When `true` *and* a `set` scope is present, span the set's whole **group** (root +
    /// related sub-sets) instead of just the one set — the collection mirror of the
    /// catalog's `include_related`, matching the list / ghost views. Ignored without a `set`.
    #[serde(default)]
    pub include_related: Option<bool>,
    /// Optional per-unit "bulk" price cutoff, in USD cents (a user display preference the
    /// SPA persists and sends, issue #289). Absent = the default $1; clamped server-side
    /// (see [`resolve_bulk_threshold_cents`]).
    #[serde(default)]
    pub bulk_max_cents: Option<i64>,
}

impl SummaryParams {
    /// The resolved bulk cutoff (in cents) to split the summary's bulk subtotal at.
    pub(crate) fn bulk_threshold_cents(&self) -> i128 {
        resolve_bulk_threshold_cents(self.bulk_max_cents)
    }
}

/// Query params for the per-set holdings landing (collection + wish list). Only the bulk
/// threshold: the per-set tiles carry the same bulk split as the summary header, so the
/// cutoff has to track the same user preference.
#[derive(Debug, Deserialize)]
pub struct SetsParams {
    /// Per-unit "bulk" price cutoff, in USD cents — the same preference as
    /// [`SummaryParams::bulk_max_cents`]. Absent = the default $1; clamped server-side.
    #[serde(default)]
    pub bulk_max_cents: Option<i64>,
}

impl SetsParams {
    /// The resolved bulk cutoff (in cents) to split each set tile's bulk subtotal at.
    pub(crate) fn bulk_threshold_cents(&self) -> i128 {
        resolve_bulk_threshold_cents(self.bulk_max_cents)
    }
}

/// How the collection list is ordered: either the collection-specific recency order
/// (the default) or one of the shared catalog card sorts, reused verbatim so the
/// collection grid can sort identically to the browse grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionSort {
    /// Most-recently added/updated first (by the holdings row's `updated_at`).
    Recent,
    /// Total copies held (regular + foil) first — a holdings-only sort with no catalog
    /// card equivalent (issue #228).
    Quantity,
    /// A card-column sort shared with the catalog card lists.
    Card(SortField),
}

/// SQL ordering expression for a holding's **total copies** (regular + foil) — the key
/// the `quantity` sort orders on. Both holdings tables (`collection_items`,
/// `wishlist_items`) name these columns identically and neither is a `cards` column, so
/// the bare names stay unambiguous under the list queries' card join (matching the other
/// `Expr::cust` sort expressions in [`super::sort`]). Both counts are `NOT NULL`, so the
/// sum is never NULL and needs no null handling.
pub(crate) fn copies_expr() -> SimpleExpr {
    Expr::cust("quantity + foil_quantity")
}

impl ListParams {
    /// Resolve the requested 1-based page and clamp the page size to `[1, MAX]`.
    pub(crate) fn page_and_size(&self) -> (u64, u64) {
        resolve_page(self.page, self.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE)
    }

    /// The trimmed search query, or `None` when it's absent or blank.
    pub(crate) fn search(&self) -> Option<&str> {
        trim_query(self.q.as_deref())
    }

    /// The trimmed set-code scope, or `None` when it's absent or blank.
    pub(crate) fn set(&self) -> Option<&str> {
        trim_query(self.set.as_deref())
    }

    /// Whether to span the scoped set's whole group (the include-related view). Only
    /// meaningful alongside a `set` scope; the handler ignores it otherwise.
    pub(crate) fn include_related(&self) -> bool {
        self.include_related.unwrap_or(false)
    }

    /// Resolve the requested 1-based page and clamp the page size for the by-drop
    /// view, which paginates over drops (not cards) and so has its own smaller bounds.
    pub(crate) fn drop_page_and_size(&self) -> (u64, u64) {
        resolve_page(
            self.page,
            self.page_size,
            DEFAULT_DROP_PAGE_SIZE,
            MAX_DROP_PAGE_SIZE,
        )
    }

    /// Resolve the `sort`/`dir` params into a validated `(sort, direction)`,
    /// defaulting to most-recently-updated. An unrecognised key/direction is a 422 —
    /// consistent with a malformed `q` — rather than being silently ignored.
    pub(crate) fn sort_spec(&self) -> Result<(CollectionSort, SortDir), AppError> {
        let (sort, default_dir) = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            // The holdings lists' natural default (and its explicit key) is recency.
            None | Some("updated" | "recent") => (CollectionSort::Recent, SortDir::Desc),
            // Total copies held (regular + foil), most first — a holdings-only sort, so
            // it's intercepted here before the catalog card-field fallback (which would
            // reject it, `quantity` being no card column).
            Some("quantity" | "copies") => (CollectionSort::Quantity, SortDir::Desc),
            Some(value) => {
                let field = SortField::parse(value)?;
                (CollectionSort::Card(field), field.default_dir())
            }
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => default_dir,
            Some(value) => SortDir::parse(value)?,
        };
        Ok((sort, dir))
    }
}

// ---------- Pure helpers ----------

/// Resolve the set-code scope for a holdings list: `None` (no scope, the whole
/// collection/wish list), a single-code slice (the per-set view), or — with
/// `include_related` — the scoped set's whole group (root + related sub-sets), resolved
/// through the shared [`load_group_set_codes`] seam the catalog set view also uses, so
/// both span identical sets. Only fetches the set list when a group actually needs
/// resolving.
pub(crate) async fn resolve_set_scope(
    state: &AppState,
    game: &str,
    set: Option<&str>,
    include_related: bool,
) -> Result<Option<Vec<String>>, AppError> {
    let Some(code) = set else { return Ok(None) };
    if !include_related {
        return Ok(Some(vec![code.to_string()]));
    }
    Ok(Some(load_group_set_codes(state, game, code).await?))
}

/// Trim, drop blanks, and de-duplicate a batch of requested external card ids,
/// preserving first-seen order (so the `IN (...)` bind list has no repeats and a
/// sloppy client list is tolerated).
pub(crate) fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

/// Validate one requested copy count: non-negative and at most
/// [`MAX_CARD_QUANTITY`], `422` otherwise. Shared by the collection and wish-list
/// `PUT` endpoints so both bound counts identically (and agree with the import
/// reconcile clamp, which uses the same constant).
pub(crate) fn validate_quantity(value: i32, field: &str) -> Result<i32, AppError> {
    if value < 0 {
        return Err(AppError::Validation(format!(
            "{field} must not be negative"
        )));
    }
    if value > MAX_CARD_QUANTITY {
        return Err(AppError::Validation(format!(
            "{field} must be at most {MAX_CARD_QUANTITY}"
        )));
    }
    Ok(value)
}

// ---------- Aggregation cores ----------

/// Fold already-fetched holdings rows (each left-joined to its card) into the
/// aggregate summary stats: distinct cards, total copies, and the estimated USD
/// value/bulk split. A row whose card is `None` (the card row vanished in a catalog
/// re-import) is skipped for **all** stats — matching the list reads. Prices are
/// aggregated in Rust, never trusting the stored decimal strings to SQL arithmetic.
/// Pure (the entity-specific query stays with each caller) so both features and their
/// tests share the one aggregation.
pub(crate) fn summarize_holdings<H: HoldingCounts>(
    rows: &[(H, Option<card::Model>)],
    bulk_threshold_cents: i128,
) -> CollectionSummary {
    let mut unique_cards: i64 = 0;
    let mut total_cards: i64 = 0;
    let mut valuation = Valuation::new(bulk_threshold_cents);
    for (item, card) in rows {
        let Some(card) = card else { continue };
        unique_cards += 1;
        total_cards += i64::from(item.quantity()) + i64::from(item.foil_quantity());
        valuation.add(
            card.price_usd.as_deref(),
            item.quantity(),
            card.price_usd_foil.as_deref(),
            item.foil_quantity(),
        );
    }

    CollectionSummary {
        unique_cards,
        total_cards,
        total_value_usd: valuation.total_usd(),
        bulk_value_usd: valuation.bulk_usd(),
    }
}

/// Per-set running totals while aggregating a user's holdings into set tiles.
#[derive(Default)]
struct SetAgg {
    /// The card's own `set_name`, used only if `card_sets` has no row for the set.
    fallback_name: String,
    /// Distinct owned cards (one per holding row).
    owned_cards: i64,
    /// Total owned copies (regular + foil).
    owned_copies: i64,
    /// Estimated USD value of the set's owned cards (regular at `usd`, foil at
    /// `usd_foil`); its `any_priced` flag reports `null` for an all-unpriced set
    /// rather than `$0.00`, matching the summary.
    valuation: Valuation,
    /// Whether any owned card in the set has a special treatment — so the tile can offer
    /// the by-sub-type view. Derived from the owned cards already in hand (no query).
    has_subtypes: bool,
}

/// Aggregate owned holdings into per-set tiles: count distinct owned cards + total
/// copies + estimated value per `set_code`, dress each with the game's set metadata
/// (falling back to the card's own `set_name` when the set row is missing), and order
/// newest set first (undated last), tie-broken by code for deterministic output. Pure so
/// it can be unit-tested without a DB. Holdings whose card row is gone are skipped.
pub(crate) fn build_collection_sets<H: HoldingCounts>(
    game: &str,
    rows: Vec<(H, Option<card::Model>)>,
    sets: Vec<card_set::Model>,
    bulk_threshold_cents: i128,
) -> Vec<CollectionSet> {
    let mut agg: HashMap<String, SetAgg> = HashMap::new();
    for (item, card) in rows {
        let Some(card) = card else { continue };
        // Classify the treatment before the card's fields move into the map entry.
        let is_special = crate::scryfall::subtypes::is_special(&card);
        // Read the card's prices before its set_code/set_name move into the map entry,
        // so the borrow is clean regardless of aggregation order.
        let usd = card.price_usd.as_deref();
        let usd_foil = card.price_usd_foil.as_deref();
        // Each set's running valuation splits its bulk subtotal at the request's chosen
        // cutoff (default $1), matching the summary header's figure.
        let entry = agg.entry(card.set_code).or_insert_with(|| SetAgg {
            fallback_name: card.set_name,
            valuation: Valuation::new(bulk_threshold_cents),
            ..SetAgg::default()
        });
        entry.has_subtypes |= is_special;
        entry.owned_cards += 1;
        entry.owned_copies += i64::from(item.quantity()) + i64::from(item.foil_quantity());
        entry
            .valuation
            .add(usd, item.quantity(), usd_foil, item.foil_quantity());
    }

    let meta: HashMap<String, card_set::Model> =
        sets.into_iter().map(|s| (s.code.clone(), s)).collect();

    let mut out: Vec<CollectionSet> = agg
        .into_iter()
        .map(|(code, agg)| {
            let SetAgg {
                fallback_name,
                owned_cards,
                owned_copies,
                valuation,
                has_subtypes,
            } = agg;
            // Dress the tile with the game's set metadata; a set present in a holding but
            // absent from card_sets (e.g. metadata not yet synced) degrades to a bare tile
            // using the card's own set name. The owned stats are identical either way, so
            // both cases build one `CollectionSet` (no duplicated arm).
            let m = meta.get(&code);
            CollectionSet {
                name: m.map_or(fallback_name, |m| m.name.clone()),
                set_type: m.and_then(|m| m.set_type.clone()),
                released_at: m.and_then(|m| m.released_at.clone()),
                card_count: m.map_or(0, |m| m.card_count),
                icon_svg_uri: m.and_then(|m| m.icon_svg_uri.clone()),
                parent_set_code: m.and_then(|m| m.parent_set_code.clone()),
                has_drops: crate::scryfall::drops::has_drops(game, &code),
                has_subtypes,
                owned_cards,
                owned_copies,
                owned_value_usd: valuation.total_usd(),
                owned_bulk_value_usd: valuation.bulk_usd(),
                code,
            }
        })
        .collect();

    // Newest release first; `None` (undated) sorts last since `None < Some`. Ties by
    // code for a stable, deterministic order.
    out.sort_by(|a, b| {
        b.released_at
            .cmp(&a.released_at)
            .then_with(|| a.code.cmp(&b.code))
    });
    out
}

/// Shape already-fetched holdings rows (each left-joined to its card) into a by-drop
/// page: group the owned/wanted cards into Secret Lair drops, then paginate over drops.
/// A holding whose card row is gone (a catalog re-import) left-joins to `None` — skip it,
/// exactly as the list/summary reads do. Entity-agnostic over [`HoldingCounts`] so the
/// collection and wish list share the identical post-fetch shaping; only the SeaORM query
/// stays with each caller.
pub(crate) fn holding_drop_page<H: HoldingCounts>(
    table: &'static DropTable,
    rows: Vec<(H, Option<card::Model>)>,
    page: u64,
    page_size: u64,
) -> Page<CollectionDropGroup> {
    let pairs: Vec<(H, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();

    let buckets = group_into_drops(table, pairs, |(_, card)| card.collector_number.as_str());

    paginate_buckets(buckets, page, page_size, |bucket| CollectionDropGroup {
        slug: bucket.slug,
        title: bucket.title,
        card_count: bucket.cards.len(),
        cards: bucket
            .cards
            .into_iter()
            .map(|(item, card)| CollectionEntry {
                card: CardResponse::from(card),
                quantity: item.quantity(),
                foil_quantity: item.foil_quantity(),
            })
            .collect(),
    })
}

/// Shape already-fetched holdings rows (each left-joined to its card) into a by-sub-type
/// page: group the owned/wanted cards by card treatment, then paginate over sub-types.
/// A holding whose card row is gone (a catalog re-import) left-joins to `None` — skip it,
/// exactly as the list/summary reads do. Entity-agnostic over [`HoldingCounts`] so the
/// collection and wish list share the identical post-fetch shaping; only the SeaORM query
/// stays with each caller.
pub(crate) fn holding_subtype_page<H: HoldingCounts>(
    rows: Vec<(H, Option<card::Model>)>,
    page: u64,
    page_size: u64,
) -> Page<CollectionSubtypeGroup> {
    let pairs: Vec<(H, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();

    let buckets = group_into_subtypes(pairs, |(_, card)| card);

    paginate_buckets(buckets, page, page_size, |bucket| CollectionSubtypeGroup {
        slug: bucket.slug,
        title: bucket.title,
        card_count: bucket.cards.len(),
        cards: bucket
            .cards
            .into_iter()
            .map(|(item, card)| CollectionEntry {
                card: CardResponse::from(card),
                quantity: item.quantity(),
                foil_quantity: item.foil_quantity(),
            })
            .collect(),
    })
}
