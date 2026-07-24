//! Authenticated, per-user card-collection endpoints.
//!
//! A collection records how many copies of each card and sealed product a signed-in user owns, per
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
//! upsert), and [`import`] (external import/sync + saved-source CRUD) — with the
//! import-specific DTOs kept here. The entity-agnostic wire DTOs, params, and helpers
//! live in [`crate::handlers::shared::holdings`], shared with the wish list (its
//! same-shaped "want" twin), and are re-exported below so the submodules and their
//! tests keep addressing them through this module.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::collection_import::jobs::{JobStatus, JobView};
use crate::collection_import::{ImportSummary, ProgressSnapshot, ReconcileMode};
use crate::entities::collection_item;
use crate::entities::prelude::CollectionItem;
use crate::error::AppError;
use crate::state::AppState;

pub(crate) mod export;
mod import;
mod price_movements;
mod products;
mod read;
mod sets;
mod value_history;
mod write;

#[cfg(test)]
mod tests;

pub use export::export_collection;
pub use import::{
    delete_collection_source, get_collection_source, get_import_job, import_collection,
    import_collection_csv, import_collection_text, save_collection_source, sync_collection_source,
};
pub use price_movements::collection_movers;
pub use products::{
    collection_product_counts, collection_product_summary, get_collection_product_entry,
    list_collection_product_sets, list_collection_products, set_collection_product_entry,
};
pub use read::{collection_summary, get_collection_entry, list_collection, owned_counts};
pub use sets::{collection_set_drops, collection_set_subtypes, collection_sets};
pub use value_history::collection_value_history;
pub use write::set_collection_entry;

// The `user_id`-parameterised read cores, reused by the public sharing handlers
// (`crate::handlers::sharing::public`) so a public collection read shares the exact
// query/shaping logic — only how `user_id` is resolved differs.
pub(crate) use products::{owned_product_sets, owned_product_summary, owned_products_page};
pub(crate) use read::{owned_counts_map, owned_list_page, summary};
pub(crate) use sets::{owned_drop_page, owned_sets, owned_subtype_page};

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::collection::__path_<fn>`
// (see the note in `crate::handlers::catalog`).
pub use export::__path_export_collection;
pub use import::{
    __path_delete_collection_source, __path_get_collection_source, __path_get_import_job,
    __path_import_collection, __path_import_collection_csv, __path_import_collection_text,
    __path_save_collection_source, __path_sync_collection_source,
};
pub use price_movements::__path_collection_movers;
pub use products::{
    __path_collection_product_counts, __path_collection_product_summary,
    __path_get_collection_product_entry, __path_list_collection_product_sets,
    __path_list_collection_products, __path_set_collection_product_entry,
};
pub use read::{
    __path_collection_summary, __path_get_collection_entry, __path_list_collection,
    __path_owned_counts,
};
pub use sets::{
    __path_collection_set_drops, __path_collection_set_subtypes, __path_collection_sets,
};
pub use value_history::__path_collection_value_history;
pub use write::__path_set_collection_entry;

// The entity-agnostic DTOs, params, and constants (shared with `handlers::wishlist`).
pub(crate) use crate::handlers::shared::{
    CollectionDropGroup, CollectionEntry, CollectionQuantities, CollectionSetsResponse,
    CollectionSort, CollectionSubtypeGroup, CollectionSummary, ListParams, MAX_OWNED_IDS,
    OwnedCountsRequest, OwnedCountsResponse, SetQuantitiesRequest, SummaryParams,
};

/// Hard ceiling on an uploaded collection CSV, enforced as a route body limit (see the
/// router). Sized generously above any real collection *when exported with only the three
/// columns we ask for* (Scryfall ID, Finish, Quantity ≈ 60 bytes/row, so ~16 MB spans far
/// more than [`collection_import`](crate::collection_import)'s row cap) while bounding the
/// memory a single upload can force us to buffer + parse. A larger, all-columns export can
/// exceed this — the UI tells the user to export only the three needed columns.
pub const MAX_CSV_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

// ---------- Import / sync request + response DTOs ----------

/// Body of `POST .../import`: which provider, the source URL/id, and how to reconcile.
/// `provider` is any string on the wire (validated against the known providers by the
/// handler), so the generated TS type is wider than the client's own body type.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ImportRequest {
    pub provider: String,
    pub source: String,
    pub mode: ReconcileMode,
}

/// Body of `PUT .../source`: the collection link to remember (provider + source URL/id),
/// plus whether saved re-syncs should use smart (incremental) sync. `smart` defaults to
/// `false` (full mirror) when omitted.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SaveSourceRequest {
    pub provider: String,
    pub source: String,
    #[serde(default)]
    pub smart: bool,
}

/// A saved external collection link for a game.
#[derive(Debug, Serialize, utoipa::ToSchema)]
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
#[derive(Debug, Serialize, utoipa::ToSchema)]
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
#[derive(Debug, Serialize, utoipa::ToSchema)]
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
