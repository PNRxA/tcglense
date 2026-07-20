//! TCGCSV integration: a one-time historic price backfill plus the daily sealed-product
//! catalog + price sync.
//!
//! [TCGCSV](https://tcgcsv.com) is a free, keyless daily mirror of TCGplayer's
//! catalog and prices. Two features live here:
//!
//! - **Historic price backfill** ([`backfill`]): on the first boot after the Scryfall
//!   card sync populates `cards.tcgplayer_id` (and, for products, after the first
//!   product sync), it walks TCGCSV's daily price *archives* (one solid-PPMd `7z` per
//!   day since 2024-02-08) and fills `card_price_history` **and** `product_price_history`
//!   for the days before we began capturing our own daily snapshots. It runs **once**
//!   (gated on an `ingest_state` row) and never overwrites an existing row.
//! - **Sealed products** ([`ingest`] + [`price_history`]): a daily sweep of the groups +
//!   products feeds that imports MTG sealed products (booster boxes, bundles, decks, …)
//!   into `products` and captures their daily market prices into `product_price_history`.
//!   Version-gated on `last-updated.txt` and wired into `catalog::refresh_all` /
//!   `catalog::snapshot_all` alongside the card sync.
//!
//! Structure mirrors `scryfall/` where sensible: [`client`] for HTTP, [`model`] for the
//! serde shapes + pure folding, [`classify`] for the pure sealed/type classifiers.

pub mod backfill;
pub mod classify;
pub mod client;
mod error;
pub mod ingest;
pub mod model;
pub mod msrp;
pub mod price_history;
mod progress;
pub mod sld_msrp;
pub mod sld_release;

pub use error::BackfillError;
/// Name of the sync progress span; `main.rs` scopes the `IndicatifLayer` to it
/// (alongside the Scryfall import span).
pub(crate) use progress::SPAN_NAME as PROGRESS_SPAN_NAME;

/// Base URL of the TCGCSV service.
pub const BASE_URL: &str = "https://tcgcsv.com";

/// TCGplayer category id for Magic: The Gathering (the only game we sync).
pub const MTG_CATEGORY_ID: u32 = 1;

/// `ingest_state.dataset` key that gates the one-time historic price backfill (distinct
/// from the Scryfall `default_cards` card-data dataset, which the status route reports).
pub const DATASET: &str = "tcgcsv_price_backfill";

/// `ingest_state.dataset` key that version-gates the daily sealed-product sweep.
pub const PRODUCTS_DATASET: &str = "tcgcsv_products";

/// Game this integration populates (matches the Scryfall provider's game id).
pub const GAME: &str = crate::scryfall::GAME;
