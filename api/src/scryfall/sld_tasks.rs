//! Background tasks keeping the Secret Lair drop snapshot fresh.
//!
//! Secret Lair drop titles aren't in the bulk card API, so they're scraped from Scryfall's
//! gallery. The **mirror origin** (`MIRROR_ENABLED`) re-scrapes daily ([`super::sld_scrape`]) and
//! installs the fresh snapshot, so its `/api/mirror/scryfall/sld-drops` endpoint serves current
//! titles; **every other instance** imports that snapshot from the mirror on the same interval
//! ([`super::sld_sync`]) rather than scraping Scryfall itself. Both fall back to the committed
//! `sld_drops.json` until their first successful fetch, and every failure is logged, never fatal.
//!
//! Split out of `tasks.rs` so the Secret Lair machinery lives beside its data ops under
//! `scryfall/`; the generic maintenance/sync orchestration stays in `tasks.rs`.

use std::time::Duration;

use reqwest::Client;

use crate::scryfall::{drops, sld_scrape, sld_sync};

/// Spawn the mirror origin's daily Secret Lair scrape: fetch Scryfall's gallery and install the
/// fresh snapshot into the drop store. A broken scrape (markup change → no drops) or an invalid
/// snapshot is rejected by [`drops::install_snapshot`], so the store keeps its last-good table.
/// The first tick runs at startup, then every `interval_hours` (a single pass when it's `0`).
pub(crate) fn spawn_sld_scrape(http: Client, user_agent: String, interval_hours: u64) {
    tokio::spawn(async move {
        loop {
            match sld_scrape::fetch_snapshot_json(&http, &user_agent).await {
                Ok(json) => match drops::install_snapshot(&json) {
                    Ok(count) => {
                        tracing::info!(count, "refreshed Secret Lair drop snapshot from Scryfall")
                    }
                    Err(err) => tracing::warn!(
                        error = %err,
                        "scraped Secret Lair snapshot rejected; keeping the current drop table"
                    ),
                },
                Err(err) => tracing::warn!(
                    error = %err,
                    "Secret Lair gallery scrape failed; keeping the current drop table"
                ),
            }
            if interval_hours == 0 {
                break; // startup-only posture: one scrape, no periodic refresh
            }
            tokio::time::sleep(Duration::from_secs(interval_hours.saturating_mul(60 * 60))).await;
        }
    });
}

/// Spawn a consumer's daily Secret Lair drop import: pull the snapshot from the mirror
/// (conditional on the last `ETag`, so an unchanged snapshot is a cheap `304`) and install it. The
/// `ETag` is held across iterations in memory — a restart simply re-fetches once. The first tick
/// runs at startup, then every `interval_hours` (a single import when it's `0`). Every failure is
/// logged, never fatal — the instance keeps serving whatever snapshot it already had loaded.
pub(crate) fn spawn_sld_import(http: Client, mirror_base: String, interval_hours: u64) {
    use crate::scryfall::sld_sync::ImportOutcome;
    tokio::spawn(async move {
        let mut prev_etag: Option<String> = None;
        loop {
            match sld_sync::import_from_mirror(&http, &mirror_base, prev_etag.as_deref()).await {
                Ok(ImportOutcome::Imported { count, etag }) => {
                    tracing::info!(count, "imported Secret Lair drop snapshot from the mirror");
                    prev_etag = etag;
                }
                Ok(ImportOutcome::Unchanged) => {
                    tracing::debug!("Secret Lair drop snapshot unchanged on the mirror")
                }
                Err(err) => tracing::warn!(
                    error = %err,
                    "Secret Lair drop import from the mirror failed"
                ),
            }
            if interval_hours == 0 {
                break; // startup-only posture: one import, no periodic re-check
            }
            tokio::time::sleep(Duration::from_secs(interval_hours.saturating_mul(60 * 60))).await;
        }
    });
}
