//! **Combos in this deck** (issue #683): which Commander Spellbook combos the deck can
//! assemble, and which it is one card short of — the panel every competitor's deck page
//! has, and the one question the bracket estimate ships a caveat for.
//!
//! **Nothing here is inferred from rules text.** A combo is a fact about how several cards
//! interact, and the only place that fact exists is a curated database; this module reads
//! the one the sync keeps (`combos` + `combo_pieces`, from Commander Spellbook — see
//! [`crate::spellbook`]) the way legality reads the published legality object and tokens
//! read `all_parts`. The database is keyed by **oracle id**, so a piece matches whichever
//! printing the deck happens to hold, and the deck is folded per gameplay identity first
//! (the same identity the diff and the bracket count by, one level below the name).
//!
//! What "in the deck" means is decided **once, in [`classify`]**, from the deck's rows:
//!
//! * a piece is held when a card of its `oracle_id` is in the deck proper — maybeboards
//!   out (issue #570), sideboards **in**: a combo whose last piece sits in the sideboard is
//!   a card away in the deck a player registers, not the deck they cast from, and the
//!   command zone in, since a commander is the most reliable piece a deck has;
//! * a piece that **must be the commander** is held only when it sits in the command zone
//!   of a format that leads with one — the same two answers the facets and the mana base
//!   borrow (`rules::deck_zone`, `rules::format_leads_with_command_zone`); in a 60-card
//!   format nothing is ever the commander, so such a combo is always at least a card away;
//! * a piece wanting more copies than the deck holds is short by that piece;
//! * a **template** ("any free sacrifice outlet") is a Scryfall query this app can't run,
//!   so it always counts as one missing card, named — a combo with a template is never
//!   reported complete, because "you have it" would be a guess.
//!
//! **One card short** is the builder's list, and it is filtered to what the deck could
//! actually add: in a format that leads with a command zone the combo's colour identity
//! must fit the commander's (Commander Spellbook splits these as "almost included" vs
//! "by adding colours"; the second is not offered here, since a deck can't change its
//! commander's colours by adding a card). Both lists are capped and their counts exact —
//! a deck of staples is one card from thousands of combos.
//!
//! **No data is not "no combos."** A self-host that hasn't synced the dataset (or opted
//! out) has an empty table, and answering "this deck has no combos" from it would be a
//! confident wrong answer — the same stance `tokens` takes on its NULL column. The
//! response says whether any combo data is present, and the panel words the empty state
//! from that.

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;

use crate::entities::prelude::{Combo, ComboPiece};
use crate::entities::{combo, combo_piece};
use crate::error::AppError;
use crate::handlers::shared::combos::{
    COMBO_CHUNK, ComboSummary, load_combos, representative_printings,
};
use crate::spellbook;
use crate::state::AppState;

use super::DeckAnalysisInput;
use super::rules::{command_zone_section_ids, format_leads_with_command_zone};

/// How many complete combos ride the response; `combo_count` stays exact.
const MAX_LISTED_COMBOS: usize = 100;
/// How many one-card-short combos ride the response; `almost_count` stays exact.
const MAX_LISTED_ALMOST: usize = 50;

// ---------- Wire types ----------

/// One piece of a combo, against this deck.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckComboPiece {
    /// The card's gameplay identity (Scryfall `oracle_id`).
    pub oracle_id: String,
    pub name: String,
    /// Copies the combo needs.
    pub quantity: i32,
    /// Whether it has to be in the command zone.
    pub must_be_commander: bool,
    /// The printing to link to: the deck's own (the lowest-sorting one it is held under)
    /// when the deck holds the card, else the catalog's newest; `null` when the catalog
    /// holds none.
    pub card_id: Option<String>,
    /// Whether the deck proper holds the card at all (in any zone, in any number).
    pub in_deck: bool,
}

/// Why a combo isn't complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckComboMissingKind {
    /// A card the deck doesn't hold, or doesn't hold enough copies of.
    Card,
    /// A wildcard requirement this app can't evaluate.
    Template,
    /// A card the deck holds that isn't in its command zone but has to be.
    Commander,
}

/// One thing a combo still needs.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckComboMissing {
    /// The card's or template's name.
    pub name: String,
    pub kind: DeckComboMissingKind,
    /// For a card: a printing to link to (the catalog's newest, or the deck's own for a
    /// `commander` miss); `null` for a template or an unheld card the catalog lacks.
    pub card_id: Option<String>,
}

