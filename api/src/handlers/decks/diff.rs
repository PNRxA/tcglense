//! **Diff two decks** (issue #674): what changed between a deck and another of the caller's
//! — the question that follows "make a v2" the moment there *is* a v2.
//!
//! The comparison is a **pure fold over two [`DeckDetail`]s** ([`diff_decks`]): the handler
//! only proves both decks are the caller's and loads them through the same `deck_detail`
//! read the deck page uses, so the diff can never disagree with the two pages it sits
//! between. Three stances are load-bearing:
//!
//! * **It folds by card, never by row.** A `deck_card` row addresses one *printing* in one
//!   section, and a playset is routinely split across rows — two arts of the same card, or
//!   regular and foil copies of one printing (which are one row's two counts). Folded per row,
//!   swapping one art for another would read as a card removed and a card added, and a foil
//!   upgrade as a change in *what* the deck plays. So the unit here is the card **name**, the
//!   identity `analysis::fold_by_name` counts by for the bracket and the mana base (the precon
//!   copy's `push_folded` is a *different* fold — by printing id within a section, for the
//!   `(deck_id, card_id, section_id)` unique constraint — and would keep a printing swap
//!   visible), and the copies are summed across every printing and both finishes. A printing
//!   swap therefore folds away on purpose: the deck plays the same card. It is its own fold
//!   rather than a call into `analysis::fold_by_name` because that one folds `AnalysisEntry`s
//!   (catalog facts, copies only) and this one needs the wire `Card` and the foil split.
//! * **Finish changes are reported separately.** A card whose copies are unchanged but whose
//!   foil split moved is a real edit — the one a "bling the deck" pass makes — and it is
//!   emitted as [`DeckDiffChange::Finish`] rather than either being hidden or being counted
//!   among the cards that changed. Both foil counts always ride the entry, so a client can
//!   also see the finish side of a count change.
//! * **Sections are matched by name** (a section's identity across two decks — ids are
//!   per-deck), in the base deck's display order with the other deck's extra sections after,
//!   and only a section with something to report is listed. A card moved between sections
//!   shows in both — removed from one, added to the other — which is exactly what happened
//!   there; the deck-wide `cards` fold, over the deck proper, is where such a move nets to
//!   nothing, so the two answer different questions and neither is wrong.
//!
//! Maybeboards are in the per-section list (flagged, as the deck page shows them) and out of
//! the deck-wide fold and its `summary`, the same "what is this deck" line every other reader
//! draws (issue #570).

use std::collections::HashMap;

use axum::{Json, extract::State};
use serde::Serialize;

use crate::auth::extractor::AuthUser;
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{CardResponse, require_game};
use crate::state::AppState;

use super::{DeckCardEntry, DeckDetail, deck_detail, load_deck};

// ---------- Wire types ----------

/// How one card differs between the two decks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum DeckDiffChange {
    /// In the other deck only.
    Added,
    /// In the base deck only.
    Removed,
    /// In both, with a different number of copies.
    Changed,
    /// In both with the same number of copies, but a different regular/foil split.
    Finish,
}

/// One card that differs, folded across every printing of it and both finishes.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDiffEntry {
    /// A printing of the card, for the tile and the link: the first one the **base** deck
    /// holds it under, or the other deck's first when the base doesn't hold it at all.
    pub card: CardResponse,
    /// The fold key — the card's name, shared by every printing of it.
    pub name: String,
    pub change: DeckDiffChange,
    /// Copies in the base deck: regular + foil, every printing. `0` when added.
    pub base_quantity: i64,
    /// Copies in the other deck, on the same grain. `0` when removed.
    pub other_quantity: i64,
    /// `other_quantity - base_quantity`: positive when the other deck plays more.
    pub delta: i64,
    /// Foil copies in the base deck, every printing.
    pub base_foil_quantity: i64,
    /// Foil copies in the other deck, every printing.
    pub other_foil_quantity: i64,
}

/// The differences within one section, matched between the two decks by **name**.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDiffSection {
    pub name: String,
    /// The base deck's section id, or `null` when only the other deck has this section.
    pub base_section_id: Option<i32>,
    /// The other deck's section id, or `null` when only the base deck has this section.
    pub other_section_id: Option<i32>,
    /// Whether the section sits outside the deck proper — the base's flag, else the other's.
    pub is_maybeboard: bool,
    /// The cards that differ here, added first, then removed, changed, finish; by name within
    /// each. Never empty — a section with nothing to report isn't listed.
    pub entries: Vec<DeckDiffEntry>,
    /// Cards held identically (same copies, same finishes) in this section of both decks.
    pub unchanged: i64,
}

