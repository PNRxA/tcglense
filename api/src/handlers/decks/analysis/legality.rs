//! The **per-card** half of legality — is this card banned or restricted in the deck's
//! format? — and the composition of that with the deck-construction rules in
//! [`super::rules`] into the single verdict a deck page (or a CLI) renders.
//!
//! `cards.legalities` is the provider's per-format object stored verbatim
//! (`{"modern": "banned", …}`) and `deck.format` is a free-form label the user picked or
//! typed; [`super::formats`] bridges the two. Everything here is deliberately reluctant:
//! a card with no legality data, a format we don't track, or a legalities object missing
//! this format's key is **never** an issue, because a false "in breach" is worse than a
//! miss.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::rules::{
    DeckRuleCardStatus, DeckRuleSeverity, DeckRuleViolation, command_zone_section_ids,
    evaluate_deck_rules,
};
use super::{CardFacts, DeckAnalysisInput};

/// A legality value as the provider writes it. Anything else is treated as unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegalityStatus {
    Legal,
    NotLegal,
    Banned,
    Restricted,
}

/// A breach-worthy status for one card. The first four come from the card's own legality
/// data; `off_colour` and `over_limit` come from the deck-construction rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckIssueStatus {
    // Declaration order **is** severity order, most severe first: it sorts the issue list,
    // picks a card's worst status when two rules catch it, and gives the banner its summary
    // order. The derived `Ord` is that order, so nothing has to restate it.
    Banned,
    NotLegal,
    CommanderOnly,
    OffColour,
    OverLimit,
    Restricted,
}

impl From<DeckRuleCardStatus> for DeckIssueStatus {
    fn from(status: DeckRuleCardStatus) -> Self {
        match status {
            DeckRuleCardStatus::OffColour => DeckIssueStatus::OffColour,
            DeckRuleCardStatus::OverLimit => DeckIssueStatus::OverLimit,
        }
    }
}

/// One offending card name in a deck (all printings of a name fold into one issue).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckLegalityIssue {
    /// External card id of one printing (for keys/links).
    pub card_id: String,
    pub name: String,
    pub status: DeckIssueStatus,
    /// Total copies across every section and printing (regular + foil).
    pub quantity: i64,
}

/// A deck's legality verdict: the offending cards, the deck-wide construction breaches, and
/// whether you could sit down with it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckLegality {
    /// The legality key the deck's format label normalised to.
    pub format_key: String,
    /// That key's display label.
    pub format_label: String,
    /// Sorted most severe first, alphabetical within each status.
    pub issues: Vec<DeckLegalityIssue>,
    /// Deck-wide construction breaches (size, command zone, colour identity).
    pub violations: Vec<DeckRuleViolation>,
    /// Per-**printing** status for every entry belonging to an offending name, keyed by
    /// external card id — what a deck view chips onto a tile.
    pub card_statuses: BTreeMap<String, DeckIssueStatus>,
    /// Cards whose catalog row carries no legality data at all (not counted as issues).
    pub unknown_count: i64,
    /// No card issues and no error-severity violation — a deck you could sit down with.
    pub legal: bool,
}

/// A card's status in one format, or `None` when unknown (no data / unexpected value).
pub(crate) fn status_of(card: &CardFacts, format_key: &str) -> Option<LegalityStatus> {
    match card.legalities.as_ref()?.get(format_key)?.as_str() {
        "legal" => Some(LegalityStatus::Legal),
        "not_legal" => Some(LegalityStatus::NotLegal),
        "banned" => Some(LegalityStatus::Banned),
        "restricted" => Some(LegalityStatus::Restricted),
        _ => None,
    }
}

