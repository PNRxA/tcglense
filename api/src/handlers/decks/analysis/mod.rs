//! Deck **analysis** (issue #596): the three questions a deck page — or a CLI, or any
//! API-key consumer — asks about a deck it already has, answered server-side.
//!
//! * **Composition** ([`stats`]) — copies, unique printings, the mana curve, colour
//!   identity, card types, and the hypergeometric draw odds for one card.
//! * **Legality** ([`legality`] + [`rules`]) — the per-card banned/restricted verdict from
//!   the catalog's Scryfall data, composed with the deck-construction rules that judge the
//!   deck as a whole (size, copy limit, command zone, colour identity).
//! * **Goldfish** ([`goldfish`]) — an opening hand, London mulligans, and a draw step.
//!
//! All three used to live in the SPA (`web/src/lib/deckStats.ts`, `legality.ts`,
//! `deckRules.ts`) and were unreachable from anything but a browser. They are the same
//! algorithms, moved whole: the SPA now renders what these endpoints return, and the CLI
//! and public API get the analysis for free. The vocabulary the *presentation* needs —
//! a status's label and colour, the format select's option list — stays client-side,
//! because that is a rendering concern and not an answer about the deck.
//!
//! Two design notes worth keeping:
//!
//! * These live on the **deck** surface (`/api/decks/{game}/{deck_id}/…`), beside `export`
//!   and `needed`, rather than under the `/api/tools` play-aid namespace. A tool there is
//!   backed by rows of its own (a life session); these are pure reads *of a deck*, so the
//!   deck owns them and `load_deck` already proves the caller may see it.
//! * Every one of them is a **`GET`**. They write nothing, so a read-only `tcgl_` key must
//!   be able to call them — and a goldfish hand a client can't reproduce with `curl` would
//!   defeat the point of moving it here. State the goldfish carries (the seed, how many
//!   mulligans, what was bottomed) rides in the query string instead of a table.
//!
//! Each read is mirrored on the public-sharing surface for a deck whose owner shared it
//! (`/api/u/{handle}/decks/{deck_id}/…`, see [`crate::handlers::sharing::decks`]); the
//! mirrors call the same three `analyse_*` entry points below, so a public deck and its
//! owner's copy can never disagree.

use std::collections::BTreeMap;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::entities::prelude::{Card, DeckCard, DeckSection};
use crate::entities::{card, deck_card, deck_section};
use crate::error::AppError;
use crate::handlers::shared::dto::{parse_legalities, split_csv, stored_faces};
use crate::state::AppState;

use super::DeckSectionResponse;

pub(crate) mod formats;
pub(crate) mod goldfish;
pub(crate) mod legality;
pub(crate) mod read;
pub(crate) mod rules;
pub(crate) mod stats;

pub use formats::{__path_list_deck_formats, list_deck_formats};
pub use read::{
    __path_deck_goldfish, __path_deck_legality, __path_deck_stats, deck_goldfish, deck_legality,
    deck_stats,
};

// The public-sharing mirrors (`/api/u/{handle}/decks/{deck_id}/…`) drive these directly, so
// a shared deck's analysis is the identical computation the owner sees.
pub(crate) use goldfish::{GoldfishHand, GoldfishParams, analyse_goldfish};
pub(crate) use legality::{DeckLegality, analyse_legality};
pub(crate) use stats::{DeckAnalytics, StatsParams, analyse_stats};

/// Everything the analysis reads off one catalog row, extracted once per deck card so the
/// pure modules below never touch a `card::Model`, a JSON blob, or a comma-joined column.
///
/// Two type lines, deliberately: composition counts a card's types off the **raw**
/// top-level `type_line` (word-exact, `Creature` / `Land` / …), while the construction
/// rules read a **lowercased front face with a faces fallback**, so a transforming
/// commander whose top-level line is absent still reads as legendary. That difference is
/// the SPA's, preserved rather than quietly unified — the two answer different questions,
/// and reconciling them here would change verdicts while moving code.
#[derive(Clone, Debug)]
pub(crate) struct CardFacts {
    /// Provider external id — what the wire and the goldfish `bottom` list address.
    pub id: String,
    pub name: String,
    /// The stored top-level type line, untouched (composition splits this itself).
    pub type_line: Option<String>,
    /// Front face only, lowercased, falling back to the first face's line.
    pub front_type_line: String,
    /// Rules text, falling back to the faces joined by newlines when the top level is
    /// null (Scryfall leaves it null on multi-faced cards).
    pub oracle_text: String,
    pub color_identity: Vec<String>,
    pub cmc: Option<f64>,
    /// Per-format legality object, or `None` when the row carries no legality data —
    /// which is "unknown", never "illegal".
    pub legalities: Option<BTreeMap<String, String>>,
}

