//! Serde shapes for the Commander Spellbook data: the **upstream** variant (trimmed to
//! what's kept) and the **compact record** the tables are written from — which is also
//! the mirror's wire format, so a consumer and the origin build rows from one shape.
//!
//! Every upstream field is optional, for the reason the Scryfall bulk-catalog lesson
//! taught: a required field on a document nobody here controls is a single point of
//! failure for the whole dataset. A variant missing what makes it a combo (an id, a card
//! piece, an oracle id on every piece) is dropped by [`ComboRecord::from_variant`]; one
//! missing anything else is kept with the field blank.

use serde::{Deserialize, Serialize};

use crate::entities::{combo, combo_piece};
use crate::handlers::decks::COLOUR_ORDER;

// ---------- Upstream ----------

/// One entry of the bulk export's `variants` array, trimmed to the fields kept.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub id: Option<String>,
    #[serde(default)]
    pub uses: Vec<CardInVariant>,
    #[serde(default)]
    pub requires: Vec<TemplateInVariant>,
    #[serde(default)]
    pub produces: Vec<FeatureProduced>,
    /// Colour identity as a bare letter string (`"GWUB"`, `"C"` for colourless).
    pub identity: Option<String>,
    pub mana_needed: Option<String>,
    pub mana_value_needed: Option<i64>,
    pub easy_prerequisites: Option<String>,
    pub notable_prerequisites: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub popularity: Option<i64>,
    pub bracket_tag: Option<String>,
    pub legalities: Option<Legalities>,
}

/// A card the variant uses, and the state it starts in.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInVariant {
    pub card: Option<VariantCard>,
    #[serde(default)]
    pub zone_locations: Vec<String>,
    pub must_be_commander: Option<bool>,
    pub quantity: Option<i64>,
}

/// The card itself: only its identity is kept (the catalog holds the rest).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantCard {
    pub name: Option<String>,
    pub oracle_id: Option<String>,
}

/// A **template** the variant requires — "any free sacrifice outlet" — a Scryfall query
/// upstream resolves and this app can't, so only the name is kept.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInVariant {
    pub template: Option<Template>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub name: Option<String>,
}

/// An effect the variant produces.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureProduced {
    pub feature: Option<Feature>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub name: Option<String>,
    /// `S` standalone, `C` contextual, `H` helper, `HU` / `PU` hidden / public utility.
    pub status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Legalities {
    pub commander: Option<bool>,
}

// ---------- Compact record ----------

/// One combo as the tables store it — and as the mirror serves it, one per JSONL line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboRecord {
    /// Upstream's variant id.
    pub id: String,
    /// Colour identity letters in WUBRG order; empty for colourless.
    #[serde(default)]
    pub identity: Vec<String>,
    #[serde(default)]
    pub mana_needed: String,
    #[serde(default)]
    pub mana_value_needed: Option<i32>,
    #[serde(default)]
    pub prerequisites: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub popularity: i32,
    #[serde(default)]
    pub bracket_tag: String,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub produces: Vec<ProducedFeature>,
    #[serde(default)]
    pub commander_legal: bool,
    pub pieces: Vec<PieceRecord>,
}

/// One produced effect: its name and upstream's status letter (see [`Feature::status`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducedFeature {
    pub name: String,
    #[serde(default)]
    pub status: String,
}

/// One card piece of a [`ComboRecord`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PieceRecord {
    pub oracle_id: String,
    pub name: String,
    #[serde(default = "one")]
    pub quantity: i32,
    #[serde(default)]
    pub must_be_commander: bool,
    #[serde(default)]
    pub zones: Vec<String>,
}

fn one() -> i32 {
    1
}

/// Clamp an upstream integer into the `i32` the column holds.
fn clamp_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Colour letters in WUBRG order out of upstream's bare-letter identity string. `C`
/// (upstream's colourless marker) and anything unrecognised fold to nothing.
fn identity_letters(identity: Option<&str>) -> Vec<String> {
    let raw = identity.unwrap_or_default().to_ascii_uppercase();
    COLOUR_ORDER
        .iter()
        .filter(|letter| raw.contains(*letter))
        .map(|letter| (*letter).to_string())
        .collect()
}