/// Evaluate a deck against a format key. Per-card semantics:
///
/// - `banned` / `not_legal` in the format -> an issue, always.
/// - `restricted` -> an issue only when more than one total copy of that name is in the
///   deck (Vintage's "max 1 copy" rule). Pauper Commander is the exception: the provider
///   writes `restricted` there to mean "legal only as the commander" (an uncommon
///   creature), so it's an issue when the card sits anywhere *but* the command zone.
/// - A card with no legality data, or a legalities object missing this format's key, is
///   counted in `unknown_count` and never flagged.
///
/// Copy counts fold across sections **and** printings by card name, so 2x of one printing
/// of a restricted card plus 1x of another printing is still a breach.
pub(crate) fn evaluate_deck_legality(format_key: &str, input: &DeckAnalysisInput) -> DeckLegality {
    // The deck proper: a maybeboard card is under consideration, not played, so it is no
    // more part of the legality verdict than it is of the card count (issue #570).
    let entries = input.deck_proper();

    // Pass 1: fold total copies per card name (restricted needs cross-printing totals).
    let mut copies_by_name: HashMap<&str, i64> = HashMap::new();
    for entry in &entries {
        *copies_by_name.entry(entry.facts.name.as_str()).or_default() += entry.signed_copies();
    }

    // Pass 2: judge each printing against the card's own legality data.
    let command_zone = command_zone_section_ids(&input.sections);
    let mut found: Vec<DeckLegalityIssue> = Vec::new();
    let mut unknown_count = 0i64;
    for entry in &entries {
        let Some(status) = status_of(&entry.facts, format_key) else {
            unknown_count += 1;
            continue;
        };
        let quantity = copies_by_name
            .get(entry.facts.name.as_str())
            .copied()
            .unwrap_or(0);
        let issue = match status {
            LegalityStatus::Banned => Some(DeckIssueStatus::Banned),
            LegalityStatus::NotLegal => Some(DeckIssueStatus::NotLegal),
            LegalityStatus::Legal => None,
            LegalityStatus::Restricted => {
                if format_key == "paupercommander" {
                    (!command_zone.contains(&entry.section_id))
                        .then_some(DeckIssueStatus::CommanderOnly)
                } else {
                    (quantity > 1).then_some(DeckIssueStatus::Restricted)
                }
            }
        };
        if let Some(status) = issue {
            found.push(DeckLegalityIssue {
                card_id: entry.facts.id.clone(),
                name: entry.facts.name.clone(),
                status,
                quantity,
            });
        }
    }

    // Pass 3: the deck-wide rules, whose per-card breaches join the same list.
    let rules = evaluate_deck_rules(format_key, &entries, &input.sections);
    found.extend(rules.card_issues.iter().map(|issue| DeckLegalityIssue {
        card_id: issue.card_id.clone(),
        name: issue.name.clone(),
        status: issue.status.into(),
        quantity: issue.quantity,
    }));

    // Fold to one issue per name and one chip per printing, keeping the worst status of
    // each. First seen wins a tie, so the list reads in the deck's own card order.
    let mut card_statuses: BTreeMap<String, DeckIssueStatus> = BTreeMap::new();
    let mut issue_order: Vec<String> = Vec::new();
    let mut issue_by_name: HashMap<String, DeckLegalityIssue> = HashMap::new();
    for issue in found {
        card_statuses
            .entry(issue.card_id.clone())
            .and_modify(|previous| {
                if issue.status < *previous {
                    *previous = issue.status;
                }
            })
            .or_insert(issue.status);
        match issue_by_name.get_mut(&issue.name) {
            Some(existing) => {
                if issue.status < existing.status {
                    *existing = issue;
                }
            }
            None => {
                issue_order.push(issue.name.clone());
                issue_by_name.insert(issue.name.clone(), issue);
            }
        }
    }
    let mut issues: Vec<DeckLegalityIssue> = issue_order
        .into_iter()
        .filter_map(|name| issue_by_name.remove(&name))
        .collect();
    issues.sort_by(|left, right| {
        left.status.cmp(&right.status).then_with(|| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.name.cmp(&right.name))
        })
    });

    let legal = issues.is_empty()
        && !rules
            .violations
            .iter()
            .any(|violation| violation.severity == DeckRuleSeverity::Error);

    DeckLegality {
        format_key: format_key.to_string(),
        format_label: super::formats::format_label(format_key),
        issues,
        violations: rules.violations,
        card_statuses,
        unknown_count,
        legal,
    }
}

