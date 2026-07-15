//! MTGJSON integration: sealed-product **contents** — which sealed products a card is
//! found in, can be pulled from, or may be in.
//!
//! [MTGJSON](https://mtgjson.com) is a free, MIT-licensed daily dump of Magic data. Its
//! `AllPrintings.json` carries, per set, a `sealedProduct[]` array (each with a
//! `contents` breakdown + `identifiers.tcgplayerProductId`), the set's `booster` sheet
//! configs, its precon `decks`, and every card's `identifiers.scryfallId`. We stream
//! that one file (gzip), resolve each product's contents to individual cards, and
//! rebuild the `sealed_contents` table (see [`crate::entities::sealed_content`]).
//!
//! The three membership buckets map the "found in / can be in / may be in" split:
//! `contents.card` + precon `contents.deck` -> `contains`; `contents.pack` booster
//! sheets -> `booster`; `contents.variable` options -> `variable`; `contents.sealed`
//! recurses into its sub-product.
//!
//! Structure mirrors [`crate::tcgcsv`]: [`client`] for HTTP + gzip decode, [`model`] for
//! the trimmed serde shapes + the pure contents resolver, [`ingest`] for the DB rebuild.
//! Wired into [`crate::catalog::refresh_all`] **after** the Scryfall + TCGCSV syncs (so
//! cards and products both exist to join against), and version-gated on the file's HTTP
//! `ETag` so an unchanged file costs one conditional request.
//!
//! MTGJSON's contents are hand-curated upstream and lag: some products ship with
//! `contents: null` (e.g. Avatar's "Commander's Bundle" originally), so the cards
//! physically inside them would show no sealed product at all. [`fallback`] holds a small
//! committed snapshot of curated memberships that the ingest merges in **only for
//! products MTGJSON left empty**, so upstream stays authoritative and the fallback
//! self-retires as gaps fill. An entry flagged `supplement` merges its rows even into a
//! product upstream *does* describe — for an axis upstream is missing (the Commander's
//! Bundle again: its contents gained an incomplete `deck` reference plus textual-only
//! land packs; issue #352). Supplements are add-only by default; an explicit membership
//! override can reclassify only their curated cards when upstream has the right card under
//! the wrong certainty.
//!
//! Secret Lair Drop (`SLD`) products are the same gap with a twist: a drop's real contents
//! is the *cards in that drop*, which the app already tracks ([`crate::scryfall::drops`]),
//! so rather than hand-author them [`sld`] **derives** each null-contents drop product's
//! cards by matching the product name to its drop (self-maintaining as new drops sync),
//! merged under the same "only when MTGJSON left it empty" gate.
//!
//! Trade-off: `AllPrintings.json` is one ~600 MB document. We fetch the ~160 MB gzip and
//! parse only the trimmed structs, but the fetch still buffers the compressed body and
//! the resolved membership set is large (a normal set's booster sheets span ~its whole
//! card list), so a rebuild transiently uses a few hundred MB. It runs at most daily on
//! a background task, gated on the `ETag`. Per-set streaming to bound that further is
//! possible future work.

pub mod client;
mod error;
mod fallback;
pub mod ingest;
pub mod model;
mod progress;
pub(crate) mod sld;

pub use error::MtgjsonError;
/// Name of the sync progress span; `main.rs` scopes the `IndicatifLayer` to it
/// (alongside the Scryfall + TCGCSV spans).
pub(crate) use progress::SPAN_NAME as PROGRESS_SPAN_NAME;

/// Base URL of the MTGJSON v5 file API.
pub const BASE_URL: &str = "https://mtgjson.com/api/v5";

/// `ingest_state.dataset` key version-gating the sealed-contents rebuild (distinct from
/// the Scryfall `default_cards` + TCGCSV `tcgcsv_products` datasets).
pub const DATASET: &str = "mtgjson_sealed_contents";

/// Game this integration populates (matches the Scryfall provider's game id).
pub const GAME: &str = crate::scryfall::GAME;
