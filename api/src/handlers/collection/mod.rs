//! Authenticated, per-user card-collection endpoints.
//!
//! A collection records how many copies of each card a signed-in user owns, per
//! game (`/api/collection/{game}/...`). Every route requires a valid access token
//! (via [`AuthUser`](crate::auth::extractor::AuthUser)) and is wired into the
//! router's `private` group, so responses are `Cache-Control: no-store` — per-user
//! data must never be shared-cached.
//!
//! Card ids in the path are the provider's **external** id (the same id the public
//! catalog exposes); each is resolved to the internal `cards.id` before storage,
//! so a holding survives a catalog re-import and the stored `card_id` matches
//! `card_price_history`. Ownership is always scoped by `user.id` from the token, so
//! one user can never read or mutate another's collection.
//!
//! The handlers are split across submodules by concern — [`read`] (list / summary /
//! owned-count reads), [`sets`] (per-set landing + by-drop), [`write`] (the owned-count
//! upsert), and [`import`] (external import/sync + saved-source CRUD) — with the shared
//! DTOs, params, and small helpers kept here.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::collection_import::jobs::{JobStatus, JobView};
use crate::collection_import::{ImportSummary, ProgressSnapshot, ReconcileMode};
use crate::entities::collection_item;
use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::prelude::CollectionItem;
use crate::error::AppError;
use crate::handlers::shared::{
    CardResponse, DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE,
    MAX_PAGE_SIZE, SortDir, SortField, resolve_page, trim_query,
};
use crate::state::AppState;

mod import;
mod read;
mod sets;
mod write;

#[cfg(test)]
mod tests;

pub use import::{
    delete_collection_source, get_collection_source, get_import_job, import_collection,
    import_collection_csv, save_collection_source, sync_collection_source,
};
pub use read::{collection_summary, get_collection_entry, list_collection, owned_counts};
pub use sets::{collection_set_drops, collection_sets};
pub use write::set_collection_entry;

/// Cap on how many card ids one batch owned-counts lookup may request. A browse page
/// shows at most a few hundred cards, so this bounds the two `IN (...)` queries well
/// above any real page while staying under SQLite's bound-variable limit and refusing
/// an abusive request.
const MAX_OWNED_IDS: usize = 500;
/// Hard ceiling on an uploaded collection CSV, enforced as a route body limit (see the
/// router). Sized generously above any real collection *when exported with only the three
/// columns we ask for* (Scryfall ID, Finish, Quantity ≈ 60 bytes/row, so ~16 MB spans far
/// more than [`collection_import`](crate::collection_import)'s row cap) while bounding the
/// memory a single upload can force us to buffer + parse. A larger, all-columns export can
/// exceed this — the UI tells the user to export only the three needed columns.
pub const MAX_CSV_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

// ---------- Response / request DTOs ----------

/// One owned card: the full public card payload plus how many copies are owned.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionDropGroup {
    /// Stable slug for anchors/links; `None` for the catch-all "Other" group of owned
    /// cards the snapshot doesn't place in a drop.
    pub slug: Option<String>,
    pub title: String,
    pub card_count: usize,
    pub cards: Vec<CollectionEntry>,
}

/// Just the owned counts for one card — what the card-detail controls read and write.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CollectionSummary {
    /// Distinct cards owned (one per collection row).
    pub unique_cards: i64,
    /// Total copies owned (regular + foil) across every card.
    pub total_cards: i64,
    /// Estimated USD value: regular copies at the card's `usd`, foil copies at
    /// `usd_foil`, as a 2-dp decimal string. `null` when nothing owned is priced.
    pub total_value_usd: Option<String>,
    /// The "bulk" portion of the total: the value of just the finishes priced under $1
    /// each (the low-value commons/uncommons), a 2-dp decimal string. `"0.00"` when
    /// something is priced but none of it is bulk; `null` when nothing owned is priced.
    pub bulk_value_usd: Option<String>,
}

