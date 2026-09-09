//! The deck's **card roles** — how many pieces of ramp, card draw, removal, board wipes,
//! counterspells, tutors, recursion and protection it holds, and which cards those are
//! (issue #671).
//!
//! "Ten ramp, ten draw, eight removal, three wipes" is the shape a Commander list is built
//! to, and the one thing the type presets can't say: they file a Cultivate under sorceries.
//! This read answers it the way the bracket estimate answers its question — one grammar per
//! role over the card's own rules text ([`super::signals::roles`], sharing its clause
//! grammar with the bracket's signals), every predicate declining when unsure, and every
//! counted card handed back so a number can be checked against the list it was made of.
//!
//! Three shapes of answer ride the response, and each serves a different reader. `roles` is
//! the bars: one group per role, always all eight in a fixed order, with a distinct-name
//! `count`, a copy-weighted `copies` and the counted cards (folded by name, capped like the
//! bracket's lists — the count stays exact). `card_roles` is the filter: every printing in
//! the deck (maybeboards included — the list a page narrows shows them too) that holds at
//! least one role, keyed by its external id, which is what lets a deck page narrow its list
//! to "the removal" without a second reader of the rules text. And `unclassified_count` is the honesty line: how many distinct cards matched no
//! role at all, so the bars are never mistaken for a partition of the deck.
//!
//! Scoped to the **deck proper** — a maybeboard card is under consideration, not played
//! (issue #570) — and the **command zone is in**: a commander that draws is draw. Every
//! format gets an answer, not only Commander; the roles are a deckbuilding vocabulary, not
//! a format's rule.

use std::collections::BTreeMap;

use serde::Serialize;

use super::signals::bracket::is_tutor;
use super::signals::roles::{
    is_board_wipe, is_card_draw, is_counterspell, is_protection, is_ramp, is_recursion, is_removal,
};
use super::{CardFacts, DeckAnalysisInput, fold_by_name};

/// Cards listed per role. `count` stays exact — a deck's row count is caller-controlled and
/// this response isn't paginated, so the list is capped the way the bracket's are.
const MAX_LISTED_CARDS: usize = 50;

/// A deckbuilding role a card can fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckRole {
    /// Makes more mana than a land drop does, or puts extra lands onto the battlefield.
    Ramp,
    /// Draws you cards.
    CardDraw,
    /// Answers one thing an opponent has — destroys, exiles, bounces, damages or edicts it.
    Removal,
    /// Removes permanents as a group.
    BoardWipe,
    /// Counters a spell or an ability on the stack.
    Counterspell,
    /// Searches the library for something that isn't a land.
    Tutor,
    /// Returns other cards from a graveyard to hand or to the battlefield.
    Recursion,
    /// Keeps something of yours from being answered — grants hexproof or indestructible,
    /// regenerates, prevents damage, phases out, or flickers your own permanent.
    Protection,
}

/// The roles in the order a builder counts them — the six everyone counts first, then the
/// two that round a list out.
pub(super) const ROLES: &[(DeckRole, &str, &str)] = &[
    (
        DeckRole::Ramp,
        "Ramp",
        "Mana rocks and dorks, and land searches that put a land onto the battlefield. A search to hand isn't ramp, and a land never is.",
    ),
    (
        DeckRole::CardDraw,
        "Card draw",
        "Cards that draw you cards. Triggers on drawing and draw replacements don't count.",
    ),
    (
        DeckRole::Removal,
        "Removal",
        "Targeted answers: destroy, exile, bounce, burn, fight, shrink, or an edict.",
    ),
    (
        DeckRole::BoardWipe,
        "Board wipes",
        "Mass removal — every creature, every nonland permanent, damage to each creature.",
    ),
    (
        DeckRole::Counterspell,
        "Counterspells",
        "Counters a spell or an ability on the stack.",
    ),
    (
        DeckRole::Tutor,
        "Tutors",
        "Library searches for something other than a land — the bracket's own reading.",
    ),
    (
        DeckRole::Recursion,
        "Recursion",
        "Returns other cards from a graveyard to hand or to the battlefield. A card that returns itself doesn't count.",
    ),
    (
        DeckRole::Protection,
        "Protection",
        "Grants hexproof or indestructible, regenerates, prevents damage, phases out, or flickers your own permanent. A card's own keyword protects only itself.",
    ),
];

