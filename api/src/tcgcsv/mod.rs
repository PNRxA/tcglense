//! TCGCSV integration: a one-time historic price backfill.
//!
//! [TCGCSV](https://tcgcsv.com) is a free, keyless daily mirror of TCGplayer's
//! catalog and prices. On the first boot after this feature ships — and after the
//! Scryfall card sync has populated `cards.tcgplayer_id` — the backfill walks
//! TCGCSV's daily price *archives* (one solid-PPMd `7z` per day since 2024-02-08)
//! and fills `card_price_history` for the days before we started capturing our own
//! daily Scryfall snapshots. It runs **once** (gated on an `ingest_state` row) and
//! never overwrites an existing `(game, card, date)` row, so the live daily snapshot
//! keeps extending the series afterwards.
//!
//! Structure mirrors `scryfall/` where sensible: [`client`] for HTTP, [`model`] for
//! the serde shapes + pure folding, [`backfill`] for the archive walk and DB writes.
//! Kept generic over category/group where cheap so part 2 (sealed products) can
//! extend it.

pub mod backfill;
pub mod client;
mod error;
pub mod model;

pub use error::BackfillError;

/// Base URL of the TCGCSV service.
pub const BASE_URL: &str = "https://tcgcsv.com";

/// TCGplayer category id for Magic: The Gathering (the only game we backfill).
pub const MTG_CATEGORY_ID: u32 = 1;

/// `ingest_state.dataset` key that gates the one-time backfill (distinct from the
/// Scryfall `default_cards` card-data dataset, which the status route reports).
pub const DATASET: &str = "tcgcsv_price_backfill";

/// Game this backfill populates (matches the Scryfall provider's game id).
pub const GAME: &str = crate::scryfall::GAME;
