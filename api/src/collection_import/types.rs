//! Value types shared across the collection-import engine (provider, reconcile mode,
//! fetched holdings, import summary).

use serde::{Deserialize, Serialize};

/// A collection provider we can import from. One variant per external service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Archidekt,
}

impl Provider {
    /// The provider's stable string id — as it appears in the API and in stored
    /// `collection_sources` rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Archidekt => "archidekt",
        }
    }

    /// Parse a provider id case-insensitively. `None` for an unknown provider.
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "archidekt" => Some(Provider::Archidekt),
            _ => None,
        }
    }

    /// Human-readable provider name for UI / error copy.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Archidekt => "Archidekt",
        }
    }

    /// Whether this provider can supply a collection for `game`. Archidekt is
    /// Magic-only (its card ids are Scryfall ids).
    pub fn supports_game(self, game: &str) -> bool {
        match self {
            Provider::Archidekt => game == crate::scryfall::GAME,
        }
    }

    /// A canonical, user-facing URL for a collection id on this provider (for linking
    /// back from the UI). `id` is a validated provider collection id.
    pub fn collection_url(self, id: &str) -> String {
        match self {
            Provider::Archidekt => format!("https://archidekt.com/collection/v2/{id}"),
        }
    }
}

/// How an import reconciles with the user's existing collection for the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReconcileMode {
    /// Set each imported card to the imported counts; leave cards not in the import
    /// untouched. Idempotent and non-destructive.
    Overwrite,
    /// Make the collection exactly mirror the import: set imported cards and delete
    /// owned cards that aren't in the import.
    Replace,
    /// Add the imported counts on top of the existing counts.
    Merge,
    /// An **incremental** mirror: fetch the provider collection most-recently-updated
    /// first and stop paging once a whole page already matches what we hold, then
    /// overwrite the fetched cards' seen finishes. Fast (it doesn't re-page an
    /// unchanged collection under the provider rate limit) but, because it never fetches
    /// the whole collection, it only touches recently-changed cards — it does **not**
    /// remove cards deleted upstream (a full [`Replace`](Self::Replace) does). See
    /// [`reconcile_smart`].
    Smart,
}

/// One card holding pulled from a provider, before aggregation. `external_card_id` is
/// the provider's card id in the form our catalog stores (`cards.external_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedHolding {
    pub external_card_id: String,
    pub foil: bool,
    pub quantity: i32,
}

/// The outcome of an import, surfaced to the user.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub provider: &'static str,
    pub mode: ReconcileMode,
    /// Total holding rows fetched from the provider (before aggregation by card).
    pub total_rows: usize,
    /// Distinct cards in the provider collection (after aggregating rows by card).
    pub distinct_cards: usize,
    /// Distinct cards that matched a card in our catalog and were applied.
    pub matched_cards: usize,
    /// Distinct cards with no match in our catalog (skipped).
    pub unmatched_cards: usize,
    /// A capped sample of unmatched card ids, for user feedback / debugging.
    pub unmatched_sample: Vec<String>,
    /// Regular copies the provider reported across all matched cards.
    pub regular_copies: i64,
    /// Foil copies the provider reported across all matched cards.
    pub foil_copies: i64,
    /// Owned cards removed by the reconcile (non-zero only in `Replace` mode).
    pub removed_cards: usize,
    /// `Smart` only: whether the fetch stopped early having reached already-synced
    /// cards (vs. scanning the whole collection). Always `false` for other modes.
    pub stopped_early: bool,
}