impl ComboRecord {
    /// Reduce an upstream variant to the record stored, or `None` when it isn't a combo
    /// this app can ever match: no id, no card pieces, or a piece with no `oracle_id` (a
    /// piece nothing in the catalog could satisfy makes the whole combo unmatchable, and
    /// listing it on a card page would name a combo the reader can never complete here).
    pub fn from_variant(variant: Variant) -> Option<Self> {
        let id = variant.id.filter(|id| !id.trim().is_empty())?;
        let mut pieces = Vec::with_capacity(variant.uses.len());
        for used in variant.uses {
            let card = used.card?;
            let oracle_id = card.oracle_id.filter(|id| !id.trim().is_empty())?;
            pieces.push(PieceRecord {
                oracle_id,
                name: card.name.unwrap_or_default(),
                quantity: clamp_i32(used.quantity.unwrap_or(1)).max(1),
                must_be_commander: used.must_be_commander.unwrap_or(false),
                zones: used.zone_locations,
            });
        }
        if pieces.is_empty() {
            return None;
        }
        let templates = variant
            .requires
            .into_iter()
            .filter_map(|t| t.template.and_then(|t| t.name))
            .filter(|name| !name.trim().is_empty())
            .collect();
        let produces = variant
            .produces
            .into_iter()
            .filter_map(|p| p.feature)
            .filter_map(|f| {
                f.name.map(|name| ProducedFeature {
                    name,
                    status: f.status.unwrap_or_default(),
                })
            })
            .collect();
        let prerequisites = [variant.easy_prerequisites, variant.notable_prerequisites]
            .into_iter()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Some(Self {
            id,
            identity: identity_letters(variant.identity.as_deref()),
            mana_needed: variant.mana_needed.unwrap_or_default(),
            mana_value_needed: variant.mana_value_needed.map(clamp_i32),
            prerequisites,
            description: variant.description.unwrap_or_default(),
            notes: variant.notes.unwrap_or_default(),
            popularity: clamp_i32(variant.popularity.unwrap_or(0)),
            bracket_tag: variant.bracket_tag.unwrap_or_default(),
            templates,
            produces,
            commander_legal: variant
                .legalities
                .and_then(|l| l.commander)
                .unwrap_or(false),
            pieces,
        })
    }

    /// Rebuild the record from stored rows — the mirror's re-serve. `pieces` in stored
    /// `position` order.
    pub fn from_models(combo: &combo::Model, pieces: &[combo_piece::Model]) -> Self {
        Self {
            id: combo.external_id.clone(),
            identity: split_list(&combo.color_identity),
            mana_needed: combo.mana_needed.clone(),
            mana_value_needed: combo.mana_value_needed,
            prerequisites: combo.prerequisites.clone(),
            description: combo.description.clone(),
            notes: combo.notes.clone(),
            popularity: combo.popularity,
            bracket_tag: combo.bracket_tag.clone(),
            templates: serde_json::from_str(&combo.templates).unwrap_or_default(),
            produces: serde_json::from_str(&combo.produces).unwrap_or_default(),
            commander_legal: combo.commander_legal,
            pieces: pieces
                .iter()
                .map(|p| PieceRecord {
                    oracle_id: p.oracle_id.clone(),
                    name: p.name.clone(),
                    quantity: p.quantity,
                    must_be_commander: p.must_be_commander,
                    zones: split_list(&p.zone_locations),
                })
                .collect(),
        }
    }
}

