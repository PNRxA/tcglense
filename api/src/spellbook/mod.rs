//! Commander Spellbook integration: the **combo database** behind the deck page's
//! "Combos" panel and the card page's "Combos with" (issue #683).
//!
//! "Does this deck have infinite combos?" cannot be answered from what the catalog holds:
//! a combo is a fact about how *several* cards interact, not a phrase in any one card's
//! rules text, so unlike the bracket signals or the card roles it is not a grammar problem
//! — it is a dataset. [Commander Spellbook](https://commanderspellbook.com) curates that
//! dataset and publishes it as a bulk export; this module syncs it into `combos` +
//! `combo_pieces`, keyed by **`oracle_id`** so every printing of a card participates, and
//! the deck analysis reads it like it reads the catalog's legality object — a provider
//! fact, never inferred.
//!
//! **Not every provider fetches upstream.** The upstream export is one ~650 MB JSON
//! document (~28 MB gzipped), and Commander Spellbook asks for sparse traffic. So the
//! mirror origin (`SYNC_FROM_UPSTREAM=true`) is the only instance that fetches and parses
//! it — streamed through [`stream`], never buffered whole — and re-serves a **compact
//! snapshot** (`/api/mirror/spellbook/combos`, gzipped JSONL of [`model::ComboRecord`],
//! a few MB) that every other instance imports instead: the same stance as the Secret
//! Lair drop tables, where a self-host never scrapes Scryfall itself. Both paths land in
//! [`ingest::replace_combos`], so the tables are shaped identically whichever document
//! fed them.
//!
//! **Attribution.** Commander Spellbook's API terms ask for credit and a link back; every
//! combo on the wire carries its `commanderspellbook.com/combo/{id}` URL, the SPA panels
//! name the source, and [`SITE_URL`] / [`ATTRIBUTION`] are the one spelling of it.

pub mod dummy;
pub mod ingest;
pub mod model;
pub mod snapshot;
pub mod stream;

pub use ingest::{refresh, replace_combos};

/// The game the combo database describes (the bookkeeping rows' `game`).
pub const GAME: &str = crate::scryfall::GAME;

/// `ingest_state.dataset` key for the combo sync (`(mtg, combos)`); `source_updated_at`
/// holds the last imported document's `ETag`.
pub const DATASET: &str = "combos";

/// Upstream bulk export: every published variant, one gzipped JSON document. The `.gz`
/// twin of `variants.json`, served with a strong `ETag` — the version gate.
pub const VARIANTS_URL: &str = "https://json.commanderspellbook.com/variants.json.gz";

/// The site the data comes from — the link the attribution asks for.
pub const SITE_URL: &str = "https://commanderspellbook.com";

/// How the source is named wherever the data is shown.
pub const ATTRIBUTION: &str = "Commander Spellbook";

/// The public page of one combo, by upstream's variant id.
pub fn combo_url(external_id: &str) -> String {
    format!("{SITE_URL}/combo/{external_id}")
}