/// One set the user owns cards in, for the collection's per-set landing. Carries the
/// same catalog set metadata a set tile needs (so the SPA can reuse `SetTile`) plus how
/// much of it the user owns.
#[derive(Debug, Serialize, PartialEq)]
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
    /// Distinct cards owned in this set.
    pub owned_cards: i64,
    /// Total copies owned (regular + foil) in this set.
    pub owned_copies: i64,
    /// Estimated USD value of the owned cards in this set (regular copies at `usd`,
    /// foil at `usd_foil`), a 2-dp decimal string. `null` when nothing owned is priced —
    /// same semantics as the summary's `total_value_usd`, scoped to the one set.
    pub owned_value_usd: Option<String>,
    /// The "bulk" portion of `owned_value_usd`: the value of just the finishes priced
    /// under $1 each, a 2-dp decimal string. `"0.00"` when the set's owned cards are
    /// priced but none are bulk; `null` when nothing owned in the set is priced.
    pub owned_bulk_value_usd: Option<String>,
}

/// The sets a user owns cards in, newest set first.
pub type CollectionSetsResponse = DataBody<Vec<CollectionSet>>;

/// Body of `PUT .../cards/{id}`: the desired absolute counts (not a delta). Setting
/// both to zero removes the card from the collection.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetQuantitiesRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Optional search query — the same Scryfall-style syntax the public catalog
    /// card lists accept (parsed by [`crate::scryfall::search`]); a malformed query
    /// is a 422. Absent/blank means no filter.
    #[serde(default)]
    pub q: Option<String>,
    /// Sort key. `updated` (the default) orders by most-recently-changed; every other
    /// key (`name`/`rarity`/`released`/`cmc`/`price`) reuses the catalog card sorts.
    /// An unknown value is a 422.
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
}

/// How the collection list is ordered: either the collection-specific recency order
/// (the default) or one of the shared catalog card sorts, reused verbatim so the
/// collection grid can sort identically to the browse grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionSort {
    /// Most-recently added/updated first (by `collection_items.updated_at`).
    Recent,
    /// A card-column sort shared with the catalog card lists.
    Card(SortField),
}

/// Body of `POST .../owned`: the external card ids to look up owned counts for. Sent
/// as a POST body rather than a GET query so a browse page's (potentially few-hundred)
/// id list can't blow the request-line length behind a proxy.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct OwnedCountsRequest {
    pub ids: Vec<String>,
}

impl ListParams {
    /// Resolve the requested 1-based page and clamp the page size to `[1, MAX]`.
    fn page_and_size(&self) -> (u64, u64) {
        resolve_page(self.page, self.page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE)
    }

    /// The trimmed search query, or `None` when it's absent or blank.
    fn search(&self) -> Option<&str> {
        trim_query(self.q.as_deref())
    }

    /// The trimmed set-code scope, or `None` when it's absent or blank.
    fn set(&self) -> Option<&str> {
        trim_query(self.set.as_deref())
    }

    /// Whether to span the scoped set's whole group (the include-related view). Only
    /// meaningful alongside a `set` scope; the handler ignores it otherwise.
    fn include_related(&self) -> bool {
        self.include_related.unwrap_or(false)
    }

