//! The deck's **estimated Commander bracket** — where a list sits on Wizards' 1–5 ladder
//! (Exhibition, Core, Upgraded, Optimized, cEDH), worked out from the cards themselves.
//!
//! The ladder is a conversation starter: two players compare brackets before a game so
//! neither is surprised. What makes it mechanisable is that brackets 1–3 are defined by
//! things a decklist *shows* — how many Game Changers it runs, whether it denies lands,
//! whether it takes extra turns — while bracket 4 is "no restrictions" and bracket 5 is a
//! statement about the metagame you're sitting down in. So this module answers the half a
//! list can answer, and says plainly which half that is.
//!
//! **The estimate is a floor.** It reports the lowest bracket the deck's cards don't rule
//! out; everything it can't see — a two-card infinite combo, whether extra turns get
//! chained, what you built the deck *for* — can only move a deck up. That is the same
//! stance [`super::rules`] takes on legality (a false "in breach" is worse than a miss),
//! and it is why the estimate never returns bracket 1 or bracket 5 on its own: both are
//! claims about intent, and no amount of card text settles them.
//!
//! The categories it counts live in [`signals`], one grammar over the card's own text per
//! category, and every matched card is handed back with the estimate — a number a player
//! can't audit is a number they won't trust.
//!
//! Scoped to **Commander**. The ladder is defined for that format and no other, so a deck
//! in any other format (or none) gets `None` — "nothing to say", exactly as an untracked
//! format does for legality.

use serde::Serialize;

use super::{AnalysisEntry, CardFacts, DeckAnalysisInput};

mod signals;

use signals::{is_extra_turn, is_game_changer, is_mass_land_denial, is_tutor};

/// The one format the bracket ladder is defined for, as [`super::formats`] keys it.
const BRACKET_FORMAT_KEY: &str = "commander";

/// Game Changers bracket 3 tolerates. Four or more is bracket 4 territory.
const BRACKET_THREE_GAME_CHANGERS: i64 = 3;

/// Cards listed per category. `count` stays exact — a deck's row count is caller-controlled
/// and this response isn't paginated, so the list is capped the way the deck list caps its
/// commanders.
const MAX_LISTED_CARDS: usize = 50;

/// Names spelled out in a reason sentence before it falls back to "and N more".
const MAX_NAMED_IN_REASON: usize = 3;

// ---------- The ladder ----------

/// One rung of the bracket ladder.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckBracketLevel {
    /// 1–5.
    pub bracket: i32,
    /// The bracket's name (`"Upgraded"`).
    pub label: String,
    /// One sentence on what playing at this bracket means.
    pub description: String,
}

/// The five brackets in order. Shipped **with every estimate** rather than mirrored
/// client-side: the panel that renders it doesn't exist until the response lands (unlike
/// the format select, which must draw without waiting on a request — see
/// `web/src/lib/legality.ts`), so a second copy would be a maintenance cost buying nothing.
const LADDER: &[(i32, &str, &str)] = &[
    (
        1,
        "Exhibition",
        "Ultra-casual. A theme, a gimmick, or a story — winning isn't the point.",
    ),
    (
        2,
        "Core",
        "Precon level. The deck has a plan and can win with it, but not quickly and not out of nowhere.",
    ),
    (
        3,
        "Upgraded",
        "Above precon. Stronger cards and a tighter plan, without early or repeatable kills.",
    ),
    (
        4,
        "Optimized",
        "High power. Anything legal, played to win as efficiently as the cards allow.",
    ),
    (
        5,
        "cEDH",
        "Tournament Commander. Built against a known metagame, where winning is the only goal.",
    ),
];

fn level(bracket: i32) -> (&'static str, &'static str) {
    LADDER
        .iter()
        .find(|(number, _, _)| *number == bracket)
        .map(|(_, label, description)| (*label, *description))
        .unwrap_or(("Unknown", ""))
}

// ---------- Categories ----------