/// One side of the comparison, named so a client can caption the two columns.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDiffSide {
    pub id: i32,
    pub name: String,
    pub format: Option<String>,
    /// Copies in the deck proper — the deck page's own `summary.total_cards`.
    pub total_cards: i64,
}

/// Counts of cards (names, not rows) per kind of change, over the deck-wide fold.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDiffSummary {
    pub added: i64,
    pub removed: i64,
    pub changed: i64,
    pub finish_changed: i64,
    pub unchanged: i64,
}

/// Everything two decks disagree on.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDiff {
    pub base: DeckDiffSide,
    pub other: DeckDiffSide,
    /// Card counts per kind of change, over `cards`.
    pub summary: DeckDiffSummary,
    /// The deck-wide fold over the **deck proper** (maybeboards excluded), section-agnostic:
    /// a card moved between sections nets to nothing here. Same ordering as a section's entries.
    pub cards: Vec<DeckDiffEntry>,
    /// The per-section view: the base deck's sections in display order, then any the other
    /// deck alone has, each listed only when something in it differs.
    pub sections: Vec<DeckDiffSection>,
}

// ---------- The fold ----------

/// One card name's copies on one side, folded across printings and finishes.
struct Held<'a> {
    /// The first printing seen, for the entry's `card`.
    card: &'a CardResponse,
    copies: i64,
    foil: i64,
}

