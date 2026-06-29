//! Scryfall integration: the MTG card-data provider.
//!
//! This is the first (and currently only) game provider. Each future TCG gets
//! its own provider module; the generic catalog layer (`crate::catalog`) decides
//! which provider to invoke per game.

pub mod client;
mod dummy;
pub mod ingest;
pub mod model;

pub use dummy::seed;
pub use ingest::refresh;

/// Game id this provider populates.
pub const GAME: &str = "mtg";
/// Bulk dataset we ingest: one card object per English (or sole-language) print.
pub const DATASET: &str = "default_cards";

const BULK_DATA_URL: &str = "https://api.scryfall.com/bulk-data";
const SETS_URL: &str = "https://api.scryfall.com/sets";