/// A category of card the bracket ladder is written in terms of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckBracketSignal {
    /// On Wizards' Game Changers list (the catalog's `game_changer` flag).
    GameChanger,
    /// Destroys, bounces, or locks down everyone's lands.
    MassLandDenial,
    /// Takes (or hands out) an extra turn.
    ExtraTurn,
    /// Searches the library for something that isn't a land.
    Tutor,
}

/// The order categories are reported in: the two that decide a bracket first, then the two
/// that only inform it.
const SIGNALS: &[(DeckBracketSignal, &str, &str)] = &[
    (
        DeckBracketSignal::GameChanger,
        "Game Changers",
        "Wizards' list of the cards that most warp a game. Brackets 1 and 2 allow none, bracket 3 allows up to three, bracket 4 has no limit.",
    ),
    (
        DeckBracketSignal::MassLandDenial,
        "Mass land denial",
        "Destroying, bouncing, or locking down everyone's lands. Brackets 1 to 3 don't allow it at all.",
    ),
    (
        DeckBracketSignal::ExtraTurn,
        "Extra turns",
        "Bracket 1 allows none. Brackets 2 and 3 allow them as long as they aren't chained into one another.",
    ),
    (
        DeckBracketSignal::Tutor,
        "Tutors",
        "Library searches for something other than a land. Brackets 1 and 2 expect these to be sparse — guidance rather than a limit.",
    ),
];

fn matches_signal(signal: DeckBracketSignal, card: &CardFacts) -> bool {
    match signal {
        DeckBracketSignal::GameChanger => is_game_changer(card),
        DeckBracketSignal::MassLandDenial => is_mass_land_denial(card),
        DeckBracketSignal::ExtraTurn => is_extra_turn(card),
        DeckBracketSignal::Tutor => is_tutor(card),
    }
}

/// One card counted towards a category.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckBracketCard {
    /// External card id of one printing (for keys and links).
    pub card_id: String,
    pub name: String,
    /// Copies of that name across the deck proper (regular + foil, every section).
    pub quantity: i64,
}

/// What the deck holds in one category.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckBracketCategory {
    pub signal: DeckBracketSignal,
    pub label: String,
    /// What this category means for the ladder.
    pub description: String,
    /// Distinct card **names** in this category — a card held in two arts counts once.
    pub count: i64,
    /// Whether this category is what put the estimate where it is.
    pub decisive: bool,
    /// The matched cards in the deck's own order, capped (`count` stays exact).
    pub cards: Vec<DeckBracketCard>,
}

/// Everything `GET /api/decks/{game}/{deck_id}/bracket` answers.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckBracketEstimate {
    /// Always `"commander"` — the estimate is `None` for every other format.
    pub format_key: String,
    /// That key's display label.
    pub format_label: String,
    /// The lowest bracket the deck's cards don't rule out: 2, 3, or 4. Never 1 or 5 —
    /// both are claims about intent, which a decklist can't settle.
    pub bracket: i32,
    /// The estimated bracket's name.
    pub label: String,
    /// One sentence on what that bracket means.
    pub description: String,
    /// All five rungs, so a client can draw the ladder without its own copy of it.
    pub ladder: Vec<DeckBracketLevel>,
    /// Why the estimate landed where it did, most decisive first.
    pub reasons: Vec<String>,
    /// What the estimate could not see. Never empty — the floor is only meaningful
    /// alongside the reasons it might be too low.
    pub caveats: Vec<String>,
    /// Every category, in a stable order, whether or not the deck holds any.
    pub categories: Vec<DeckBracketCategory>,
    /// Whether the deck also clears the extra bar bracket 1 sets (no Game Changers, no
    /// mass land denial, no extra turns). Whether it *is* an Exhibition deck is still the
    /// builder's call.
    pub exhibition_possible: bool,
}

// ---------- Evaluation ----------

/// One card name folded across every section and printing it appears in — the same fold
/// the legality verdict does, so a card in two arts is one Game Changer rather than two.
struct NameFold<'a> {
    facts: &'a CardFacts,
    card_id: String,
    copies: i64,
}