impl From<&card::Model> for CardFacts {
    fn from(m: &card::Model) -> Self {
        let faces = stored_faces(m);
        let front_type_line = m
            .type_line
            .clone()
            .or_else(|| faces.first().and_then(|f| f.type_line.clone()))
            .unwrap_or_default()
            .split("//")
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let oracle_text = m.oracle_text.clone().unwrap_or_else(|| {
            faces
                .iter()
                .map(|f| f.oracle_text.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n")
        });

        Self {
            id: m.external_id.clone(),
            name: m.name.clone(),
            type_line: m.type_line.clone(),
            front_type_line,
            oracle_text,
            color_identity: split_csv(m.color_identity.clone()),
            cmc: m.cmc,
            legalities: parse_legalities(m.legalities.as_deref()),
        }
    }
}

/// One deck card as the analysis sees it: the catalog facts plus which section it sits in
/// and how many copies of it are there.
#[derive(Clone, Debug)]
pub(crate) struct AnalysisEntry {
    pub facts: CardFacts,
    pub section_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
}

impl AnalysisEntry {
    /// Copies, floored at zero — what composition and the construction rules count.
    pub(crate) fn copies(&self) -> i64 {
        i64::from(self.quantity)
            .saturating_add(i64::from(self.foil_quantity))
            .max(0)
    }

    /// Copies **unfloored**, which is what the per-card legality fold has always summed.
    /// Identical for every row a healthy database can produce; kept distinct so the port
    /// changes no verdict it didn't mean to.
    pub(crate) fn signed_copies(&self) -> i64 {
        i64::from(self.quantity).saturating_add(i64::from(self.foil_quantity))
    }
}

/// A deck loaded for analysis: its sections in display order, and every card in it whose
/// catalog row still exists.
pub(crate) struct DeckAnalysisInput {
    pub sections: Vec<DeckSectionResponse>,
    pub entries: Vec<AnalysisEntry>,
}

impl DeckAnalysisInput {
    /// The deck **proper** — everything outside a maybeboard section (issue #570). Every
    /// reader that answers "what is this deck" splits here.
    pub(crate) fn deck_proper(&self) -> Vec<&AnalysisEntry> {
        let maybeboard: std::collections::HashSet<i32> = self
            .sections
            .iter()
            .filter(|s| s.is_maybeboard)
            .map(|s| s.id)
            .collect();
        self.entries
            .iter()
            .filter(|e| !maybeboard.contains(&e.section_id))
            .collect()
    }

    /// The entries whose section is in `ids`, in the deck's own row order.
    pub(crate) fn in_sections(&self, ids: &[i32]) -> Vec<&AnalysisEntry> {
        let wanted: std::collections::HashSet<i32> = ids.iter().copied().collect();
        self.entries
            .iter()
            .filter(|e| wanted.contains(&e.section_id))
            .collect()
    }

    /// The same selection as [`Self::in_sections`], as **row indices** — what the goldfish
    /// needs, since a drawn slot has to find its catalog model in the parallel row vector.
    pub(crate) fn row_indices_in_sections(&self, ids: &[i32]) -> Vec<usize> {
        let wanted: std::collections::HashSet<i32> = ids.iter().copied().collect();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| wanted.contains(&e.section_id))
            .map(|(index, _)| index)
            .collect()
    }
}