/// Fold a slice of deck-card entries by card name, in first-seen order — the same identity
/// `analysis::fold_by_name` counts by. Rows holding no copies are skipped.
fn fold_by_name<'a>(
    entries: impl IntoIterator<Item = &'a DeckCardEntry>,
) -> Vec<(String, Held<'a>)> {
    let mut folds: Vec<(String, Held<'a>)> = Vec::new();
    let mut index_by_name: HashMap<&str, usize> = HashMap::new();
    for entry in entries {
        let copies = i64::from(entry.quantity).saturating_add(i64::from(entry.foil_quantity));
        if copies <= 0 {
            continue;
        }
        let foil = i64::from(entry.foil_quantity).max(0);
        match index_by_name.get(entry.card.name.as_str()) {
            Some(&index) => {
                folds[index].1.copies += copies;
                folds[index].1.foil += foil;
            }
            None => {
                index_by_name.insert(entry.card.name.as_str(), folds.len());
                folds.push((
                    entry.card.name.clone(),
                    Held {
                        card: &entry.card,
                        copies,
                        foil,
                    },
                ));
            }
        }
    }
    folds
}

/// Compare two folded sides: the entries that differ (sorted), and how many cards matched
/// exactly. The base's first-seen order seeds the walk so the output is deterministic before
/// the sort, and a name only the other deck holds is walked afterwards.
fn compare<'a>(
    base: Vec<(String, Held<'a>)>,
    other: Vec<(String, Held<'a>)>,
) -> (Vec<DeckDiffEntry>, i64) {
    let mut other_by_name: HashMap<String, Held<'a>> = other.into_iter().collect();
    let mut entries = Vec::new();
    let mut unchanged = 0i64;
    for (name, held) in base {
        match other_by_name.remove(&name) {
            Some(theirs) => {
                let change = if held.copies != theirs.copies {
                    DeckDiffChange::Changed
                } else if held.foil != theirs.foil {
                    DeckDiffChange::Finish
                } else {
                    unchanged += 1;
                    continue;
                };
                entries.push(DeckDiffEntry {
                    card: held.card.clone(),
                    name,
                    change,
                    base_quantity: held.copies,
                    other_quantity: theirs.copies,
                    delta: theirs.copies - held.copies,
                    base_foil_quantity: held.foil,
                    other_foil_quantity: theirs.foil,
                });
            }
            None => entries.push(DeckDiffEntry {
                card: held.card.clone(),
                name,
                change: DeckDiffChange::Removed,
                base_quantity: held.copies,
                other_quantity: 0,
                delta: -held.copies,
                base_foil_quantity: held.foil,
                other_foil_quantity: 0,
            }),
        }
    }
    for (name, theirs) in other_by_name {
        entries.push(DeckDiffEntry {
            card: theirs.card.clone(),
            name,
            change: DeckDiffChange::Added,
            base_quantity: 0,
            other_quantity: theirs.copies,
            delta: theirs.copies,
            base_foil_quantity: 0,
            other_foil_quantity: theirs.foil,
        });
    }
    entries.sort_by(|a, b| a.change.cmp(&b.change).then_with(|| a.name.cmp(&b.name)));
    (entries, unchanged)
}

/// Tally a deck-wide entry list into the summary counts.
fn summarize(entries: &[DeckDiffEntry], unchanged: i64) -> DeckDiffSummary {
    let mut summary = DeckDiffSummary {
        unchanged,
        ..DeckDiffSummary::default()
    };
    for entry in entries {
        match entry.change {
            DeckDiffChange::Added => summary.added += 1,
            DeckDiffChange::Removed => summary.removed += 1,
            DeckDiffChange::Changed => summary.changed += 1,
            DeckDiffChange::Finish => summary.finish_changed += 1,
        }
    }
    summary
}

/// The cards of `deck` outside its maybeboard sections — the deck proper.
fn deck_proper(deck: &DeckDetail) -> impl Iterator<Item = &DeckCardEntry> {
    let maybeboards: Vec<i32> = deck
        .sections
        .iter()
        .filter(|section| section.is_maybeboard)
        .map(|section| section.id)
        .collect();
    deck.cards
        .iter()
        .filter(move |entry| !maybeboards.contains(&entry.section_id))
}

/// Diff `base` against `other`: a pure function of the two details (see the module doc).
pub(crate) fn diff_decks(base: &DeckDetail, other: &DeckDetail) -> DeckDiff {
    // Deck-wide, over the deck proper of each.
    let (cards, unchanged) = compare(
        fold_by_name(deck_proper(base)),
        fold_by_name(deck_proper(other)),
    );
    let summary = summarize(&cards, unchanged);

    // Per section, matched by name. The other deck's sections are indexed by name once;
    // the base's display order leads, then whatever the other deck alone has, in its order.
    let mut other_sections: HashMap<&str, &super::DeckSectionResponse> = other
        .sections
        .iter()
        .map(|section| (section.name.as_str(), section))
        .collect();
    let mut sections = Vec::new();
    for section in &base.sections {
        let theirs = other_sections.remove(section.name.as_str());
        let base_fold = fold_by_name(base.cards.iter().filter(|e| e.section_id == section.id));
        let other_fold = match theirs {
            Some(t) => fold_by_name(other.cards.iter().filter(|e| e.section_id == t.id)),
            None => Vec::new(),
        };
        let (entries, unchanged) = compare(base_fold, other_fold);
        if entries.is_empty() {
            continue;
        }
        sections.push(DeckDiffSection {
            name: section.name.clone(),
            base_section_id: Some(section.id),
            other_section_id: theirs.map(|t| t.id),
            is_maybeboard: section.is_maybeboard,
            entries,
            unchanged,
        });
    }
    for section in &other.sections {
        // Only the sections the walk above didn't claim — those the base deck lacks.
        if !other_sections.contains_key(section.name.as_str()) {
            continue;
        }
        let other_fold = fold_by_name(other.cards.iter().filter(|e| e.section_id == section.id));
        let (entries, unchanged) = compare(Vec::new(), other_fold);
        if entries.is_empty() {
            continue;
        }
        sections.push(DeckDiffSection {
            name: section.name.clone(),
            base_section_id: None,
            other_section_id: Some(section.id),
            is_maybeboard: section.is_maybeboard,
            entries,
            unchanged,
        });
    }

    DeckDiff {
        base: side_of(base),
        other: side_of(other),
        summary,
        cards,
        sections,
    }
}

fn side_of(deck: &DeckDetail) -> DeckDiffSide {
    DeckDiffSide {
        id: deck.id,
        name: deck.name.clone(),
        format: deck.format.clone(),
        total_cards: deck.summary.total_cards,
    }
}

// ---------- The route ----------

/// Diff two decks
///
/// `GET /api/decks/{game}/{deck_id}/diff/{other_id}` -> what changed between two of the
/// caller's decks (issue #674): per card — added, removed, a different number of copies, or
/// only a different regular/foil split — over the deck proper (`cards`, with `summary`) and
/// per section matched by name (`sections`). Cards are folded by **name** across every
/// printing and both finishes, so a printing swap is not a change and a split playset is one
/// card. `404` when either deck isn't the caller's (never `403`); a deck diffed against
/// itself is an empty diff.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/diff/{other_id}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "The base deck (must be the caller's)"),
        ("other_id" = i32, Path, description = "The deck to compare it with (must be the caller's)"),
    ),
    responses(
        (status = 200, description = "The differences, deck-wide and per section.", body = DeckDiff),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or either deck is not the caller's."),
    ),
)]
pub async fn diff_deck(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id, other_id)): Path<(String, i32, i32)>,
) -> Result<Json<DeckDiff>, AppError> {
    require_game(&game)?;
    // Both sides are ownership-checked the same way: a foreign id on either is the uniform
    // 404, so a diff is no more of an existence oracle over deck ids than a read.
    let base = load_deck(&state, user.id, &game, deck_id).await?;
    let other = load_deck(&state, user.id, &game, other_id).await?;
    let handle = crate::auth::username::handle_of(&user);
    let base = deck_detail(&state, &base, handle.clone()).await?;
    let other = deck_detail(&state, &other, handle).await?;
    Ok(Json(diff_decks(&base, &other)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::DeckSectionResponse;
    use crate::handlers::shared::CollectionSummary;
    use crate::test_support::card_model;

    fn card(id: i32, name: &str) -> CardResponse {
        let mut model = card_model(id);
        model.name = name.to_string();
        CardResponse::from(model)
    }

    fn section(id: i32, name: &str, is_maybeboard: bool) -> DeckSectionResponse {
        DeckSectionResponse {
            id,
            name: name.to_string(),
            position: id,
            is_maybeboard,
        }
    }

    fn entry(card: CardResponse, section_id: i32, quantity: i32, foil: i32) -> DeckCardEntry {
        DeckCardEntry {
            card,
            section_id,
            quantity,
            foil_quantity: foil,
        }
    }

    fn summary(total: i64) -> CollectionSummary {
        CollectionSummary {
            unique_cards: 0,
            total_cards: total,
            total_value_usd: None,
            bulk_value_usd: None,
        }
    }

    fn deck(id: i32, sections: Vec<DeckSectionResponse>, cards: Vec<DeckCardEntry>) -> DeckDetail {
        let total = cards
            .iter()
            .map(|c| i64::from(c.quantity + c.foil_quantity))
            .sum();
        let ts = "2024-01-01T00:00:00Z".parse().unwrap();
        DeckDetail {
            id,
            game: "mtg".into(),
            name: format!("Deck {id}"),
            description: None,
            format: None,
            folder_id: None,
            is_public: false,
            handle: None,
            summary: summary(total),
            maybeboard_summary: summary(0),
            sections,
            cards,
            created_at: ts,
            updated_at: ts,
        }
    }

    /// The playset case the issue names: the base holds one printing of a card as 2
    /// regular + 2 foil (one row), the other holds two *arts* of it, 2 + 2 regular (two
    /// rows). Per row that is a removal and two additions; per card it is one card whose
    /// copies are unchanged and whose finish moved.
    #[test]
    fn a_split_playset_is_one_card_and_a_finish_change_not_three_rows() {
        let base = deck(
            1,
            vec![section(10, "Creatures", false)],
            vec![entry(card(100, "Llanowar Elves"), 10, 2, 2)],
        );
        let other = deck(
            2,
            vec![section(20, "Creatures", false)],
            vec![
                entry(card(100, "Llanowar Elves"), 20, 2, 0),
                entry(card(101, "Llanowar Elves"), 20, 2, 0),
            ],
        );
        let diff = diff_decks(&base, &other);

        assert_eq!(
            diff.cards.len(),
            1,
            "one card, not three rows: {:?}",
            diff.cards
        );
        let entry = &diff.cards[0];
        assert_eq!(entry.change, DeckDiffChange::Finish);
        assert_eq!(
            (entry.base_quantity, entry.other_quantity, entry.delta),
            (4, 4, 0)
        );
        assert_eq!(
            (entry.base_foil_quantity, entry.other_foil_quantity),
            (2, 0)
        );
        assert_eq!(
            entry.card.id, "ext-100",
            "the base's printing represents the card"
        );
        assert_eq!(
            diff.summary,
            DeckDiffSummary {
                finish_changed: 1,
                ..DeckDiffSummary::default()
            }
        );
        // The section view says the same thing, under the section matched by name.
        assert_eq!(diff.sections.len(), 1);
        assert_eq!(diff.sections[0].name, "Creatures");
        assert_eq!(diff.sections[0].base_section_id, Some(10));
        assert_eq!(diff.sections[0].other_section_id, Some(20));
        assert_eq!(diff.sections[0].entries.len(), 1);
        assert_eq!(diff.sections[0].entries[0].change, DeckDiffChange::Finish);
    }

    /// A pure printing swap — same card, same copies, same finishes, different art — is
    /// not a change at all: the deck plays the same card.
    #[test]
    fn a_printing_swap_folds_away() {
        let base = deck(
            1,
            vec![section(10, "Lands", false)],
            vec![entry(card(100, "Command Tower"), 10, 1, 0)],
        );
        let other = deck(
            2,
            vec![section(20, "Lands", false)],
            vec![entry(card(101, "Command Tower"), 20, 1, 0)],
        );
        let diff = diff_decks(&base, &other);
        assert!(diff.cards.is_empty(), "{:?}", diff.cards);
        assert!(diff.sections.is_empty());
        assert_eq!(diff.summary.unchanged, 1);
    }

    #[test]
    fn added_removed_and_changed_are_told_apart_and_ordered() {
        let base = deck(
            1,
            vec![section(10, "Spells", false)],
            vec![
                entry(card(100, "Counterspell"), 10, 4, 0),
                entry(card(101, "Opt"), 10, 4, 0),
                entry(card(102, "Brainstorm"), 10, 1, 0),
            ],
        );
        let other = deck(
            2,
            vec![section(20, "Spells", false)],
            vec![
                entry(card(100, "Counterspell"), 20, 2, 1),
                entry(card(103, "Ponder"), 20, 4, 0),
                entry(card(102, "Brainstorm"), 20, 1, 0),
            ],
        );
        let diff = diff_decks(&base, &other);
        let kinds: Vec<(&str, DeckDiffChange, i64)> = diff
            .cards
            .iter()
            .map(|e| (e.name.as_str(), e.change, e.delta))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("Ponder", DeckDiffChange::Added, 4),
                ("Opt", DeckDiffChange::Removed, -4),
                ("Counterspell", DeckDiffChange::Changed, -1),
            ]
        );
        assert_eq!(
            diff.summary,
            DeckDiffSummary {
                added: 1,
                removed: 1,
                changed: 1,
                finish_changed: 0,
                unchanged: 1,
            }
        );
        assert_eq!(diff.base.total_cards, 9);
        assert_eq!(diff.other.total_cards, 8);
    }

    /// A card moved between sections is a removal in one and an addition in the other per
    /// section — what happened there — and nothing deck-wide, because the deck still plays it.
    #[test]
    fn a_section_move_shows_per_section_and_nets_out_deck_wide() {
        let base = deck(
            1,
            vec![section(10, "Ramp", false), section(11, "Artifacts", false)],
            vec![entry(card(100, "Sol Ring"), 10, 1, 0)],
        );
        let other = deck(
            2,
            vec![section(20, "Ramp", false), section(21, "Artifacts", false)],
            vec![entry(card(100, "Sol Ring"), 21, 1, 0)],
        );
        let diff = diff_decks(&base, &other);
        assert!(diff.cards.is_empty());
        assert_eq!(diff.summary.unchanged, 1);
        let by_section: Vec<(&str, DeckDiffChange)> = diff
            .sections
            .iter()
            .map(|s| (s.name.as_str(), s.entries[0].change))
            .collect();
        assert_eq!(
            by_section,
            vec![
                ("Ramp", DeckDiffChange::Removed),
                ("Artifacts", DeckDiffChange::Added)
            ]
        );
    }

    /// Maybeboards are listed per section (flagged) and kept out of the deck-wide fold —
    /// a card under consideration isn't a change to the deck. A section only the other
    /// deck has trails the base's order and carries no base id.
    #[test]
    fn maybeboards_are_flagged_per_section_and_excluded_deck_wide() {
        let base = deck(1, vec![section(10, "Main", false)], vec![]);
        let other = deck(
            2,
            vec![section(20, "Main", false), section(21, "Maybeboard", true)],
            vec![entry(card(100, "Rhystic Study"), 21, 1, 0)],
        );
        let diff = diff_decks(&base, &other);
        assert!(diff.cards.is_empty());
        assert_eq!(diff.summary, DeckDiffSummary::default());
        assert_eq!(diff.sections.len(), 1);
        let section = &diff.sections[0];
        assert_eq!(section.name, "Maybeboard");
        assert!(section.is_maybeboard);
        assert_eq!(section.base_section_id, None);
        assert_eq!(section.other_section_id, Some(21));
        assert_eq!(section.entries[0].change, DeckDiffChange::Added);
        assert_eq!(section.entries[0].card.id, "ext-100");
    }

    #[test]
    fn a_deck_against_itself_is_empty() {
        let d = deck(
            1,
            vec![section(10, "Main", false)],
            vec![entry(card(100, "Island"), 10, 20, 4)],
        );
        let diff = diff_decks(&d, &d);
        assert!(diff.cards.is_empty());
        assert!(diff.sections.is_empty());
        assert_eq!(diff.summary.unchanged, 1);
    }
}