fn matches_role(role: DeckRole, card: &CardFacts) -> bool {
    match role {
        DeckRole::Ramp => is_ramp(card),
        DeckRole::CardDraw => is_card_draw(card),
        DeckRole::Removal => is_removal(card),
        DeckRole::BoardWipe => is_board_wipe(card),
        DeckRole::Counterspell => is_counterspell(card),
        DeckRole::Tutor => is_tutor(card),
        DeckRole::Recursion => is_recursion(card),
        DeckRole::Protection => is_protection(card),
    }
}

/// Every role one card fills, in the reported order. Pure, so the same reading fills both
/// the per-role lists and the per-printing map — the two can't disagree. Shared with the
/// suggestions read, so "you have N, you own M more" counts both sides with one grammar.
pub(super) fn roles_of(card: &CardFacts) -> Vec<DeckRole> {
    ROLES
        .iter()
        .map(|(role, _, _)| *role)
        .filter(|role| matches_role(*role, card))
        .collect()
}

/// One card counted towards a role.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckRoleCard {
    /// External card id of one printing (for keys and links).
    pub card_id: String,
    pub name: String,
    /// Copies of that name across the deck proper (regular + foil, every section).
    pub quantity: i64,
}

/// What the deck holds in one role.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckRoleGroup {
    pub role: DeckRole,
    pub label: String,
    /// What the role counts, and the near-miss it deliberately doesn't.
    pub description: String,
    /// Distinct card **names** in this role — a card held in two arts counts once.
    pub count: i64,
    /// Copies (regular + foil) across those names — what a 60-card builder counts, and the
    /// same number as `count` in a singleton deck.
    pub copies: i64,
    /// The matched cards in the deck's own order, capped (`count` stays exact).
    pub cards: Vec<DeckRoleCard>,
}

/// Everything `GET /api/decks/{game}/{deck_id}/roles` answers.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckRoles {
    /// Every role, in a stable order, whether or not the deck holds any.
    pub roles: Vec<DeckRoleGroup>,
    /// The roles each printing in the deck fills, keyed by external card id — only printings
    /// holding at least one role appear, and a card may fill several. **Maybeboards are in
    /// this map** although they are out of every count above: a role is a fact about the
    /// card, and the list a page filters by it still shows its maybeboards.
    pub card_roles: BTreeMap<String, Vec<DeckRole>>,
    /// Distinct card names in the deck proper.
    pub card_count: i64,
    /// Distinct card names that matched no role. The roles aren't a partition of the deck
    /// — most creatures and every land are here — so this is on the wire rather than left
    /// to be inferred wrongly from the bars.
    pub unclassified_count: i64,
}