/// One combo against this deck.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckCombo {
    #[serde(flatten)]
    #[cfg_attr(test, ts(flatten))]
    pub summary: ComboSummary,
    /// Every piece, in the combo's own order.
    pub pieces: Vec<DeckComboPiece>,
    /// What the deck still needs — empty for a combo it can assemble.
    pub missing: Vec<DeckComboMissing>,
}

/// Everything a deck's combo read says.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckCombos {
    /// Combos the deck proper can assemble: fewest pieces first, then most-played. Capped
    /// at 100; `combo_count` is exact.
    pub combos: Vec<DeckCombo>,
    pub combo_count: i64,
    /// Combos exactly one card (or one template, or one commander swap) away, most-played
    /// first, within the deck's colour identity where its format has one. Capped at 50;
    /// `almost_count` is exact.
    pub almost: Vec<DeckCombo>,
    pub almost_count: i64,
    /// Whether any combo data is present at all. `false` means the dataset hasn't been
    /// synced (or was switched off) — an empty `combos` is then "unknown", never "none".
    pub available: bool,
    /// Where the data comes from, for the attribution the source asks for.
    pub source: String,
    pub source_url: String,
}

// ---------- The fold ----------

/// One gameplay identity as the deck proper holds it.
#[derive(Debug)]
struct Held {
    /// The lowest-sorting external id it is held under, for links.
    card_id: String,
    copies: i64,
    in_command_zone: bool,
}

/// Fold the deck proper by oracle id. A card without one (a token) can't be a piece.
fn fold_held(input: &DeckAnalysisInput, command_zone: &HashSet<i32>) -> HashMap<String, Held> {
    let mut held: HashMap<String, Held> = HashMap::new();
    for entry in input.deck_proper() {
        let copies = entry.copies();
        if copies == 0 {
            continue;
        }
        let Some(oracle_id) = entry.facts.oracle_id.as_deref() else {
            continue;
        };
        let in_zone = command_zone.contains(&entry.section_id);
        match held.get_mut(oracle_id) {
            Some(h) => {
                h.copies += copies;
                h.in_command_zone |= in_zone;
                if entry.facts.id < h.card_id {
                    h.card_id = entry.facts.id.clone();
                }
            }
            None => {
                held.insert(
                    oracle_id.to_string(),
                    Held {
                        card_id: entry.facts.id.clone(),
                        copies,
                        in_command_zone: in_zone,
                    },
                );
            }
        }
    }
    held
}

/// One row of the piece scan: `(combo_id, oracle_id, quantity, must_be_commander,
/// piece_count, template_count, popularity, color_identity)` — the piece's own columns,
/// then its parent's.
type HitRow = (i32, String, i32, bool, i32, i32, i32, String);

/// One matched piece row, as the scan returns it: the piece and its parent's facts.
struct Hit {
    combo_id: i32,
    oracle_id: String,
    quantity: i32,
    must_be_commander: bool,
    piece_count: i32,
    template_count: i32,
    popularity: i32,
    color_identity: String,
}

/// A combo the deck touches, after the scan: how far from complete it is.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    combo_id: i32,
    /// Pieces the deck fails on (missing, short, or not the commander) plus templates.
    short_by: i32,
    piece_count: i32,
    popularity: i32,
    identity: BTreeSet<String>,
}

/// Judge every touched combo from the piece scan alone: per combo, the pieces the deck
/// holds are the rows returned, so `piece_count - held` are the cards it lacks, plus the
/// held pieces that fail their own check, plus every template.
fn classify(hits: &[Hit], held: &HashMap<String, Held>) -> Vec<Candidate> {
    struct Acc {
        piece_count: i32,
        template_count: i32,
        popularity: i32,
        color_identity: String,
        /// Distinct held pieces seen (a combo lists each card once).
        held_pieces: HashSet<String>,
        /// Held pieces that still fail: too few copies, or not the commander.
        failed: i32,
    }
    let mut by_combo: HashMap<i32, Acc> = HashMap::new();
    for hit in hits {
        let acc = by_combo.entry(hit.combo_id).or_insert_with(|| Acc {
            piece_count: hit.piece_count,
            template_count: hit.template_count,
            popularity: hit.popularity,
            color_identity: hit.color_identity.clone(),
            held_pieces: HashSet::new(),
            failed: 0,
        });
        if !acc.held_pieces.insert(hit.oracle_id.clone()) {
            continue;
        }
        let Some(h) = held.get(&hit.oracle_id) else {
            continue;
        };
        if h.copies < i64::from(hit.quantity) || (hit.must_be_commander && !h.in_command_zone) {
            acc.failed += 1;
        }
    }
    by_combo
        .into_iter()
        .map(|(combo_id, acc)| Candidate {
            combo_id,
            short_by: (acc.piece_count - i32::try_from(acc.held_pieces.len()).unwrap_or(i32::MAX))
                .max(0)
                + acc.failed
                + acc.template_count,
            piece_count: acc.piece_count,
            popularity: acc.popularity,
            identity: spellbook::model::split_list(&acc.color_identity)
                .into_iter()
                .collect(),
        })
        .collect()
}