/// Split a comma-joined stored list; `""` is the empty list.
pub(crate) fn split_list(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed-down entry from the live export (2026-09-08), pinning the field names the
    /// parser reads — a rename upstream must fail here, not silently drop every combo.
    const LIVE_VARIANT: &str = r#"{"id": "245-2034-6705", "status": "OK", "uses": [
        {"card": {"id": 245, "name": "Marneus Calgar", "oracleId": "f6479f7e-01f4-49f1-a444-04bf38934f6b", "spoiler": false, "faces": 1, "typeLine": "Legendary Creature — Astartes Warrior"}, "zoneLocations": ["B"], "battlefieldCardState": "", "mustBeCommander": true, "quantity": 1, "usedFace": null},
        {"card": {"id": 2034, "name": "Ashnod's Altar", "oracleId": "4d18bcba-a346-445e-a182-6cc30b7e066d"}, "zoneLocations": ["B"], "mustBeCommander": false, "quantity": 1}
      ], "requires": [{"template": {"id": 3, "name": "A creature token"}, "zoneLocations": ["B"], "quantity": 1}],
      "produces": [{"feature": {"id": 24, "name": "Infinite card draw", "uncountable": true, "status": "S"}, "quantity": 1}, {"feature": {"id": 61, "name": "Infinite draw triggers", "status": "H"}, "quantity": 1}],
      "of": [{"id": 11231}], "includes": [{"id": 26876}], "identity": "GWUB", "manaNeeded": "{6}", "manaValueNeeded": 6,
      "easyPrerequisites": "", "notablePrerequisites": "Marneus has a counter on it.", "description": "Activate Marneus by paying {6}.", "notes": "", "popularity": 4, "spoiler": false, "bracketTag": "S",
      "legalities": {"commander": true, "pauperCommander": false}, "prices": {"tcgplayer": "26.11"}, "variantCount": 14}"#;

    #[test]
    fn reduces_a_live_variant_to_its_record() {
        let variant: Variant = serde_json::from_str(LIVE_VARIANT).expect("parse");
        let record = ComboRecord::from_variant(variant).expect("a combo");
        assert_eq!(record.id, "245-2034-6705");
        // WUBRG order regardless of upstream's spelling.
        assert_eq!(record.identity, vec!["W", "U", "B", "G"]);
        assert_eq!(record.mana_needed, "{6}");
        assert_eq!(record.mana_value_needed, Some(6));
        assert_eq!(record.prerequisites, "Marneus has a counter on it.");
        assert_eq!(record.popularity, 4);
        assert_eq!(record.bracket_tag, "S");
        assert!(record.commander_legal);
        assert_eq!(record.templates, vec!["A creature token"]);
        assert_eq!(
            record.produces,
            vec![
                ProducedFeature {
                    name: "Infinite card draw".into(),
                    status: "S".into()
                },
                ProducedFeature {
                    name: "Infinite draw triggers".into(),
                    status: "H".into()
                }
            ]
        );
        assert_eq!(record.pieces.len(), 2);
        assert_eq!(
            record.pieces[0].oracle_id,
            "f6479f7e-01f4-49f1-a444-04bf38934f6b"
        );
        assert!(record.pieces[0].must_be_commander);
        assert_eq!(record.pieces[0].zones, vec!["B"]);
        assert!(!record.pieces[1].must_be_commander);
    }

    #[test]
    fn colourless_and_unknown_identity_letters_fold_to_nothing() {
        assert!(identity_letters(Some("C")).is_empty());
        assert!(identity_letters(None).is_empty());
        assert_eq!(identity_letters(Some("rg")), vec!["R", "G"]);
    }

    #[test]
    fn a_variant_missing_what_makes_it_matchable_is_dropped() {
        let no_id: Variant =
            serde_json::from_str(r#"{"uses": [{"card": {"name": "X", "oracleId": "o"}}]}"#)
                .expect("parse");
        assert!(ComboRecord::from_variant(no_id).is_none());
        let no_pieces: Variant = serde_json::from_str(r#"{"id": "1", "uses": []}"#).expect("parse");
        assert!(ComboRecord::from_variant(no_pieces).is_none());
        let no_oracle: Variant = serde_json::from_str(
            r#"{"id": "1", "uses": [{"card": {"name": "X", "oracleId": "o"}}, {"card": {"name": "Y", "oracleId": null}}]}"#,
        )
        .expect("parse");
        assert!(ComboRecord::from_variant(no_oracle).is_none());
        // Everything else may be absent.
        let minimal: Variant = serde_json::from_str(
            r#"{"id": "1", "uses": [{"card": {"name": "X", "oracleId": "o"}}]}"#,
        )
        .expect("parse");
        let record = ComboRecord::from_variant(minimal).expect("a combo");
        assert_eq!(record.pieces[0].quantity, 1);
        assert_eq!(record.popularity, 0);
        assert!(!record.commander_legal);
    }

    #[test]
    fn the_record_round_trips_through_its_own_json() {
        let variant: Variant = serde_json::from_str(LIVE_VARIANT).expect("parse");
        let record = ComboRecord::from_variant(variant).expect("a combo");
        let line = serde_json::to_string(&record).expect("serialise");
        assert!(!line.contains('\n'), "one record per JSONL line");
        let back: ComboRecord = serde_json::from_str(&line).expect("deserialise");
        assert_eq!(back, record);
    }
}