    /// Resolve the requested 1-based page and clamp the page size for the by-drop
    /// view, which paginates over drops (not cards) and so has its own smaller bounds.
    fn drop_page_and_size(&self) -> (u64, u64) {
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
    fn sort_spec(&self) -> Result<(CollectionSort, SortDir), AppError> {
        let (sort, default_dir) = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            // The collection's natural default (and its explicit key) is recency.
            None | Some("updated" | "recent") => (CollectionSort::Recent, SortDir::Desc),
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

// ---------- Import / sync request + response DTOs ----------

/// Body of `POST .../import`: which provider, the source URL/id, and how to reconcile.
/// `provider` is any string on the wire (validated against the known providers by the
/// handler), so the generated TS type is wider than the client's own body type.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ImportRequest {
    pub provider: String,
    pub source: String,
    pub mode: ReconcileMode,
}

/// Body of `PUT .../source`: the collection link to remember (provider + source URL/id),
/// plus whether saved re-syncs should use smart (incremental) sync. `smart` defaults to
/// `false` (full mirror) when omitted.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SaveSourceRequest {
    pub provider: String,
    pub source: String,
    #[serde(default)]
    pub smart: bool,
}

/// A saved external collection link for a game.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CollectionSource"))]
pub struct CollectionSourceResponse {
    pub provider: &'static str,
    pub external_id: String,
    /// A canonical, user-facing URL for the collection on the provider.
    pub url: String,
    /// RFC3339 timestamp of the last successful sync, or null if never synced.
    pub last_synced_at: Option<String>,
    /// Whether a saved re-sync uses smart (incremental) sync rather than a full mirror.
    pub smart: bool,
}

/// Live fetch progress for a running import: how many provider rows we've fetched so far,
/// and the collection's total when the provider reported one up front. A determinate
/// progress bar can be drawn when `total` is present; otherwise (a smart sync, which stops
/// early) only the running `fetched` count is meaningful.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ImportProgress {
    /// Provider rows fetched so far.
    pub fetched: u32,
    /// Total rows to fetch, when known; `null` for a smart sync (no meaningful total).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub total: Option<u32>,
}

impl From<ProgressSnapshot> for ImportProgress {
    fn from(s: ProgressSnapshot) -> Self {
        Self {
            fetched: s.fetched_rows,
            total: s.total_rows,
        }
    }
}

/// The status of a background import/sync job — returned when one is enqueued and each
/// time the client polls. Imports run asynchronously (throttled by the provider rate
/// limit), so the client kicks one off and polls this until `complete`/`error`.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ImportJob"))]
pub struct ImportJobResponse {
    pub job_id: u64,
    /// `queued` | `running` | `complete` | `error`.
    // The `ts(type)` override preserves the literal union (the strings come from
    // `from_status` below, which the TS derive can't see through).
    #[cfg_attr(test, ts(type = "\"queued\" | \"running\" | \"complete\" | \"error\""))]
    pub status: &'static str,
    /// Live fetch progress, present only while `status == "running"` (the fetch is
    /// underway); absent once queued/complete/error.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub progress: Option<ImportProgress>,
    /// The import summary, present only when `status == "complete"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub summary: Option<ImportSummary>,
    /// A user-facing message, present only when `status == "error"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

impl ImportJobResponse {
    /// Shape a lifecycle status alone (no progress) — used for the immediate `202` when a
    /// job is enqueued (always `Queued`, nothing fetched yet).
    fn from_status(job_id: u64, status: JobStatus) -> Self {
        let (status, summary, error) = match status {
            JobStatus::Queued => ("queued", None, None),
            JobStatus::Running => ("running", None, None),
            JobStatus::Complete(summary) => ("complete", Some(summary), None),
            JobStatus::Failed(message) => ("error", None, Some(message)),
        };
        Self {
            job_id,
            status,
            progress: None,
            summary,
            error,
        }
    }

    /// Shape a polled job view: the lifecycle status plus, while the fetch is running, its
    /// live progress (so the client can render a progress bar).
    fn from_view(job_id: u64, view: JobView) -> Self {
        let running = matches!(view.status, JobStatus::Running);
        let mut resp = Self::from_status(job_id, view.status);
        if running {
            resp.progress = Some(ImportProgress::from(view.progress));
        }
        resp
    }
}

/// Query params for a CSV upload. The reconcile `mode` rides in the query string so the
/// request body can be the raw CSV file. Parsed as an optional string (not a typed
/// `ReconcileMode`) so a missing/invalid mode is our JSON `422`, not axum's default
/// text/plain query-rejection.
#[derive(Debug, Deserialize)]
pub struct CsvImportParams {
    pub mode: Option<String>,
}

// ---------- Shared helpers ----------

/// The user's collection row for a card, if any. Shared by the get/set entry handlers.
async fn find_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    card_id: i32,
) -> Result<Option<collection_item::Model>, AppError> {
    Ok(CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(collection_item::Column::CardId.eq(card_id))
        .one(&state.db)
        .await?)
}