fn fold_by_name<'a>(entries: &[&'a AnalysisEntry]) -> Vec<NameFold<'a>> {
    let mut folds: Vec<NameFold<'a>> = Vec::new();
    let mut index_by_name: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for entry in entries {
        let copies = entry.copies();
        if copies == 0 {
            continue;
        }
        match index_by_name.get(entry.facts.name.as_str()) {
            Some(&index) => folds[index].copies += copies,
            None => {
                index_by_name.insert(entry.facts.name.as_str(), folds.len());
                folds.push(NameFold {
                    facts: &entry.facts,
                    card_id: entry.facts.id.clone(),
                    copies,
                });
            }
        }
    }
    folds
}

/// "Armageddon", "Armageddon and Ravages of War", "A, B and 4 more".
fn name_list(cards: &[DeckBracketCard]) -> String {
    let named: Vec<&str> = cards
        .iter()
        .take(MAX_NAMED_IN_REASON)
        .map(|card| card.name.as_str())
        .collect();
    let remainder = cards.len().saturating_sub(named.len());
    let mut parts: Vec<String> = named.into_iter().map(str::to_string).collect();
    if remainder > 0 {
        parts.push(format!("{remainder} more"));
    }
    match parts.len() {
        0 => String::new(),
        1 => parts.remove(0),
        _ => {
            let last = parts.pop().unwrap_or_default();
            format!("{} and {last}", parts.join(", "))
        }
    }
}