/// Evaluate a loaded deck, or `None` when its format isn't a legality-tracked one (custom
/// text, "Cube", "Casual", a blank field) — `None` means "nothing to evaluate", never
/// "illegal".
pub(crate) fn analyse_legality(
    format: Option<&str>,
    input: &DeckAnalysisInput,
) -> Option<DeckLegality> {
    let key = super::formats::normalize_format_key(format)?;
    Some(evaluate_deck_legality(key, input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{deck, entry, section};

    #[test]
    fn an_untracked_format_is_not_evaluated() {
        let input = deck(
            vec![section(1, "Main", false)],
            vec![entry("a", "A", 1, 1, 0).legal("commander", "legal")],
        );
        assert!(analyse_legality(None, &input).is_none());
        assert!(analyse_legality(Some(""), &input).is_none());
        assert!(analyse_legality(Some("Cube"), &input).is_none());
        assert!(analyse_legality(Some("Commander"), &input).is_some());
    }

    #[test]
    fn a_banned_card_is_always_an_issue() {
        let input = deck(
            vec![section(1, "Main", false)],
            vec![entry("a", "Sway", 1, 1, 0).legal("commander", "banned")],
        );
        let result = analyse_legality(Some("EDH"), &input).unwrap();
        assert_eq!(result.format_key, "commander");
        assert_eq!(result.format_label, "Commander");
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].status, DeckIssueStatus::Banned);
        assert!(!result.legal);
        assert_eq!(result.card_statuses["a"], DeckIssueStatus::Banned);
    }

    #[test]
    fn a_card_without_legality_data_is_unknown_never_a_breach() {
        let input = deck(
            vec![section(1, "Main", false)],
            // No legalities at all, and a legalities object missing the format's key.
            vec![
                entry("a", "Mystery", 1, 1, 0),
                entry("b", "Other", 1, 1, 0).legal("modern", "legal"),
            ],
        );
        let result = analyse_legality(Some("Commander"), &input).unwrap();
        assert_eq!(result.unknown_count, 2);
        assert!(result.issues.is_empty());
        assert!(result.legal);
    }

    #[test]
    fn restricted_only_breaches_above_one_copy_and_folds_across_printings() {
        let one = deck(
            vec![section(1, "Main", false)],
            vec![entry("r1", "Ancestral", 1, 1, 0).legal("vintage", "restricted")],
        );
        assert!(
            analyse_legality(Some("Vintage"), &one)
                .unwrap()
                .issues
                .is_empty()
        );

        let two = deck(
            vec![section(1, "Main", false)],
            vec![
                entry("r1", "Ancestral", 1, 1, 0).legal("vintage", "restricted"),
                entry("r2", "Ancestral", 1, 0, 1).legal("vintage", "restricted"),
            ],
        );
        let result = analyse_legality(Some("Vintage"), &two).unwrap();
        assert_eq!(result.issues.len(), 1, "one issue per name");
        assert_eq!(result.issues[0].status, DeckIssueStatus::Restricted);
        assert_eq!(result.issues[0].quantity, 2);
        assert_eq!(result.card_statuses.len(), 2, "one chip per printing");
    }

    #[test]
    fn pauper_commander_reads_restricted_as_commander_only() {
        let sections = vec![section(1, "Commander", false), section(2, "Main", false)];
        let leading = deck(
            sections.clone(),
            vec![entry("c", "Chulane", 1, 1, 0).legal("paupercommander", "restricted")],
        );
        assert!(
            analyse_legality(Some("PDH"), &leading)
                .unwrap()
                .issues
                .is_empty(),
            "the same card is fine in the command zone"
        );

        let in_the_99 = deck(
            sections,
            vec![entry("c", "Chulane", 2, 1, 0).legal("paupercommander", "restricted")],
        );
        let result = analyse_legality(Some("PDH"), &in_the_99).unwrap();
        assert_eq!(result.issues[0].status, DeckIssueStatus::CommanderOnly);
    }

    #[test]
    fn a_maybeboard_card_is_not_part_of_the_verdict() {
        let input = deck(
            vec![section(1, "Main", false), section(2, "Cuts", true)],
            vec![entry("bad", "Sway", 2, 1, 0).legal("commander", "banned")],
        );
        let result = analyse_legality(Some("Commander"), &input).unwrap();
        assert!(
            result.issues.is_empty(),
            "a card only being considered is not played"
        );
        assert!(result.legal);
    }

    #[test]
    fn the_worst_status_wins_when_two_rules_catch_one_card() {
        // Banned *and* over the singleton limit: the banner should say banned.
        let input = deck(
            vec![section(1, "Main", false)],
            vec![entry("a", "Sway", 1, 2, 0).legal("commander", "banned")],
        );
        let result = analyse_legality(Some("Commander"), &input).unwrap();
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].status, DeckIssueStatus::Banned);
        assert_eq!(result.card_statuses["a"], DeckIssueStatus::Banned);
    }

    #[test]
    fn issues_sort_most_severe_first_then_by_name() {
        let input = deck(
            vec![section(1, "Main", false)],
            vec![
                entry("z", "Zebra", 1, 1, 0).legal("modern", "not_legal"),
                entry("a", "Aardvark", 1, 1, 0).legal("modern", "not_legal"),
                entry("b", "Banned One", 1, 1, 0).legal("modern", "banned"),
            ],
        );
        let result = analyse_legality(Some("Modern"), &input).unwrap();
        let names: Vec<&str> = result.issues.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Banned One", "Aardvark", "Zebra"]);
    }

    #[test]
    fn construction_violations_ride_the_same_verdict() {
        let input = deck(
            vec![section(1, "Commander", false), section(2, "Main", false)],
            vec![
                entry("cmd", "Krenko", 1, 1, 0)
                    .type_line("Legendary Creature — Goblin")
                    .colors("R")
                    .legal("commander", "legal"),
                entry("off", "Counterspell", 2, 1, 0)
                    .type_line("Instant")
                    .colors("U")
                    .legal("commander", "legal"),
            ],
        );
        let result = analyse_legality(Some("Commander"), &input).unwrap();
        assert!(!result.legal);
        assert_eq!(result.issues[0].status, DeckIssueStatus::OffColour);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.message.contains("colour identity"))
        );
    }

    #[test]
    fn a_warning_alone_leaves_the_deck_legal() {
        let input = deck(
            vec![section(1, "Commander", false), section(2, "Main", false)],
            vec![
                entry("cmd", "Krenko", 1, 1, 0)
                    .type_line("Legendary Creature — Goblin")
                    .colors("R")
                    .legal("commander", "legal"),
            ],
        );
        let result = analyse_legality(Some("Commander"), &input).unwrap();
        assert!(result.legal, "an unfinished deck is not an illegal one");
        assert!(
            result
                .violations
                .iter()
                .all(|v| v.severity == DeckRuleSeverity::Warning)
        );
    }
}
