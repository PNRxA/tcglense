//! Provider-neutral deck-import values.

use crate::collection_import::Provider;
use crate::entities::deck;

/// One provider/export card row before it is resolved to the local catalog.
///
/// Network payloads normally carry a Scryfall id, while Moxfield CSV/plain-text rows
/// commonly identify a printing by `(set_code, collector_number)`. Name is retained as
/// the final fallback and for useful unmatched-card feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckCardRow {
    pub section: String,
    pub card_name: String,
    pub external_card_id: Option<String>,
    pub set_code: Option<String>,
    pub collector_number: Option<String>,
    pub foil: bool,
    pub quantity: i32,
}

/// A whole provider deck, normalized but not yet resolved or written.
#[derive(Debug)]
pub struct ParsedDeck {
    pub provider: Provider,
    pub name: String,
    pub format: Option<String>,
    pub rows: Vec<DeckCardRow>,
}

/// Internal result of the all-or-nothing create path. The handler returns a lightweight
/// deck-list header; full sections/cards remain behind the normal deck detail endpoint.
#[derive(Debug)]
pub struct CreatedDeckImport {
    pub deck: deck::Model,
    pub card_count: i64,
    pub provider: Provider,
    pub total_rows: usize,
    pub matched_cards: usize,
    pub unmatched_cards: usize,
    pub unmatched_sample: Vec<String>,
}

/// Uploaded deck-list representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum DeckImportFileFormat {
    Csv,
    Text,
}
