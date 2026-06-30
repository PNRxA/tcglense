//! Scryfall integration: the MTG card-data provider.
//!
//! This is the first (and currently only) game provider. Each future TCG gets
//! its own provider module; the generic catalog layer (`crate::catalog`) decides
//! which provider to invoke per game.

pub mod client;
pub mod drops;
mod dummy;
pub mod ingest;
pub mod model;
mod progress;
pub mod search;

pub use dummy::seed;
pub use ingest::refresh;
/// Name of the import progress span; `main.rs` scopes the `IndicatifLayer` to it.
pub(crate) use progress::SPAN_NAME as PROGRESS_SPAN_NAME;

/// Game id this provider populates.
pub const GAME: &str = "mtg";
/// Human-readable game name, shown in the import progress bar.
pub(crate) const GAME_NAME: &str = "Magic: The Gathering";
/// Bulk dataset we ingest: one card object per English (or sole-language) print.
pub const DATASET: &str = "default_cards";

const BULK_DATA_URL: &str = "https://api.scryfall.com/bulk-data";
const SETS_URL: &str = "https://api.scryfall.com/sets";