/// The colour identity the deck plays in, when its format leads with a command zone and
/// the zone holds something: the zone's colours. `None` is "no constraint to apply".
fn deck_identity(
    input: &DeckAnalysisInput,
    command_zone: &HashSet<i32>,
    leads: bool,
) -> Option<BTreeSet<String>> {
    if !leads {
        return None;
    }
    let mut letters: BTreeSet<String> = BTreeSet::new();
    let mut any = false;
    for entry in input.in_sections(&command_zone.iter().copied().collect::<Vec<_>>()) {
        if entry.copies() == 0 {
            continue;
        }
        any = true;
        letters.extend(entry.facts.color_identity.iter().cloned());
    }
    any.then_some(letters)
}

/// Build one combo's wire shape against the deck.
fn dress(
    model: &combo::Model,
    pieces: &[combo_piece::Model],
    held: &HashMap<String, Held>,
    printings: &HashMap<String, String>,
) -> DeckCombo {
    let summary = ComboSummary::from_model(model);
    let mut missing: Vec<DeckComboMissing> = Vec::new();
    let mut out: Vec<DeckComboPiece> = Vec::with_capacity(pieces.len());
    for p in pieces {
        let h = held.get(&p.oracle_id);
        let card_id = h
            .map(|h| h.card_id.clone())
            .or_else(|| printings.get(&p.oracle_id).cloned());
        match h {
            None => missing.push(DeckComboMissing {
                name: p.name.clone(),
                kind: DeckComboMissingKind::Card,
                card_id: card_id.clone(),
            }),
            Some(h) if h.copies < i64::from(p.quantity) => missing.push(DeckComboMissing {
                name: p.name.clone(),
                kind: DeckComboMissingKind::Card,
                card_id: card_id.clone(),
            }),
            Some(h) if p.must_be_commander && !h.in_command_zone => {
                missing.push(DeckComboMissing {
                    name: p.name.clone(),
                    kind: DeckComboMissingKind::Commander,
                    card_id: card_id.clone(),
                })
            }
            Some(_) => {}
        }
        out.push(DeckComboPiece {
            oracle_id: p.oracle_id.clone(),
            name: p.name.clone(),
            quantity: p.quantity,
            must_be_commander: p.must_be_commander,
            card_id,
            in_deck: h.is_some(),
        });
    }
    for template in &summary.templates {
        missing.push(DeckComboMissing {
            name: template.clone(),
            kind: DeckComboMissingKind::Template,
            card_id: None,
        });
    }
    DeckCombo {
        summary,
        pieces: out,
        missing,
    }
}