/// Load a deck's sections and cards for analysis.
///
/// Ordered by card name then deck-card id, the same ordering `deck_detail` returns, so a
/// goldfish shuffle is seeded from a stable sequence regardless of insertion order — a
/// seed that produced a hand yesterday reproduces it today. A deck card whose catalog row
/// is gone (a re-import) is LEFT-joined to `None` and skipped, exactly as every other deck
/// reader does.
pub(crate) async fn load_analysis(
    state: &AppState,
    deck_id: i32,
) -> Result<DeckAnalysisInput, AppError> {
    let sections: Vec<DeckSectionResponse> = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck_id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?
        .into_iter()
        .map(DeckSectionResponse::from)
        .collect();

    let rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.eq(deck_id))
        .order_by_asc(card::Column::Name)
        .order_by_asc(deck_card::Column::Id)
        .all(&state.db)
        .await?;

    let entries = rows
        .into_iter()
        .filter_map(|(item, card)| {
            card.map(|c| AnalysisEntry {
                facts: CardFacts::from(&c),
                section_id: item.section_id,
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();

    Ok(DeckAnalysisInput { sections, entries })
}

/// Load a deck's analysis input **and** keep the catalog rows, for the goldfish reader —
/// it hands drawn cards back on the wire as full card payloads, so it needs the models the
/// facts were derived from.
pub(crate) async fn load_analysis_with_cards(
    state: &AppState,
    deck_id: i32,
) -> Result<(DeckAnalysisInput, Vec<card::Model>), AppError> {
    let sections: Vec<DeckSectionResponse> = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck_id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?
        .into_iter()
        .map(DeckSectionResponse::from)
        .collect();

    let rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.eq(deck_id))
        .order_by_asc(card::Column::Name)
        .order_by_asc(deck_card::Column::Id)
        .all(&state.db)
        .await?;

    let mut entries = Vec::with_capacity(rows.len());
    let mut models = Vec::with_capacity(rows.len());
    for (item, card) in rows {
        let Some(c) = card else { continue };
        entries.push(AnalysisEntry {
            facts: CardFacts::from(&c),
            section_id: item.section_id,
            quantity: item.quantity,
            foil_quantity: item.foil_quantity,
        });
        models.push(c);
    }

    Ok((DeckAnalysisInput { sections, entries }, models))
}

/// Builders the module's own unit tests construct decks with, so a test names only the
/// facts its case is about.
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;

    /// A card with nothing set but its id and name.
    pub(crate) fn card(id: &str, name: &str) -> CardFacts {
        CardFacts {
            id: id.to_string(),
            name: name.to_string(),
            type_line: None,
            front_type_line: String::new(),
            oracle_text: String::new(),
            color_identity: Vec::new(),
            cmc: None,
            legalities: None,
        }
    }

    impl CardFacts {
        pub(crate) fn type_line(mut self, line: &str) -> Self {
            self.front_type_line = line.split("//").next().unwrap_or_default().to_lowercase();
            self.type_line = Some(line.to_string());
            self
        }

        pub(crate) fn oracle(mut self, text: &str) -> Self {
            self.oracle_text = text.to_string();
            self
        }

        /// Colour identity as the catalog stores it — a comma-joined list.
        pub(crate) fn colors(mut self, csv: &str) -> Self {
            self.color_identity = split_csv(Some(csv.to_string()));
            self
        }

        pub(crate) fn cmc(mut self, value: f64) -> Self {
            self.cmc = Some(value);
            self
        }

        pub(crate) fn legal(mut self, format: &str, status: &str) -> Self {
            self.legalities
                .get_or_insert_with(BTreeMap::new)
                .insert(format.to_string(), status.to_string());
            self
        }
    }

    /// One deck row: `entry(card_id, name, section_id, quantity, foil_quantity)`.
    pub(crate) fn entry(
        id: &str,
        name: &str,
        section_id: i32,
        quantity: i32,
        foil_quantity: i32,
    ) -> AnalysisEntry {
        AnalysisEntry {
            facts: card(id, name),
            section_id,
            quantity,
            foil_quantity,
        }
    }

    impl AnalysisEntry {
        pub(crate) fn type_line(mut self, line: &str) -> Self {
            self.facts = self.facts.type_line(line);
            self
        }
        pub(crate) fn oracle(mut self, text: &str) -> Self {
            self.facts = self.facts.oracle(text);
            self
        }
        pub(crate) fn colors(mut self, csv: &str) -> Self {
            self.facts = self.facts.colors(csv);
            self
        }
        pub(crate) fn cmc(mut self, value: f64) -> Self {
            self.facts = self.facts.cmc(value);
            self
        }
        pub(crate) fn legal(mut self, format: &str, status: &str) -> Self {
            self.facts = self.facts.legal(format, status);
            self
        }
    }

    pub(crate) fn section(id: i32, name: &str, is_maybeboard: bool) -> DeckSectionResponse {
        DeckSectionResponse {
            id,
            name: name.to_string(),
            position: id,
            is_maybeboard,
        }
    }

    pub(crate) fn deck(
        sections: Vec<DeckSectionResponse>,
        entries: Vec<AnalysisEntry>,
    ) -> DeckAnalysisInput {
        DeckAnalysisInput { sections, entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::card_model;

    /// Scryfall leaves the top-level `type_line` and `oracle_text` null on some multi-faced
    /// cards and puts them on the faces, and `scryfall::map` stores that verbatim — so the
    /// faces fallback is the only thing standing between a transforming commander and a
    /// deck reported illegal because its commander read as typeless. Nothing else in the
    /// suite exercises it: every other fixture sets the top-level fields directly.
    #[test]
    fn card_facts_fall_back_to_the_faces() {
        let faces = serde_json::json!([
            {
                "name": "Delver of Secrets",
                "type_line": "Legendary Creature — Human Wizard",
                "oracle_text": "At the beginning of your upkeep, look at the top card."
            },
            { "name": "Insectile Aberration", "type_line": "Creature — Human Insect", "oracle_text": "Flying" }
        ])
        .to_string();
        let model = card::Model {
            type_line: None,
            oracle_text: None,
            card_faces: Some(faces),
            ..card_model(1)
        };

        let facts = CardFacts::from(&model);
        assert_eq!(facts.front_type_line, "legendary creature — human wizard");
        assert!(facts.oracle_text.contains("beginning of your upkeep"));
        assert!(
            facts.oracle_text.contains("Flying"),
            "both faces' rules text joins, so a back-face keyword still reads"
        );
        // Composition still counts types off the *raw* top-level line, which is absent here —
        // the two readings answer different questions and this pins the difference.
        assert_eq!(facts.type_line, None);
    }

    /// The top level wins when it is present, faces or no faces.
    #[test]
    fn card_facts_prefer_the_top_level_line() {
        let model = card::Model {
            type_line: Some("Creature — Bear // Land".to_string()),
            oracle_text: Some("Vigilance".to_string()),
            card_faces: Some(
                serde_json::json!([{ "type_line": "Land", "oracle_text": "Tap for {G}." }])
                    .to_string(),
            ),
            ..card_model(2)
        };
        let facts = CardFacts::from(&model);
        assert_eq!(facts.front_type_line, "creature — bear ");
        assert_eq!(facts.oracle_text, "Vigilance");
    }
}