/// Count a loaded deck's roles.
pub(crate) fn analyse_roles(input: &DeckAnalysisInput) -> DeckRoles {
    // The deck **proper**, command zone included (issue #570's split, the bracket's stance).
    let proper = input.deck_proper();
    let folds = fold_by_name(&proper);

    // One reading per distinct name, shared by every shape of the answer below.
    let readings: Vec<Vec<DeckRole>> = folds.iter().map(|fold| roles_of(fold.facts)).collect();

    let roles = ROLES
        .iter()
        .map(|(role, label, description)| {
            let matched: Vec<usize> = readings
                .iter()
                .enumerate()
                .filter(|(_, held)| held.contains(role))
                .map(|(index, _)| index)
                .collect();
            DeckRoleGroup {
                role: *role,
                label: (*label).to_string(),
                description: (*description).to_string(),
                count: matched.len() as i64,
                copies: matched
                    .iter()
                    .map(|index| folds[*index].copies)
                    .fold(0i64, i64::saturating_add),
                cards: matched
                    .iter()
                    .take(MAX_LISTED_CARDS)
                    .map(|index| DeckRoleCard {
                        card_id: folds[*index].card_id.clone(),
                        name: folds[*index].facts.name.clone(),
                        quantity: folds[*index].copies,
                    })
                    .collect(),
            }
        })
        .collect();

    // Every printing, not only the fold's representative: the deck page filters rows by
    // the printing they hold, and a second art of one card must narrow with the first. And
    // every row, maybeboards included — the filtered list still shows them, and a maybeboard
    // Cultivate that vanished under "Ramp" would read as "not ramp". A name only a maybeboard
    // holds is read here, once, since no fold above did.
    let mut held_by_name: BTreeMap<&str, Vec<DeckRole>> = BTreeMap::new();
    for (fold, held) in folds.iter().zip(&readings) {
        held_by_name.insert(fold.facts.name.as_str(), held.clone());
    }
    let mut card_roles: BTreeMap<String, Vec<DeckRole>> = BTreeMap::new();
    for entry in input.entries.iter().filter(|entry| entry.copies() > 0) {
        let held = held_by_name
            .entry(entry.facts.name.as_str())
            .or_insert_with(|| roles_of(&entry.facts));
        if !held.is_empty() {
            card_roles.insert(entry.facts.id.clone(), held.clone());
        }
    }

    let unclassified_count = readings.iter().filter(|held| held.is_empty()).count() as i64;

    DeckRoles {
        roles,
        card_roles,
        card_count: folds.len() as i64,
        unclassified_count,
    }
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

    fn group(roles: &DeckRoles, role: DeckRole) -> &DeckRoleGroup {
        roles
            .roles
            .iter()
            .find(|group| group.role == role)
            .expect("every role is always reported")
    }

    #[test]
    fn every_role_is_always_reported_in_order() {
        let input = deck(sections(), vec![entry("a", "A", MAIN, 1, 0)]);
        let roles = analyse_roles(&input);
        let order: Vec<DeckRole> = roles.roles.iter().map(|group| group.role).collect();
        assert_eq!(
            order,
            vec![
                DeckRole::Ramp,
                DeckRole::CardDraw,
                DeckRole::Removal,
                DeckRole::BoardWipe,
                DeckRole::Counterspell,
                DeckRole::Tutor,
                DeckRole::Recursion,
                DeckRole::Protection,
            ]
        );
        assert!(
            roles
                .roles
                .iter()
                .all(|group| !group.description.is_empty())
        );
        assert_eq!(roles.card_count, 1);
        assert_eq!(roles.unclassified_count, 1, "a blank card fills no role");
        assert!(roles.card_roles.is_empty());
    }

    #[test]
    fn a_commander_list_is_counted_the_way_a_builder_counts_it() {
        let input = deck(
            sections(),
            vec![
                entry("sol", "Sol Ring", MAIN, 1, 0).oracle("{T}: Add {C}{C}."),
                entry("cult", "Cultivate", MAIN, 1, 0).oracle(
                    "Search your library for up to two basic land cards, reveal those cards, put one onto the battlefield tapped and the other into your hand, then shuffle.",
                ),
                entry("harm", "Harmonize", MAIN, 1, 0).oracle("Draw three cards."),
                entry("stp", "Swords to Plowshares", MAIN, 1, 0)
                    .oracle("Exile target creature. Its controller gains life equal to its power."),
                entry("wrath", "Wrath of God", MAIN, 1, 0)
                    .oracle("Destroy all creatures. They can't be regenerated."),
                entry("cs", "Counterspell", MAIN, 1, 0).oracle("Counter target spell."),
                entry("dt", "Demonic Tutor", MAIN, 1, 0)
                    .oracle("Search your library for a card, put that card into your hand, then shuffle."),
                entry("ew", "Eternal Witness", MAIN, 1, 0).oracle(
                    "When this creature enters, you may return target card from your graveyard to your hand.",
                ),
                entry("boots", "Swiftfoot Boots", MAIN, 1, 0)
                    .oracle("Equipped creature has hexproof and haste.\nEquip {1}"),
                entry("bear", "Grizzly Bears", MAIN, 1, 0).type_line("Creature — Bear"),
                entry("forest", "Forest", MAIN, 30, 0)
                    .type_line("Basic Land — Forest")
                    .oracle("({T}: Add {G}.)"),
            ],
        );
        let roles = analyse_roles(&input);
        let names = |role: DeckRole| -> Vec<&str> {
            group(&roles, role)
                .cards
                .iter()
                .map(|card| card.name.as_str())
                .collect()
        };
        assert_eq!(names(DeckRole::Ramp), vec!["Sol Ring", "Cultivate"]);
        assert_eq!(names(DeckRole::CardDraw), vec!["Harmonize"]);
        assert_eq!(names(DeckRole::Removal), vec!["Swords to Plowshares"]);
        assert_eq!(names(DeckRole::BoardWipe), vec!["Wrath of God"]);
        assert_eq!(names(DeckRole::Counterspell), vec!["Counterspell"]);
        assert_eq!(names(DeckRole::Tutor), vec!["Demonic Tutor"]);
        assert_eq!(names(DeckRole::Recursion), vec!["Eternal Witness"]);
        assert_eq!(names(DeckRole::Protection), vec!["Swiftfoot Boots"]);
        assert_eq!(roles.card_count, 11);
        assert_eq!(roles.unclassified_count, 2, "the bear and the Forest");
        assert_eq!(roles.card_roles.get("sol"), Some(&vec![DeckRole::Ramp]));
        assert!(!roles.card_roles.contains_key("forest"));
    }

    /// A cantripping removal spell is both — the roles are not a partition.
    #[test]
    fn a_card_may_fill_several_roles() {
        let input = deck(
            sections(),
            vec![
                entry("ac", "Anguished Unmaking-ish", MAIN, 1, 0)
                    .oracle("Exile target nonland permanent. Draw a card."),
            ],
        );
        let roles = analyse_roles(&input);
        assert_eq!(group(&roles, DeckRole::Removal).count, 1);
        assert_eq!(group(&roles, DeckRole::CardDraw).count, 1);
        assert_eq!(
            roles.card_roles.get("ac"),
            Some(&vec![DeckRole::CardDraw, DeckRole::Removal]),
            "in the reported order"
        );
        assert_eq!(roles.unclassified_count, 0);
    }

    /// A maybeboard card is being considered, not played (issue #570); a commander is
    /// played — one that draws is draw.
    #[test]
    fn the_maybeboard_is_out_and_the_command_zone_is_in() {
        let input = deck(
            sections(),
            vec![
                entry("cmd", "Tatyova, Benthic Druid", COMMAND, 1, 0)
                    .oracle("Landfall — Whenever a land you control enters, you gain 1 life and draw a card."),
                entry("maybe", "Harmonize", MAYBE, 1, 0).oracle("Draw three cards."),
            ],
        );
        let roles = analyse_roles(&input);
        let draw = group(&roles, DeckRole::CardDraw);
        assert_eq!(draw.count, 1);
        assert_eq!(draw.cards[0].name, "Tatyova, Benthic Druid");
        assert!(roles.card_roles.contains_key("cmd"));
        // …but the filter map still knows what the maybeboard card is, because the list it
        // narrows shows maybeboards too.
        assert_eq!(
            roles.card_roles.get("maybe"),
            Some(&vec![DeckRole::CardDraw]),
            "a maybeboard row is filterable even though it is not counted"
        );
        assert_eq!(roles.card_count, 1);
    }

    /// Two printings of one card are one card with both copies — and **both** printings are
    /// in the filter map, since the page narrows rows by the printing each holds.
    #[test]
    fn printings_fold_by_name_but_every_printing_is_filterable() {
        let input = deck(
            sections(),
            vec![
                entry("bolt1", "Lightning Bolt", MAIN, 2, 0)
                    .oracle("Lightning Bolt deals 3 damage to any target."),
                entry("bolt2", "Lightning Bolt", MAIN, 1, 1)
                    .oracle("Lightning Bolt deals 3 damage to any target."),
            ],
        );
        let roles = analyse_roles(&input);
        let removal = group(&roles, DeckRole::Removal);
        assert_eq!(removal.count, 1);
        assert_eq!(removal.copies, 4);
        assert_eq!(removal.cards.len(), 1);
        assert_eq!(removal.cards[0].quantity, 4);
        assert_eq!(roles.card_roles.len(), 2);
        assert_eq!(roles.card_count, 1);
    }

    /// A zero-count row is a deck entry on its way out; it is not in the deck.
    #[test]
    fn a_zero_count_row_is_not_in_the_deck() {
        let input = deck(
            sections(),
            vec![entry("cs", "Counterspell", MAIN, 0, 0).oracle("Counter target spell.")],
        );
        let roles = analyse_roles(&input);
        assert_eq!(group(&roles, DeckRole::Counterspell).count, 0);
        assert!(roles.card_roles.is_empty());
        assert_eq!(roles.card_count, 0);
    }

    /// `count` and `copies` are exact even when the list is capped.
    #[test]
    fn the_card_list_is_capped_but_the_counts_are_not() {
        let entries = (1..=(MAX_LISTED_CARDS as i32 + 5))
            .map(|index| {
                entry(&format!("r{index}"), &format!("Rock {index}"), MAIN, 2, 0)
                    .oracle("{T}: Add {C}.")
            })
            .collect();
        let roles = analyse_roles(&deck(sections(), entries));
        let ramp = group(&roles, DeckRole::Ramp);
        assert_eq!(ramp.count, MAX_LISTED_CARDS as i64 + 5);
        assert_eq!(ramp.copies, (MAX_LISTED_CARDS as i64 + 5) * 2);
        assert_eq!(ramp.cards.len(), MAX_LISTED_CARDS);
        assert_eq!(
            roles.card_roles.len(),
            MAX_LISTED_CARDS + 5,
            "the filter map is never capped — it is bounded by the deck"
        );
    }
}
