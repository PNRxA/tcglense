//! Scryfall integration: the MTG card-data provider.
//!
//! This is the first (and currently only) game provider. Each future TCG gets
//! its own provider module; the generic catalog layer (`crate::catalog`) decides
//! which provider to invoke per game.

pub mod client;
pub mod drops;
mod dummy;
mod foil_variants;
pub mod ingest;
mod map;
pub mod model;
mod price_history;
mod progress;
/// Card rulings ("Notes and Rules Information", issue #522): Scryfall's `rulings` bulk
/// import into `card_rulings`, keyed by `oracle_id`.
pub mod rulings;
pub mod search;
/// DB persistence for the Secret Lair drop snapshot (reseed the store on boot from the last-good
/// scrape/import instead of the committed seed).
pub mod sld_persist;
/// Secret Lair drop gallery scrape (the mirror origin's daily "fetch from source").
pub mod sld_scrape;
/// Secret Lair drop snapshot import from the mirror (a consumer's daily pull).
pub mod sld_sync;
/// Background tasks refreshing the Secret Lair drop snapshot (scrape on the origin, import on
/// every other instance).
pub mod sld_tasks;
pub mod subtypes;

pub use dummy::seed;
/// Copy each foil-★ variant's foil price onto its nonfoil base card (issue #209), so a
/// consolidated foil holding values correctly; run every sync tick before the snapshot.
pub(crate) use foil_variants::enrich_foil_variant_prices;
pub use ingest::refresh;
/// Daily price-history capture, re-exported at the provider level so callers use
/// `scryfall::snapshot_prices` / `scryfall::format_date` without reaching into the
/// `price_history` submodule.
pub(crate) use price_history::{format_date, snapshot_prices};
/// Name of the import progress span; `main.rs` scopes the `IndicatifLayer` to it.
pub(crate) use progress::SPAN_NAME as PROGRESS_SPAN_NAME;

/// Game id this provider populates.
pub const GAME: &str = "mtg";
/// Human-readable game name, shown in the import progress bar.
pub(crate) const GAME_NAME: &str = "Magic: The Gathering";
/// Bulk dataset we ingest: one card object per English (or sole-language) print.
pub const DATASET: &str = "default_cards";
/// Bulk dataset of card rulings ("Notes and Rules Information", issue #522), keyed by
/// `oracle_id`. Tracked in its own `ingest_state` `(mtg, rulings)` row, so it version-gates
/// independently of the card import.
pub const DATASET_RULINGS: &str = "rulings";

/// Upstream bulk-data catalog. `pub` so the dataset-source seam ([`crate::datasets`])
/// and the mirror handler ([`crate::handlers::mirror`]) can resolve/re-serve it.
pub const BULK_DATA_URL: &str = "https://api.scryfall.com/bulk-data";
/// Upstream set list. `pub` for the same reason as [`BULK_DATA_URL`].
pub const SETS_URL: &str = "https://api.scryfall.com/sets";