/// The combos a deck can assemble, and the ones it is a card away from.
pub(crate) async fn analyse_combos(
    state: &AppState,
    game: &str,
    format: Option<&str>,
    input: &DeckAnalysisInput,
) -> Result<DeckCombos, AppError> {
    let mut response = DeckCombos {
        combos: Vec::new(),
        combo_count: 0,
        almost: Vec::new(),
        almost_count: 0,
        available: false,
        source: spellbook::ATTRIBUTION.to_string(),
        source_url: spellbook::SITE_URL.to_string(),
    };
    // "Any data at all?" — one indexed probe, so an unsynced self-host's panel can say
    // "no combo data" rather than "no combos".
    response.available = Combo::find()
        .filter(combo::Column::Game.eq(game))
        .select_only()
        .column(combo::Column::Id)
        .limit(1)
        .into_tuple::<i32>()
        .one(&state.db)
        .await?
        .is_some();
    if !response.available {
        return Ok(response);
    }

    let leads = format_leads_with_command_zone(format);
    let command_zone: HashSet<i32> = if leads {
        command_zone_section_ids(&input.sections)
            .into_iter()
            .collect()
    } else {
        HashSet::new()
    };
    let held = fold_held(input, &command_zone);
    if held.is_empty() {
        return Ok(response);
    }
    let identity = deck_identity(input, &command_zone, leads);

    // The one scan: every piece row for a held identity, joined to its parent for the
    // combo's size + popularity + colours. Chunked on the deck's own ids, which are
    // caller-controlled.
    let oracle_ids: Vec<&str> = held.keys().map(String::as_str).collect();
    let mut hits: Vec<Hit> = Vec::new();
    for chunk in oracle_ids.chunks(COMBO_CHUNK) {
        let rows: Vec<HitRow> = ComboPiece::find()
            .select_only()
            .column(combo_piece::Column::ComboId)
            .column(combo_piece::Column::OracleId)
            .column(combo_piece::Column::Quantity)
            .column(combo_piece::Column::MustBeCommander)
            .column(combo::Column::PieceCount)
            .column(combo::Column::TemplateCount)
            .column(combo::Column::Popularity)
            .column(combo::Column::ColorIdentity)
            .inner_join(Combo)
            .filter(combo_piece::Column::Game.eq(game))
            .filter(combo_piece::Column::OracleId.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        hits.extend(rows.into_iter().map(
            |(
                combo_id,
                oracle_id,
                quantity,
                must_be_commander,
                piece_count,
                template_count,
                popularity,
                color_identity,
            )| Hit {
                combo_id,
                oracle_id,
                quantity,
                must_be_commander,
                piece_count,
                template_count,
                popularity,
                color_identity,
            },
        ));
    }

    let candidates = classify(&hits, &held);
    let mut complete: Vec<&Candidate> = candidates.iter().filter(|c| c.short_by == 0).collect();
    let mut almost: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.short_by == 1)
        .filter(|c| {
            identity
                .as_ref()
                .is_none_or(|deck| c.identity.is_subset(deck))
        })
        .collect();
    // Fewest pieces first (a two-card combo is the headline), then most-played, then a
    // stable id so a precon and its copy answer identically.
    complete.sort_by(|a, b| {
        a.piece_count
            .cmp(&b.piece_count)
            .then(b.popularity.cmp(&a.popularity))
            .then(a.combo_id.cmp(&b.combo_id))
    });
    almost.sort_by(|a, b| {
        b.popularity
            .cmp(&a.popularity)
            .then(a.piece_count.cmp(&b.piece_count))
            .then(a.combo_id.cmp(&b.combo_id))
    });
    response.combo_count = i64::try_from(complete.len()).unwrap_or(i64::MAX);
    response.almost_count = i64::try_from(almost.len()).unwrap_or(i64::MAX);

    let complete_ids: Vec<i32> = complete
        .iter()
        .take(MAX_LISTED_COMBOS)
        .map(|c| c.combo_id)
        .collect();
    let almost_ids: Vec<i32> = almost
        .iter()
        .take(MAX_LISTED_ALMOST)
        .map(|c| c.combo_id)
        .collect();
    let mut wanted = complete_ids.clone();
    wanted.extend(almost_ids.iter().copied());
    let rows = load_combos(&state.db, game, &wanted).await?;
    let by_id: HashMap<i32, &(combo::Model, Vec<combo_piece::Model>)> =
        rows.iter().map(|row| (row.0.id, row)).collect();

    // Printings for the pieces the deck lacks (held ones link to the deck's own).
    let unheld: Vec<String> = rows
        .iter()
        .flat_map(|(_, pieces)| pieces.iter())
        .filter(|p| !held.contains_key(&p.oracle_id))
        .map(|p| p.oracle_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let printings = representative_printings(&state.db, game, &unheld).await?;

    response.combos = complete_ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|(model, pieces)| dress(model, pieces, &held, &printings))
        .collect();
    response.almost = almost_ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|(model, pieces)| dress(model, pieces, &held, &printings))
        .collect();
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{deck, entry, section};

    fn hit(combo_id: i32, oracle: &str, quantity: i32, commander: bool, size: i32) -> Hit {
        Hit {
            combo_id,
            oracle_id: oracle.to_string(),
            quantity,
            must_be_commander: commander,
            piece_count: size,
            template_count: 0,
            popularity: 1,
            color_identity: "W,U".to_string(),
        }
    }

    fn held(oracle: &str, copies: i64, in_zone: bool) -> (String, Held) {
        (
            oracle.to_string(),
            Held {
                card_id: format!("{oracle}-print"),
                copies,
                in_command_zone: in_zone,
            },
        )
    }

    fn short_by(candidates: &[Candidate], id: i32) -> i32 {
        candidates
            .iter()
            .find(|c| c.combo_id == id)
            .expect("candidate")
            .short_by
    }

    #[test]
    fn a_combo_is_complete_when_every_piece_is_held_and_short_by_the_rest() {
        let held: HashMap<String, Held> = [held("a", 1, false), held("b", 1, false)].into();
        // Combo 1 is a + b; combo 2 is a + b + c (c not held); combo 3 is a + d + e.
        let hits = vec![
            hit(1, "a", 1, false, 2),
            hit(1, "b", 1, false, 2),
            hit(2, "a", 1, false, 3),
            hit(2, "b", 1, false, 3),
            hit(3, "a", 1, false, 3),
        ];
        let candidates = classify(&hits, &held);
        assert_eq!(short_by(&candidates, 1), 0);
        assert_eq!(short_by(&candidates, 2), 1);
        assert_eq!(short_by(&candidates, 3), 2);
    }

    #[test]
    fn a_template_is_always_one_missing_card() {
        let held: HashMap<String, Held> = [held("a", 1, false)].into();
        let mut with_template = hit(1, "a", 1, false, 1);
        with_template.template_count = 1;
        let candidates = classify(&[with_template], &held);
        assert_eq!(
            short_by(&candidates, 1),
            1,
            "held piece + one template = one short"
        );
    }

    #[test]
    fn a_commander_piece_counts_only_from_the_command_zone_and_copies_count() {
        let held: HashMap<String, Held> =
            [held("a", 1, false), held("b", 1, true), held("r", 3, false)].into();
        let hits = vec![
            // Combo 1 needs `a` as commander — held, but in the 99.
            hit(1, "a", 1, true, 2),
            hit(1, "b", 1, false, 2),
            // Combo 2 needs `b` as commander — it's in the zone.
            hit(2, "b", 1, true, 2),
            hit(2, "a", 1, false, 2),
            // Combo 3 needs four copies of `r`; the deck runs three.
            hit(3, "r", 4, false, 1),
        ];
        let candidates = classify(&hits, &held);
        assert_eq!(short_by(&candidates, 1), 1);
        assert_eq!(short_by(&candidates, 2), 0);
        assert_eq!(short_by(&candidates, 3), 1);
    }

    #[test]
    fn the_deck_folds_by_oracle_id_across_printings_and_zones() {
        let sections = vec![
            section(1, "Commander", false),
            section(2, "Main", false),
            section(3, "Maybe", true),
        ];
        let mut a1 = entry("print-2", "Alpha", 2, 2, 0);
        a1.facts.oracle_id = Some("oa".into());
        let mut a2 = entry("print-1", "Alpha", 2, 0, 1);
        a2.facts.oracle_id = Some("oa".into());
        let mut cmd = entry("print-9", "Boss", 1, 1, 0);
        cmd.facts.oracle_id = Some("ob".into());
        let mut maybe = entry("print-5", "Maybe", 3, 1, 0);
        maybe.facts.oracle_id = Some("om".into());
        let no_oracle = entry("print-7", "Token", 2, 1, 0);
        let input = deck(sections, vec![a1, a2, cmd, maybe, no_oracle]);
        let zone: HashSet<i32> = [1].into();
        let held = fold_held(&input, &zone);
        assert_eq!(held.len(), 2, "maybeboard + oracle-less rows are out");
        let alpha = &held["oa"];
        assert_eq!(alpha.copies, 3, "both printings, both finishes");
        assert_eq!(
            alpha.card_id, "print-1",
            "the lowest-sorting printing links"
        );
        assert!(!alpha.in_command_zone);
        assert!(held["ob"].in_command_zone);
    }

    #[test]
    fn the_deck_identity_is_the_command_zones_when_the_format_leads_with_one() {
        let sections = vec![section(1, "Commander", false), section(2, "Main", false)];
        let mut cmd = entry("c", "Boss", 1, 1, 0);
        cmd.facts.color_identity = vec!["W".into(), "B".into()];
        let mut spell = entry("s", "Spell", 2, 1, 0);
        spell.facts.color_identity = vec!["G".into()];
        let input = deck(sections, vec![cmd, spell]);
        let zone: HashSet<i32> = [1].into();
        let identity = deck_identity(&input, &zone, true).expect("an identity");
        assert_eq!(identity, ["B", "W"].into_iter().map(String::from).collect());
        // No command zone lead: no constraint. An empty zone: none either.
        assert!(deck_identity(&input, &zone, false).is_none());
        assert!(deck_identity(&input, &HashSet::new(), true).is_none());
    }
}
