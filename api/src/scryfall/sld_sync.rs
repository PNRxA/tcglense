//! Distributing the Secret Lair drop snapshot from the mirror origin to every other instance.
//!
//! The mirror origin scrapes Scryfall's gallery daily ([`super::sld_scrape`]) and serves the
//! resulting snapshot at `GET /api/mirror/scryfall/sld-drops` (see [`crate::handlers::mirror`]).
//! Every other instance **imports** that snapshot from the mirror daily ([`super::sld_tasks`])
//! rather than scraping Scryfall itself — the same posture as the dataset mirror and the
//! fingerprint index: a self-host contacts one origin, not the upstream provider. This module is
//! the consumer side: fetch the snapshot (conditional on the last `ETag`, so an unchanged snapshot
//! is a cheap `304`), then hand it to [`super::drops::install_snapshot`], which validates it before
//! swapping the store — a malformed / Secret-Lair-less payload is rejected, never installed.

use reqwest::{
    StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};

use crate::scryfall::drops::{self, SnapshotError};

/// Path the SLD-drops snapshot is served under on the mirror + the import path targets. Must match
/// the literal route registered in [`crate::router`].
pub const MIRROR_PREFIX: &str = "/api/mirror/scryfall/sld-drops";

/// A failure importing the snapshot from the mirror. Non-fatal at the call site (logged; the
/// instance keeps serving whatever drop snapshot it already had loaded).
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("sld-drops mirror request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("sld-drops payload was rejected: {0}")]
    Snapshot(#[from] SnapshotError),
}

/// The result of one import attempt.
pub enum ImportOutcome {
    /// The mirror's snapshot is unchanged since our last import (a `304`, or the served `ETag`
    /// still matches ours) — nothing was installed.
    Unchanged,
    /// `count` drops installed; carry the served `ETag` to condition the next fetch on.
    Imported { count: usize, etag: Option<String> },
}

/// Pull the current SLD drop snapshot from the mirror at `mirror_base` and, when it differs from
/// `prev_etag`, install it into the drop store. Conditional on `prev_etag` (an `If-None-Match`), so
/// an unchanged snapshot costs a single cheap `304`. The install validates the payload before
/// swapping the store, so a malformed / Secret-Lair-less snapshot is rejected and the current
/// table stands.
pub async fn import_from_mirror(
    http: &reqwest::Client,
    mirror_base: &str,
    prev_etag: Option<&str>,
) -> Result<ImportOutcome, ImportError> {
    let url = format!("{}{MIRROR_PREFIX}", mirror_base.trim_end_matches('/'));
    let mut request = http.get(&url);
    if let Some(tag) = prev_etag {
        request = request.header(IF_NONE_MATCH, tag);
    }
    let response = request.send().await?;
    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(ImportOutcome::Unchanged);
    }
    let response = response.error_for_status()?;
    let served_etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    // Belt-and-braces: some caches answer a conditional GET with a full 200 that still carries the
    // same ETag — treat that as unchanged too, so we never re-install needlessly.
    if let (Some(prev), Some(served)) = (prev_etag, served_etag.as_deref()) {
        if prev == served {
            return Ok(ImportOutcome::Unchanged);
        }
    }
    let body = response.text().await?;
    let count = drops::install_snapshot(&body)?;
    Ok(ImportOutcome::Imported {
        count,
        etag: served_etag,
    })
}