fn plural(count: i64, one: &str, many: &str) -> String {
    if count == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// Estimate a Commander deck's bracket from its cards.
fn estimate_bracket(format_key: &str, input: &DeckAnalysisInput) -> DeckBracketEstimate {
    // The deck **proper**: a maybeboard card is under consideration, not played, so it is
    // no more part of the bracket than it is of the card count or the legality verdict
    // (issue #570). The command zone is in — a Game Changer commander is the clearest case
    // of one there is.
    let folds = fold_by_name(&input.deck_proper());

    let mut categories: Vec<DeckBracketCategory> = SIGNALS
        .iter()
        .map(|(signal, label, description)| {
            let matched: Vec<&NameFold> = folds
                .iter()
                .filter(|fold| matches_signal(*signal, fold.facts))
                .collect();
            DeckBracketCategory {
                signal: *signal,
                label: (*label).to_string(),
                description: (*description).to_string(),
                count: matched.len() as i64,
                decisive: false,
                cards: matched
                    .iter()
                    .take(MAX_LISTED_CARDS)
                    .map(|fold| DeckBracketCard {
                        card_id: fold.card_id.clone(),
                        name: fold.facts.name.clone(),
                        quantity: fold.copies,
                    })
                    .collect(),
            }
        })
        .collect();

    let count_of = |signal: DeckBracketSignal| {
        categories
            .iter()
            .find(|category| category.signal == signal)
            .map_or(0, |category| category.count)
    };
    let game_changers = count_of(DeckBracketSignal::GameChanger);
    let land_denial = count_of(DeckBracketSignal::MassLandDenial);
    let extra_turns = count_of(DeckBracketSignal::ExtraTurn);
    let tutors = count_of(DeckBracketSignal::Tutor);

    // The ladder, read from the bottom: bracket 2 is the floor a list can establish (1 is
    // intent), 3 tolerates up to three Game Changers, and 4 is where anything goes.
    let bracket = if land_denial > 0 || game_changers > BRACKET_THREE_GAME_CHANGERS {
        4
    } else if game_changers > 0 {
        3
    } else {
        2
    };

    for category in &mut categories {
        category.decisive = match category.signal {
            DeckBracketSignal::MassLandDenial => land_denial > 0,
            DeckBracketSignal::GameChanger => game_changers > 0,
            _ => false,
        };
    }
    let named = |signal: DeckBracketSignal| {
        categories
            .iter()
            .find(|category| category.signal == signal)
            .map(|category| name_list(&category.cards))
            .unwrap_or_default()
    };

    let mut reasons: Vec<String> = Vec::new();
    if land_denial > 0 {
        reasons.push(format!(
            "{land_denial} mass land denial {} — {}. Brackets 1 to 3 don't allow any.",
            plural(land_denial, "card", "cards"),
            named(DeckBracketSignal::MassLandDenial)
        ));
    }
    if game_changers > BRACKET_THREE_GAME_CHANGERS {
        reasons.push(format!(
            "{game_changers} Game Changers — {}. Bracket 3 allows at most three.",
            named(DeckBracketSignal::GameChanger)
        ));
    } else if game_changers > 0 {
        reasons.push(format!(
            "{game_changers} Game {} — {}. Brackets 1 and 2 allow none; bracket 3 allows up to three.",
            plural(game_changers, "Changer", "Changers"),
            named(DeckBracketSignal::GameChanger)
        ));
    }
    if reasons.is_empty() {
        reasons.push(
            "No Game Changers and no mass land denial — nothing in this list holds the deck \
             above bracket 2."
                .to_string(),
        );
    }

    let exhibition_possible = bracket == 2 && extra_turns == 0;

    let mut caveats: Vec<String> = vec![
        "Two-card infinite combos aren't detected. A deck that can assemble one is bracket 4 \
         — or bracket 3 if it can only do it late."
            .to_string(),
    ];
    if extra_turns > 0 {
        caveats.push(format!(
            "{extra_turns} extra-turn {} counted. Brackets 2 and 3 allow them; chaining them \
             into one another is bracket 4, which a list can't show.",
            plural(extra_turns, "card is", "cards are"),
        ));
    }
    if tutors > 0 {
        caveats.push(format!(
            "{tutors} {} counted. Brackets 1 and 2 expect tutors to be sparse — a judgement \
             call, not a limit.",
            plural(tutors, "tutor is", "tutors are"),
        ));
    }
    caveats.push(
        "Bracket 5 (cEDH) describes the metagame a deck is built for, not what's in it."
            .to_string(),
    );
    if exhibition_possible {
        caveats.push(
            "Nothing here rules out bracket 1 either — Exhibition is about how a deck was \
             built, not what it holds."
                .to_string(),
        );
    }

    let (label, description) = level(bracket);
    DeckBracketEstimate {
        format_key: format_key.to_string(),
        format_label: super::formats::format_label(format_key),
        bracket,
        label: label.to_string(),
        description: description.to_string(),
        ladder: LADDER
            .iter()
            .map(|(bracket, label, description)| DeckBracketLevel {
                bracket: *bracket,
                label: (*label).to_string(),
                description: (*description).to_string(),
            })
            .collect(),
        reasons,
        caveats,
        categories,
        exhibition_possible,
    }
}

/// Estimate a loaded deck's bracket, or `None` when its format isn't Commander — the one
/// format the ladder is defined for. `None` means "nothing to say", never "bracket 1".
pub(crate) fn analyse_bracket(
    format: Option<&str>,
    input: &DeckAnalysisInput,
) -> Option<DeckBracketEstimate> {
    let key = super::formats::normalize_format_key(format)?;
    (key == BRACKET_FORMAT_KEY).then(|| estimate_bracket(key, input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{deck, entry, section};

    const MAIN: i32 = 1;
    const COMMAND: i32 = 2;
    const MAYBE: i32 = 3;

    fn sections() -> Vec<crate::handlers::decks::DeckSectionResponse> {
        vec![
            section(MAIN, "Main", false),
            section(COMMAND, "Commander", false),
            section(MAYBE, "Maybeboard", true),
        ]
    }

    fn category(estimate: &DeckBracketEstimate, signal: DeckBracketSignal) -> &DeckBracketCategory {
        estimate
            .categories
            .iter()
            .find(|category| category.signal == signal)
            .expect("every category is always reported")
    }

    /// A Game Changer, by index, so a test can ask for four of them without naming four
    /// real cards.
    fn game_changer(index: i32) -> AnalysisEntry {
        entry(
            &format!("gc{index}"),
            &format!("Game Changer {index}"),
            MAIN,
            1,
            0,
        )
        .game_changer(true)
    }

    #[test]
    fn only_commander_decks_get_an_estimate() {
        let input = deck(sections(), vec![entry("a", "A", MAIN, 1, 0)]);
        assert!(analyse_bracket(None, &input).is_none());
        assert!(analyse_bracket(Some("Modern"), &input).is_none());
        assert!(analyse_bracket(Some("Pauper Commander"), &input).is_none());
        assert!(analyse_bracket(Some("Cube"), &input).is_none());
        // Every spelling the format normaliser accepts for Commander, cEDH included — a
        // deck labelled cEDH is still estimated from its cards.
        for spelling in ["Commander", "EDH", "edh", "cedh"] {
            let estimate = analyse_bracket(Some(spelling), &input)
                .unwrap_or_else(|| panic!("{spelling} should be estimated"));
            assert_eq!(estimate.format_key, "commander");
            assert_eq!(estimate.format_label, "Commander");
        }
    }

    #[test]
    fn a_clean_list_floors_at_core() {
        let input = deck(sections(), vec![entry("a", "Llanowar Elves", MAIN, 1, 0)]);
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 2);
        assert_eq!(estimate.label, "Core");
        assert!(estimate.exhibition_possible);
        assert!(estimate.reasons[0].contains("above bracket 2"));
        assert_eq!(estimate.ladder.len(), 5, "the whole ladder ships with it");
    }

    #[test]
    fn game_changers_lift_the_estimate_to_upgraded_then_optimized() {
        let three = deck(sections(), (1..=3).map(game_changer).collect::<Vec<_>>());
        let estimate = analyse_bracket(Some("Commander"), &three).unwrap();
        assert_eq!(estimate.bracket, 3);
        assert_eq!(estimate.label, "Upgraded");
        assert!(category(&estimate, DeckBracketSignal::GameChanger).decisive);
        assert!(
            !estimate.exhibition_possible,
            "a deck with Game Changers is not an Exhibition deck"
        );

        let four = deck(sections(), (1..=4).map(game_changer).collect::<Vec<_>>());
        let estimate = analyse_bracket(Some("Commander"), &four).unwrap();
        assert_eq!(estimate.bracket, 4);
        assert_eq!(estimate.label, "Optimized");
        assert!(estimate.reasons[0].contains("at most three"));
    }

    #[test]
    fn mass_land_denial_alone_reaches_optimized() {
        let input = deck(
            sections(),
            vec![entry("arm", "Armageddon", MAIN, 1, 0).oracle("Destroy all lands.")],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 4);
        assert!(category(&estimate, DeckBracketSignal::MassLandDenial).decisive);
        assert!(estimate.reasons[0].contains("Armageddon"));
    }

    /// Extra turns and tutors are reported but never move the number: bracket 2 and 3 both
    /// allow them, and whether they're *chained* is exactly what a list can't show.
    #[test]
    fn extra_turns_and_tutors_inform_without_deciding() {
        let input = deck(
            sections(),
            vec![
                entry("tw", "Time Warp", MAIN, 1, 0).oracle("Take an extra turn after this one."),
                entry("dt", "Demonic Tutor", MAIN, 1, 0)
                    .oracle("Search your library for a card, then shuffle and put that card into your hand."),
            ],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 2, "neither category decides a bracket");
        assert_eq!(category(&estimate, DeckBracketSignal::ExtraTurn).count, 1);
        assert_eq!(category(&estimate, DeckBracketSignal::Tutor).count, 1);
        assert!(!category(&estimate, DeckBracketSignal::ExtraTurn).decisive);
        assert!(
            !estimate.exhibition_possible,
            "bracket 1 allows no extra turns at all"
        );
        assert!(
            estimate.caveats.iter().any(|c| c.contains("chaining")),
            "the caveat has to say what wasn't checked"
        );
    }

    /// The command zone counts — a Game Changer commander is the clearest case of one.
    #[test]
    fn a_game_changer_commander_counts() {
        let input = deck(
            sections(),
            vec![entry("cmd", "Thrasios, Triton Hero", COMMAND, 1, 0).game_changer(true)],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 3);
        assert_eq!(
            category(&estimate, DeckBracketSignal::GameChanger).cards[0].name,
            "Thrasios, Triton Hero"
        );
    }

    /// A maybeboard card is being considered, not played — the same split every other
    /// "what is this deck" reader makes (issue #570).
    #[test]
    fn a_maybeboard_card_is_not_part_of_the_estimate() {
        let input = deck(
            sections(),
            vec![
                entry("arm", "Armageddon", MAYBE, 1, 0).oracle("Destroy all lands."),
                entry("gc", "Rhystic Study", MAYBE, 1, 0).game_changer(true),
            ],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 2);
        assert_eq!(category(&estimate, DeckBracketSignal::GameChanger).count, 0);
    }

    /// Two printings of one Game Changer are one Game Changer, with both copies counted —
    /// the same fold the legality verdict does.
    #[test]
    fn printings_of_one_name_fold_into_one_card() {
        let input = deck(
            sections(),
            vec![
                entry("a1", "Rhystic Study", MAIN, 1, 0).game_changer(true),
                entry("a2", "Rhystic Study", COMMAND, 0, 1).game_changer(true),
            ],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        let changers = category(&estimate, DeckBracketSignal::GameChanger);
        assert_eq!(changers.count, 1);
        assert_eq!(changers.cards.len(), 1);
        assert_eq!(changers.cards[0].quantity, 2);
    }

    /// A zero-count row is a deck entry on its way out; it is not in the deck.
    #[test]
    fn a_zero_count_row_is_not_in_the_deck() {
        let input = deck(
            sections(),
            vec![entry("gc", "Rhystic Study", MAIN, 0, 0).game_changer(true)],
        );
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert_eq!(estimate.bracket, 2);
    }

    /// `count` is exact even when the list is capped, so a huge deck can't quietly report
    /// fewer Game Changers than it holds.
    #[test]
    fn the_card_list_is_capped_but_the_count_is_not() {
        let entries: Vec<AnalysisEntry> = (1..=(MAX_LISTED_CARDS as i32 + 5))
            .map(game_changer)
            .collect();
        let input = deck(sections(), entries);
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        let changers = category(&estimate, DeckBracketSignal::GameChanger);
        assert_eq!(changers.count, MAX_LISTED_CARDS as i64 + 5);
        assert_eq!(changers.cards.len(), MAX_LISTED_CARDS);
    }

    /// Every category is always reported, in a stable order, so a client can lay the panel
    /// out without conditional slots.
    #[test]
    fn every_category_is_always_reported_in_order() {
        let input = deck(sections(), vec![entry("a", "A", MAIN, 1, 0)]);
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        let signals: Vec<DeckBracketSignal> = estimate
            .categories
            .iter()
            .map(|category| category.signal)
            .collect();
        assert_eq!(
            signals,
            vec![
                DeckBracketSignal::GameChanger,
                DeckBracketSignal::MassLandDenial,
                DeckBracketSignal::ExtraTurn,
                DeckBracketSignal::Tutor,
            ]
        );
        assert!(
            estimate
                .categories
                .iter()
                .all(|c| !c.description.is_empty()),
            "each category explains itself"
        );
    }

    /// The floor is only honest alongside what it couldn't see, so the caveats are never
    /// empty — combos and cEDH intent are unconditional.
    #[test]
    fn the_caveats_always_name_what_was_not_checked() {
        let input = deck(sections(), vec![entry("a", "A", MAIN, 1, 0)]);
        let estimate = analyse_bracket(Some("Commander"), &input).unwrap();
        assert!(estimate.caveats.iter().any(|c| c.contains("combos")));
        assert!(estimate.caveats.iter().any(|c| c.contains("cEDH")));
    }

    #[test]
    fn a_reason_names_at_most_three_cards_then_counts_the_rest() {
        let cards: Vec<DeckBracketCard> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|name| DeckBracketCard {
                card_id: name.to_lowercase(),
                name: (*name).to_string(),
                quantity: 1,
            })
            .collect();
        assert_eq!(name_list(&cards), "A, B, C and 2 more");
        assert_eq!(name_list(&cards[..1]), "A");
        assert_eq!(name_list(&cards[..2]), "A and B");
        assert_eq!(name_list(&[]), "");
    }
}
