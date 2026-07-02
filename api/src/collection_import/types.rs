//! Value types shared across the collection-import engine (provider, reconcile mode,
//! fetched holdings, import summary).

use serde::{Deserialize, Serialize};

/// A collection provider we can import from. One variant per external service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CollectionProvider"))]
pub enum Provider {
    Archidekt,
    Moxfield,
}

impl Provider {
    /// The provider's stable string id — as it appears in the API and in stored
    /// `collection_sources` rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Archidekt => "archidekt",
            Provider::Moxfield => "moxfield",
        }
    }

    /// Parse a provider id case-insensitively. `None` for an unknown provider.
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "archidekt" => Some(Provider::Archidekt),
            "moxfield" => Some(Provider::Moxfield),
            _ => None,
        }
    }

    /// Human-readable provider name for UI / error copy.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Archidekt => "Archidekt",
            Provider::Moxfield => "Moxfield",
        }
    }

    /// Whether this provider can supply a collection for `game`. Both are Magic-only
    /// (their card ids / printings key off Scryfall data).
    pub fn supports_game(self, game: &str) -> bool {
        match self {
            Provider::Archidekt | Provider::Moxfield => game == crate::scryfall::GAME,
        }
    }

    /// Whether this provider's **live network** import is currently enabled — the one-off
    /// URL/link import and the saved-link re-sync, both of which fetch from the provider's
    /// API. Moxfield is **temporarily disabled**: its API only serves clients whose
    /// `User-Agent` it has explicitly approved, which we don't have yet, so a live fetch is
    /// throttled / tarpitted into failure (see `moxfield::fetch_failure`). Its **CSV
    /// upload** needs no network and is unaffected — that stays the supported way to import
    /// a Moxfield collection for now.
    ///
    /// This is the single source of truth for the disable; the import handlers gate on it
    /// (see `handlers::collection::import`) and the web UI hides the disabled provider from
    /// the link-import picker. Re-enable by flipping the arm to `true` once an approved
    /// `MOXFIELD_USER_AGENT` is configured.
    pub fn network_import_enabled(self) -> bool {
        match self {
            Provider::Archidekt => true,
            Provider::Moxfield => false,
        }
    }

    /// A canonical, user-facing URL for a collection id on this provider (for linking
    /// back from the UI). `id` is a validated provider collection id.
    pub fn collection_url(self, id: &str) -> String {
        match self {
            Provider::Archidekt => format!("https://archidekt.com/collection/v2/{id}"),
            Provider::Moxfield => format!("https://moxfield.com/collection/{id}"),
        }
    }
}

/// Deployment-level provider settings that a fetch needs beyond the shared HTTP client
/// (today: Moxfield's approved User-Agent). Captured once at startup into the import
/// queue so background workers don't reach back into the full app config.
#[derive(Debug, Clone, Default)]
pub struct ProviderSettings {
    /// `User-Agent` for Moxfield requests. Moxfield's API only serves approved agents
    /// (see `Config::moxfield_user_agent`); `None` falls back to the client default.
    pub moxfield_user_agent: Option<String>,
}

/// How an import reconciles with the user's existing collection for the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_round_trip() {
        for provider in [Provider::Archidekt, Provider::Moxfield] {
            assert_eq!(Provider::from_id(provider.as_str()), Some(provider));
        }
        assert_eq!(Provider::from_id("MOXFIELD"), Some(Provider::Moxfield));
        assert_eq!(Provider::from_id("deckbox"), None);
    }

    #[test]
    fn moxfield_network_import_is_disabled_but_archidekt_is_enabled() {
        // Moxfield's live URL import / re-sync is turned off pending an approved
        // User-Agent (its CSV upload is unaffected — that path never checks this).
        assert!(!Provider::Moxfield.network_import_enabled());
        assert!(Provider::Archidekt.network_import_enabled());
    }
}
