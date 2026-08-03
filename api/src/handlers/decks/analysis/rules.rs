//! Deck-**construction** rules per format: how many cards a deck holds, how many copies of
//! one name, what the command zone contains, and whether the 99 stay inside the commander's
//! colour identity. The per-card side of legality — the provider's banned/restricted data —
//! is [`super::legality`], which composes the two; nothing here reaches back into it, so
//! the dependency stays one-way.
//!
//! Everything is derived from data already on the catalog row — type line, colour identity,
//! oracle text — so a newly printed Partner commander or "any number of cards named" card
//! obeys the rules the day the catalog ingests it, with no curated list to maintain. A
//! false "in breach" is worse than a miss, so an unrecognised format, an empty deck, or a
//! card we can't read confidently is **skipped rather than guessed at**, and "you haven't
//! finished building this yet" is a `warning`, never an `error`.

use std::collections::HashMap;

use serde::Serialize;

use crate::handlers::decks::DeckSectionResponse;

use super::{AnalysisEntry, CardFacts};

// ---------- Zones ----------

/// Which zone a section's cards sit in. Derived from the section *name* (a deck has no
/// per-section zone column, and users file cards by category), so this is deliberately
/// generous: anything unrecognised is the deck proper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeckZone {
    Command,
    Sideboard,
    Main,
}

/// Section names that mean "command zone" — the deck's seeded `Commander` plus the
/// spellings imports and Oathbreaker players arrive with.
const COMMAND_ZONE_NAMES: &[&str] = &[
    "commander",
    "commanders",
    "command zone",
    "oathbreaker",
    "oathbreakers",
    "signature spell",
    "signature spells",
];

/// Section names that sit beside the deck rather than in it. A companion lives in the
/// sideboard in constructed and outside the 100 in Commander, so it counts as one here.
const SIDEBOARD_NAMES: &[&str] = &[
    "sideboard",
    "sideboards",
    "side board",
    "companion",
    "companions",
];

/// The zone a section name puts its cards in.
pub(crate) fn deck_zone(name: &str) -> DeckZone {
    let key = name.trim().to_lowercase();
    if COMMAND_ZONE_NAMES.contains(&key.as_str()) {
        DeckZone::Command
    } else if SIDEBOARD_NAMES.contains(&key.as_str()) {
        DeckZone::Sideboard
    } else {
        DeckZone::Main
    }
}

/// Ids of the sections holding the command zone — the per-card check needs them to tell a
/// Pauper Commander's uncommon commander from the same card sitting in the 99.
pub(crate) fn command_zone_section_ids(sections: &[DeckSectionResponse]) -> Vec<i32> {
    sections
        .iter()
        .filter(|section| deck_zone(&section.name) == DeckZone::Command)
        .map(|section| section.id)
        .collect()
}

// ---------- Card predicates ----------

/// Whether `word` appears in `line` on word boundaries, the way a `\b…\b` regex matches —
/// a word character being `[A-Za-z0-9_]`. `line` is already lowercased; `word` must be too.
fn has_word(line: &str, word: &str) -> bool {
    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(offset) = line[from..].find(word) {
        let start = from + offset;
        let end = start + word.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Whether every one of `words` is on the card's front type line.
fn has_type(card: &CardFacts, words: &[&str]) -> bool {
    words
        .iter()
        .all(|word| has_word(&card.front_type_line, word))
}

fn is_basic_land(card: &CardFacts) -> bool {
    has_type(card, &["basic", "land"])
}

/// Oracle text as lowercased ability lines with reminder text stripped, so a keyword test
/// matches the ability itself and never the parenthetical that explains it (every Partner
/// card's reminder text names Partner, and so does nothing else).
fn ability_lines(card: &CardFacts) -> Vec<String> {
    // `(…)` spans are dropped, non-nested and only when they close — matching the
    // `\([^)]*\)` the SPA used, so an unbalanced `(` leaves its text in place rather than
    // swallowing the rest of the card.
    let mut stripped = String::with_capacity(card.oracle_text.len());
    let mut rest = card.oracle_text.as_str();
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        stripped.push_str(&rest[..open]);
        rest = &rest[open + close + 1..];
    }
    stripped.push_str(rest);
    stripped
        .replace(['\u{2018}', '\u{2019}'], "'")
        .split('\n')
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Whether the card has a keyword ability on a line of its own (with or without a cost or
/// subject clause after it — "Partner with …", "Partner—Survivors").
fn has_ability(card: &CardFacts, keyword: &str) -> bool {
    ability_lines(card).iter().any(|line| {
        line == keyword
            || line
                .strip_prefix(keyword)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|next| next == ' ' || next == '\u{2014}' || next == '-')
    })
}

/// The card named by a "Partner with <name>" ability, lowercased, or `None`.
fn partner_with_name(card: &CardFacts) -> Option<String> {
    for line in ability_lines(card) {
        if let Some(rest) = line.strip_prefix("partner with ") {
            let name = rest.strip_suffix('.').unwrap_or(rest).trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

const NUMBER_WORDS: &[(&str, i64)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
];

/// A card's own cap on copies, independent of its format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyLimit {
    /// "A deck can have any number of cards named …", and every basic land.
    Unlimited,
    Cards(i64),
}

impl CopyLimit {
    fn permits(self, copies: i64) -> bool {
        match self {
            CopyLimit::Unlimited => true,
            CopyLimit::Cards(limit) => copies <= limit,
        }
    }
}

/// How many copies of this card a deck may hold *regardless* of its format, or `None` when
/// the format's own limit applies. Basic lands are unbounded, and so is the "A deck can
/// have any number of cards named …" cycle (Relentless Rats, Shadowborn Apostle, Dragon's
/// Approach, …) — including in singleton formats, where rule 903.5b lets that text override
/// the one-copy rule. Seven Dwarves and Nazgûl name their own cap ("up to seven/nine"),
/// read from the same sentence rather than hard-coded, so a future card in either cycle
/// needs no code change.
pub(crate) fn card_copy_limit(card: &CardFacts) -> Option<CopyLimit> {
    if is_basic_land(card) {
        return Some(CopyLimit::Unlimited);
    }
    let text = card.oracle_text.to_lowercase();
    if text.contains("a deck can have any number of cards named") {
        return Some(CopyLimit::Unlimited);
    }
    let prefix = "a deck can have up to ";
    let index = text.find(prefix)?;
    let rest = &text[index + prefix.len()..];
    let word: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if word.is_empty() || !rest[word.len()..].starts_with(" cards named") {
        return None;
    }
    NUMBER_WORDS
        .iter()
        .find(|(spelling, _)| *spelling == word)
        .map(|(_, count)| CopyLimit::Cards(*count))
}

// ---------- Format rule table ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandZoneKind {
    Commander,
    Brawl,
    Pdh,
    Oathbreaker,
}

/// How a format's command zone works. `noun` names its card in messages.
#[derive(Clone, Copy, Debug)]
struct CommandZoneRule {
    kind: CommandZoneKind,
    noun: &'static str,
    /// Whether Partner / Friends forever / a Background can make the zone a pair.
    allow_pairs: bool,
}

/// Cards in the deck proper, command zone included: an exact count or a floor.
#[derive(Clone, Copy, Debug)]
enum DeckSize {
    Exact(i64),
    Min(i64),
}

#[derive(Clone, Copy, Debug)]
struct FormatRules {
    size: DeckSize,
    /// Copies of one card name across the deck, sideboard included (1 = singleton).
    max_copies: i64,
    /// Cards allowed in sideboard sections; `None` when the format has no sideboard.
    max_sideboard: Option<i64>,
    command_zone: Option<CommandZoneRule>,
}

const COMMANDER_ZONE: CommandZoneRule = CommandZoneRule {
    kind: CommandZoneKind::Commander,
    noun: "commander",
    allow_pairs: true,
};
// Brawl leads with a legendary creature *or* planeswalker. Arena's Brawl queues have
// followed paper on Partner/Background, so pairs are allowed here too — being permissive
// keeps a legal deck from being called illegal.
const BRAWL_ZONE: CommandZoneRule = CommandZoneRule {
    kind: CommandZoneKind::Brawl,
    noun: "commander",
    allow_pairs: true,
};
// Pauper Commander leads with an *uncommon creature* — legendary is not required, and most
// PDH commanders aren't. The rarity half of that rule needs no check here: the provider
// marks the eligible uncommons `restricted` in `paupercommander`, which the per-card check
// reads as "legal only as the commander".
const PDH_ZONE: CommandZoneRule = CommandZoneRule {
    kind: CommandZoneKind::Pdh,
    noun: "commander",
    allow_pairs: true,
};
const OATHBREAKER_ZONE: CommandZoneRule = CommandZoneRule {
    kind: CommandZoneKind::Oathbreaker,
    noun: "oathbreaker",
    allow_pairs: false,
};

const CONSTRUCTED: FormatRules = FormatRules {
    size: DeckSize::Min(60),
    max_copies: 4,
    max_sideboard: Some(15),
    command_zone: None,
};
const EDH: FormatRules = FormatRules {
    size: DeckSize::Exact(100),
    max_copies: 1,
    max_sideboard: None,
    command_zone: Some(COMMANDER_ZONE),
};

const fn singleton(size: i64, zone: Option<CommandZoneRule>) -> FormatRules {
    FormatRules {
        size: DeckSize::Exact(size),
        max_copies: 1,
        max_sideboard: None,
        command_zone: zone,
    }
}

/// Construction rules per legality key. A format absent from this table is evaluated on its
/// per-card legality alone — the deck-wide checks simply don't run, which is why an
/// unsupported format can never produce a wrong "illegal" verdict.
fn format_rules(key: &str) -> Option<FormatRules> {
    Some(match key {
        "standard" | "pioneer" | "modern" | "legacy" | "vintage" | "pauper" | "alchemy"
        | "historic" | "timeless" | "penny" | "premodern" | "oldschool" => CONSTRUCTED,
        "commander" | "duel" | "predh" => EDH,
        "paupercommander" => singleton(100, Some(PDH_ZONE)),
        "brawl" => singleton(100, Some(BRAWL_ZONE)),
        // Both Arena Brawl queues that build off Standard are 60-card decks; the 100-card
        // variant is `brawl` above (Historic/Timeless Brawl).
        "standardbrawl" | "competitivebrawl" => singleton(60, Some(BRAWL_ZONE)),
        // 100-card singleton highlander with no command zone.
        "gladiator" => singleton(100, None),
        // 58 cards + the oathbreaker planeswalker + its signature spell.
        "oathbreaker" => singleton(60, Some(OATHBREAKER_ZONE)),
        _ => return None,
    })
}

// ---------- Command-zone eligibility ----------

/// Whether a card may lead a deck in this kind of command zone.
fn can_lead(card: &CardFacts, kind: CommandZoneKind) -> bool {
    if kind == CommandZoneKind::Oathbreaker {
        return has_type(card, &["legendary", "planeswalker"]);
    }
    if kind == CommandZoneKind::Pdh {
        return has_type(card, &["creature"]);
    }
    if has_type(card, &["legendary", "creature"]) {
        return true;
    }
    // "can be your commander" covers the designed-for-the-zone planeswalkers and oddities;
    // a Background is only ever a commander (paired with "Choose a Background").
    if ability_lines(card)
        .iter()
        .any(|line| line.contains("can be your commander"))
    {
        return true;
    }
    if has_type(card, &["background"]) {
        return true;
    }
    // Rule 903.3a reads the card's characteristics *outside* the battlefield, so a
    // legendary card its own text turns into a creature everywhere but there (Grist, the
    // Hunger Tide) leads a deck even though its printed front face is a planeswalker.
    if has_type(card, &["legendary"])
        && ability_lines(card)
            .iter()
            .any(|line| line.contains("isn't on the battlefield") && line.contains("creature"))
    {
        return true;
    }
    kind == CommandZoneKind::Brawl && has_type(card, &["legendary", "planeswalker"])
}

/// Whether two cards may share a command zone (Partner and its cousins).
fn pair_allowed(left: &CardFacts, right: &CardFacts) -> bool {
    let partnered =
        |card: &CardFacts| has_ability(card, "partner") && partner_with_name(card).is_none();
    if partnered(left) && partnered(right) {
        return true;
    }
    let named = |from: &CardFacts, to: &CardFacts| {
        partner_with_name(from).is_some_and(|name| name == to.name.to_lowercase())
    };
    if named(left, right) || named(right, left) {
        return true;
    }
    if has_ability(left, "friends forever") && has_ability(right, "friends forever") {
        return true;
    }
    let pairs = |ability: &str, test: &dyn Fn(&CardFacts) -> bool| {
        (has_ability(left, ability) && test(right)) || (has_ability(right, ability) && test(left))
    };
    if pairs("doctor's companion", &|card| {
        has_type(card, &["time lord doctor"])
    }) {
        return true;
    }
    if pairs("choose a background", &|card| {
        has_type(card, &["background"])
    }) {
        return true;
    }
    false
}

// ---------- Evaluation ----------

/// A per-card breach the construction rules found (the deck views chip these on tiles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckRuleCardStatus {
    OffColour,
    OverLimit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeckRuleCardIssue {
    pub card_id: String,
    pub name: String,
    pub status: DeckRuleCardStatus,
    /// Total copies of that name in the deck.
    pub quantity: i64,
}

/// Which construction rule a violation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum DeckRuleId {
    DeckSize,
    SideboardSize,
    CommandZone,
    CommanderEligibility,
    ColourIdentity,
}

/// How serious a violation is. `Error` = illegal as it stands; `Warning` = simply not
/// finished being built yet — a half-built deck must never be reported as illegal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckRuleSeverity {
    Error,
    Warning,
}

/// One deck-wide construction breach, with a ready-to-render sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckRuleViolation {
    pub rule: DeckRuleId,
    pub severity: DeckRuleSeverity,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DeckRuleResult {
    pub violations: Vec<DeckRuleViolation>,
    pub card_issues: Vec<DeckRuleCardIssue>,
}

const COLOUR_ORDER: &[&str] = &["W", "U", "B", "R", "G"];

/// Colour identity as its mana symbols in WUBRG order, or "colourless".
fn identity_label(identity: &[String]) -> String {
    if identity.is_empty() {
        return "colourless".to_string();
    }
    COLOUR_ORDER
        .iter()
        .filter(|colour| identity.iter().any(|held| held == *colour))
        .map(|colour| format!("{{{colour}}}"))
        .collect::<Vec<_>>()
        .join("")
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        _ => format!(
            "{} and {}",
            names[..names.len() - 1].join(", "),
            names[names.len() - 1]
        ),
    }
}

/// Distinct values in first-seen order — the ordering the browser's `Set` gave these
/// messages, and what makes "A and B" read in the order the cards were listed.
fn unique_in_order(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = Vec::new();
    for value in values {
        if !seen.contains(&value) {
            seen.push(value);
        }
    }
    seen
}

/// One card name folded across every section and printing it appears in.
struct NameFold<'a> {
    facts: &'a CardFacts,
    card_ids: Vec<String>,
    copies: i64,
}

/// Judge a deck against its format's construction rules. `entries` must already be the deck
/// proper (maybeboard excluded, like everything else that answers "what is this deck"), and
/// `sections` supplies the names the zone split reads.
///
/// Returns no violations at all when the format has no rule profile or the deck is empty —
/// there is nothing useful to say about either.
pub(crate) fn evaluate_deck_rules(
    format_key: &str,
    entries: &[&AnalysisEntry],
    sections: &[DeckSectionResponse],
) -> DeckRuleResult {
    let Some(rules) = format_rules(format_key) else {
        return DeckRuleResult::default();
    };
    if entries.is_empty() {
        return DeckRuleResult::default();
    }

    let zone_by_id: HashMap<i32, DeckZone> = sections
        .iter()
        .map(|section| (section.id, deck_zone(&section.name)))
        .collect();
    let mut violations: Vec<DeckRuleViolation> = Vec::new();
    let mut card_issues: Vec<DeckRuleCardIssue> = Vec::new();

    // Pass 1: split the deck into zones and fold copies by name (a name's total spans every
    // section and printing, so 3 of one art plus 2 of another is five copies). Insertion
    // order is preserved so the issue list reads in the deck's own card order.
    // One entry per command-zone **row** with its copy count — never one per copy. A row's
    // count is caller-controlled up to a million per finish, so expanding it here would let
    // one `GET` allocate (and then linearly scan) an arbitrarily large vector. Only the
    // count matters until it is small enough to reason about card-by-card, and the two
    // places that need the cards expand it themselves, under a bound.
    let mut commanders: Vec<(&CardFacts, i64)> = Vec::new();
    let mut commander_count = 0i64;
    let mut folds: Vec<NameFold> = Vec::new();
    let mut fold_index: HashMap<&str, usize> = HashMap::new();
    let mut deck_copies = 0i64;
    let mut sideboard_copies = 0i64;
    for entry in entries {
        let copies = entry.copies();
        if copies == 0 {
            continue;
        }
        let zone = zone_by_id
            .get(&entry.section_id)
            .copied()
            .unwrap_or(DeckZone::Main);
        if zone == DeckZone::Command && rules.command_zone.is_some() {
            commanders.push((&entry.facts, copies));
            commander_count += copies;
        }
        // A command-zone section in a format without one (a Modern deck still gets the
        // seeded `Commander` section) is just part of the deck.
        if zone == DeckZone::Sideboard {
            sideboard_copies += copies;
        } else {
            deck_copies += copies;
        }

        match fold_index.get(entry.facts.name.as_str()) {
            Some(&index) => {
                folds[index].card_ids.push(entry.facts.id.clone());
                folds[index].copies += copies;
            }
            None => {
                fold_index.insert(entry.facts.name.as_str(), folds.len());
                folds.push(NameFold {
                    facts: &entry.facts,
                    card_ids: vec![entry.facts.id.clone()],
                    copies,
                });
            }
        }
    }

    // ---- Deck size ----
    let (exact, required) = match rules.size {
        DeckSize::Exact(count) => (Some(count), count),
        DeckSize::Min(count) => (None, count),
    };
    if deck_copies > 0 && deck_copies < required {
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::DeckSize,
            severity: DeckRuleSeverity::Warning,
            message: format!(
                "{deck_copies} of {required} cards — {} to go.",
                required - deck_copies
            ),
        });
    } else if let Some(exact) = exact
        && deck_copies > exact
    {
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::DeckSize,
            severity: DeckRuleSeverity::Error,
            message: format!(
                "{deck_copies} cards — {} over the {exact}-card limit.",
                deck_copies - exact
            ),
        });
    }
    if let Some(max_sideboard) = rules.max_sideboard
        && sideboard_copies > max_sideboard
    {
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::SideboardSize,
            severity: DeckRuleSeverity::Error,
            message: format!(
                "{sideboard_copies} cards in the sideboard — the limit is {max_sideboard}."
            ),
        });
    }

    // ---- Copy limit ----
    for fold in &folds {
        let limit = card_copy_limit(fold.facts).unwrap_or(CopyLimit::Cards(rules.max_copies));
        if limit.permits(fold.copies) {
            continue;
        }
        for card_id in &fold.card_ids {
            card_issues.push(DeckRuleCardIssue {
                card_id: card_id.clone(),
                name: fold.facts.name.clone(),
                status: DeckRuleCardStatus::OverLimit,
                quantity: fold.copies,
            });
        }
    }

    // ---- Command zone ----
    if let Some(zone) = rules.command_zone {
        violations.extend(command_zone_violations(&zone, &commanders, commander_count));
        // Colour identity: the 99 may not stray outside the command zone's combined
        // identity. Skipped while the zone is empty — an unbuilt deck isn't off-colour.
        if commander_count > 0 {
            let identity: Vec<String> = unique_in_order(
                commanders
                    .iter()
                    .flat_map(|(card, _)| card.color_identity.iter().cloned()),
            );
            // By name, not by row: the commander's own identity defines the deck's, and a
            // second printing of it in the 99 is a copy-limit matter, not an off-colour one.
            let commander_names: Vec<&str> = commanders
                .iter()
                .map(|(card, _)| card.name.as_str())
                .collect();
            let off_colour: Vec<&NameFold> = folds
                .iter()
                .filter(|fold| {
                    !commander_names.contains(&fold.facts.name.as_str())
                        && fold
                            .facts
                            .color_identity
                            .iter()
                            .any(|colour| !identity.contains(colour))
                })
                .collect();
            for fold in &off_colour {
                for card_id in &fold.card_ids {
                    card_issues.push(DeckRuleCardIssue {
                        card_id: card_id.clone(),
                        name: fold.facts.name.clone(),
                        status: DeckRuleCardStatus::OffColour,
                        quantity: fold.copies,
                    });
                }
            }
            if !off_colour.is_empty() {
                let names = join_names(&unique_in_order(
                    commanders.iter().map(|(card, _)| card.name.clone()),
                ));
                let count = off_colour.len();
                let falls = if count == 1 {
                    "card falls"
                } else {
                    "cards fall"
                };
                violations.push(DeckRuleViolation {
                    rule: DeckRuleId::ColourIdentity,
                    severity: DeckRuleSeverity::Error,
                    message: format!(
                        "{count} {falls} outside {names}'s colour identity ({}).",
                        identity_label(&identity)
                    ),
                });
            }
        }
    }

    DeckRuleResult {
        violations,
        card_issues,
    }
}

/// The command zone's own rules: how many cards it holds, and whether they may lead.
///
/// `commanders` is one entry per **row** (`(card, copies)`) and `count` is their total, so
/// a row claiming a million copies costs one entry rather than a million. Every branch that
/// needs the cards *individually* only runs once the count is down to two, so it expands
/// there — under a bound it has already checked.
fn command_zone_violations(
    zone: &CommandZoneRule,
    commanders: &[(&CardFacts, i64)],
    count: i64,
) -> Vec<DeckRuleViolation> {
    let mut violations = Vec::new();
    // The zone's cards, one per copy. Only ever called where `count <= 2`.
    let expand = || -> Vec<&CardFacts> {
        commanders
            .iter()
            .flat_map(|(card, copies)| std::iter::repeat_n(*card, *copies as usize))
            .collect()
    };

    if zone.kind == CommandZoneKind::Oathbreaker {
        // Two cards, and they must be one of each: the planeswalker and its signature spell.
        if count < 2 {
            violations.push(DeckRuleViolation {
                rule: DeckRuleId::CommandZone,
                severity: DeckRuleSeverity::Warning,
                message: "No oathbreaker and signature spell — put a legendary planeswalker \
                          and one instant or sorcery in a section named \"Oathbreaker\"."
                    .to_string(),
            });
            return violations;
        }
        if count > 2 {
            violations.push(DeckRuleViolation {
                rule: DeckRuleId::CommandZone,
                severity: DeckRuleSeverity::Error,
                message: format!(
                    "{count} cards in the command zone — an Oathbreaker deck has one \
                     oathbreaker and one signature spell."
                ),
            });
            return violations;
        }
        let pair = expand();
        let walkers = pair
            .iter()
            .filter(|card| can_lead(card, CommandZoneKind::Oathbreaker))
            .count();
        let spells = pair
            .iter()
            .filter(|card| has_type(card, &["instant"]) || has_type(card, &["sorcery"]))
            .count();
        if walkers != 1 || spells != 1 {
            violations.push(DeckRuleViolation {
                rule: DeckRuleId::CommanderEligibility,
                severity: DeckRuleSeverity::Error,
                message: format!(
                    "{} can't lead the deck — an Oathbreaker deck needs exactly one legendary \
                     planeswalker and one instant or sorcery as its signature spell.",
                    join_names(
                        &pair
                            .iter()
                            .map(|card| card.name.clone())
                            .collect::<Vec<_>>()
                    )
                ),
            });
        }
        return violations;
    }

    if count == 0 {
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::CommandZone,
            severity: DeckRuleSeverity::Warning,
            message: format!(
                "No {} — put one in a section named \"Commander\".",
                zone.noun
            ),
        });
        return violations;
    }

    let max_commanders = if zone.allow_pairs { 2 } else { 1 };
    if count > max_commanders {
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::CommandZone,
            severity: DeckRuleSeverity::Error,
            message: if zone.allow_pairs {
                format!(
                    "{count} cards in the command zone — a deck has one {}, or two that pair.",
                    zone.noun
                )
            } else {
                format!(
                    "{count} cards in the command zone — a deck has one {}.",
                    zone.noun
                )
            },
        });
    } else if count == 2
        && let pair = expand()
        && !pair_allowed(pair[0], pair[1])
    {
        // Two copies of one card is a different mistake from two cards that don't pair, and
        // "X and X can't be commanders together" would read as nonsense.
        let (first, second) = (pair[0], pair[1]);
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::CommandZone,
            severity: DeckRuleSeverity::Error,
            message: if first.name == second.name {
                format!(
                    "Two copies of {} in the command zone — a deck has one {}.",
                    first.name, zone.noun
                )
            } else {
                format!(
                    "{} can't be commanders together — a pair needs Partner, Friends forever, \
                     Doctor's companion, or Choose a Background.",
                    join_names(&[first.name.clone(), second.name.clone()])
                )
            },
        });
    }

    // By row: a name is reported once however many copies of it are in the zone, which is
    // what `unique_in_order` below already collapsed it to.
    let ineligible: Vec<String> = commanders
        .iter()
        .filter(|(card, _)| !can_lead(card, zone.kind))
        .map(|(card, _)| card.name.clone())
        .collect();
    if !ineligible.is_empty() {
        let names = join_names(&unique_in_order(ineligible));
        let allowed = match zone.kind {
            CommandZoneKind::Brawl => "a legendary creature or planeswalker",
            CommandZoneKind::Pdh => "an uncommon creature",
            _ => "a legendary creature (or a card that says it can be your commander)",
        };
        violations.push(DeckRuleViolation {
            rule: DeckRuleId::CommanderEligibility,
            severity: DeckRuleSeverity::Error,
            message: format!(
                "{names} can't be your {} — a {} must be {allowed}.",
                zone.noun, zone.noun
            ),
        });
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{card, entry, section};

    #[test]
    fn zones_come_from_section_names() {
        assert_eq!(deck_zone("Commander"), DeckZone::Command);
        assert_eq!(deck_zone("  commanders "), DeckZone::Command);
        assert_eq!(deck_zone("Command Zone"), DeckZone::Command);
        assert_eq!(deck_zone("Signature Spell"), DeckZone::Command);
        assert_eq!(deck_zone("Sideboard"), DeckZone::Sideboard);
        assert_eq!(deck_zone("Companion"), DeckZone::Sideboard);
        assert_eq!(deck_zone("Creatures"), DeckZone::Main);
        assert_eq!(deck_zone("Ramp"), DeckZone::Main);
    }

    #[test]
    fn word_matching_respects_boundaries() {
        assert!(has_word("legendary creature — human", "creature"));
        assert!(!has_word("creatureland", "creature"));
        assert!(has_word("artifact land", "land"));
        assert!(!has_word("island", "land"));
    }

    #[test]
    fn reminder_text_never_satisfies_a_keyword() {
        // Every Partner card's reminder text names Partner; only the ability line counts.
        let reminder = card("x", "Not A Partner")
            .oracle("Flying\n(Partner means you can have two commanders.)");
        assert!(!has_ability(&reminder, "partner"));
        let real = card("y", "Real").oracle("Partner\nFlying");
        assert!(has_ability(&real, "partner"));
        let clause = card("z", "Clause").oracle("Partner—Survivors");
        assert!(has_ability(&clause, "partner"));
    }

    #[test]
    fn reads_partner_with_off_the_ability_line() {
        let pir = card("a", "Pir").oracle("Partner with Toothy, Imaginary Friend\nFlying");
        assert_eq!(
            partner_with_name(&pir).as_deref(),
            Some("toothy, imaginary friend")
        );
        assert_eq!(
            partner_with_name(&card("b", "Plain").oracle("Flying")),
            None
        );
    }

    #[test]
    fn copy_limits_read_the_card_not_a_list() {
        assert_eq!(
            card_copy_limit(&card("a", "Forest").type_line("Basic Land — Forest")),
            Some(CopyLimit::Unlimited)
        );
        assert_eq!(
            card_copy_limit(
                &card("b", "Relentless Rats")
                    .oracle("A deck can have any number of cards named Relentless Rats.")
            ),
            Some(CopyLimit::Unlimited)
        );
        assert_eq!(
            card_copy_limit(
                &card("c", "Seven Dwarves")
                    .oracle("A deck can have up to seven cards named Seven Dwarves.")
            ),
            Some(CopyLimit::Cards(7))
        );
        assert_eq!(
            card_copy_limit(&card("d", "Bear").type_line("Creature")),
            None
        );
    }

    #[test]
    fn an_unknown_format_is_never_judged() {
        let rows = [entry("a", "A", 1, 1, 0)];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("cube", &refs, &[section(1, "Main", false)]);
        assert!(result.violations.is_empty());
        assert!(result.card_issues.is_empty());
    }

    #[test]
    fn an_unfinished_deck_warns_and_is_never_an_error() {
        let sections = [section(1, "Commander", false), section(2, "Lands", false)];
        let rows = [
            entry("cmd", "Atraxa", 1, 1, 0)
                .type_line("Legendary Creature — Phyrexian Angel Horror")
                .colors("W,U,B,G"),
            entry("l", "Forest", 2, 10, 0).type_line("Basic Land — Forest"),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        let size = result
            .violations
            .iter()
            .find(|v| v.rule == DeckRuleId::DeckSize)
            .expect("a short deck reports its progress");
        assert_eq!(size.severity, DeckRuleSeverity::Warning);
        assert_eq!(size.message, "11 of 100 cards — 89 to go.");
        assert!(result.card_issues.is_empty());
    }

    #[test]
    fn an_over_size_deck_is_an_error() {
        let sections = [section(1, "Lands", false)];
        let rows = [entry("l", "Forest", 1, 101, 0).type_line("Basic Land — Forest")];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        let size = &result.violations[0];
        assert_eq!(size.severity, DeckRuleSeverity::Error);
        assert_eq!(size.message, "101 cards — 1 over the 100-card limit.");
    }

    #[test]
    fn a_constructed_deck_is_judged_on_size_copies_and_its_sideboard() {
        // The only coverage of the CONSTRUCTED profile, and the only assertion anywhere that
        // the 15-card sideboard cap exists: `DeckZone::Sideboard` is reachable solely through
        // a section *name*, so nothing else in the suite ever files a card into one.
        let sections = [
            section(1, "Creatures", false),
            section(2, "Sideboard", false),
        ];
        let rows = [
            entry("bolt", "Lightning Bolt", 1, 5, 0).type_line("Instant"),
            entry("sb", "Pyroblast", 2, 16, 0).type_line("Instant"),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("modern", &refs, &sections);

        // 5 cards in the deck proper — the sideboard is beside the deck, not in it.
        let size = result
            .violations
            .iter()
            .find(|v| v.rule == DeckRuleId::DeckSize)
            .expect("a 5-card Modern deck is short of 60");
        assert_eq!(size.severity, DeckRuleSeverity::Warning);
        assert_eq!(size.message, "5 of 60 cards — 55 to go.");

        let sideboard = result
            .violations
            .iter()
            .find(|v| v.rule == DeckRuleId::SideboardSize)
            .expect("16 cards is over the 15-card sideboard cap");
        assert_eq!(sideboard.severity, DeckRuleSeverity::Error);
        assert_eq!(
            sideboard.message,
            "16 cards in the sideboard — the limit is 15."
        );

        // Four copies is the constructed limit; five is not, and the sideboard's 16 of one
        // name breaches it too (the copy limit spans the sideboard).
        assert_eq!(
            result
                .card_issues
                .iter()
                .filter(|i| i.status == DeckRuleCardStatus::OverLimit)
                .count(),
            2
        );
        // A format with no command zone never reports one, even though the deck has no
        // commander at all.
        assert!(
            !result
                .violations
                .iter()
                .any(|v| v.rule == DeckRuleId::CommandZone)
        );
    }

    #[test]
    fn a_command_zone_row_is_counted_not_expanded() {
        // A row's copy count is caller-controlled up to a million per finish. The zone must
        // report the true total without ever materialising one entry per copy — this would
        // allocate ~16 MB and take seconds if it did.
        let sections = [section(1, "Commander", false)];
        let rows = [entry("a", "Krenko", 1, 1_000_000, 0)
            .type_line("Legendary Creature — Goblin")
            .colors("R")];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert!(
            result.violations.iter().any(|v| v.message
                == "1000000 cards in the command zone — a deck has one commander, or two that pair."),
            "got {:?}",
            result.violations
        );
    }

    #[test]
    fn the_singleton_limit_catches_a_second_copy_across_printings() {
        let sections = [section(1, "Ramp", false)];
        let rows = [
            entry("sr-a", "Sol Ring", 1, 1, 0).type_line("Artifact"),
            entry("sr-b", "Sol Ring", 1, 1, 0).type_line("Artifact"),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert_eq!(result.card_issues.len(), 2, "one chip per printing");
        assert!(
            result
                .card_issues
                .iter()
                .all(|i| i.status == DeckRuleCardStatus::OverLimit && i.quantity == 2)
        );
    }

    #[test]
    fn off_colour_cards_are_named_against_the_commanders_identity() {
        let sections = [section(1, "Commander", false), section(2, "Spells", false)];
        let rows = [
            entry("cmd", "Krenko", 1, 1, 0)
                .type_line("Legendary Creature — Goblin Warrior")
                .colors("R"),
            entry("bad", "Counterspell", 2, 1, 0)
                .type_line("Instant")
                .colors("U"),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        let colour = result
            .violations
            .iter()
            .find(|v| v.rule == DeckRuleId::ColourIdentity)
            .expect("an off-colour card is reported");
        assert_eq!(
            colour.message,
            "1 card falls outside Krenko's colour identity ({R})."
        );
        assert!(
            result
                .card_issues
                .iter()
                .any(|i| i.card_id == "bad" && i.status == DeckRuleCardStatus::OffColour)
        );
    }

    #[test]
    fn two_commanders_need_a_reason_to_pair() {
        let sections = [section(1, "Commander", false)];
        let strangers = [
            entry("a", "Krenko", 1, 1, 0)
                .type_line("Legendary Creature — Goblin")
                .colors("R"),
            entry("b", "Talrand", 1, 1, 0)
                .type_line("Legendary Creature — Merfolk")
                .colors("U"),
        ];
        let refs: Vec<&AnalysisEntry> = strangers.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.rule == DeckRuleId::CommandZone
                    && v.message.contains("can't be commanders together"))
        );

        let partners = [
            entry("a", "Thrasios", 1, 1, 0)
                .type_line("Legendary Creature — Merfolk Wizard")
                .oracle("Partner\n{4}: Scry 1, then draw a card.")
                .colors("G,U"),
            entry("b", "Tymna", 1, 1, 0)
                .type_line("Legendary Creature — Human Cleric")
                .oracle("Partner\nFirst strike")
                .colors("W,B"),
        ];
        let refs: Vec<&AnalysisEntry> = partners.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert!(
            !result
                .violations
                .iter()
                .any(|v| v.rule == DeckRuleId::CommandZone),
            "two Partner commanders pair"
        );
    }

    #[test]
    fn two_copies_of_one_commander_reads_as_a_duplicate() {
        let sections = [section(1, "Commander", false)];
        let rows = [entry("a", "Krenko", 1, 2, 0)
            .type_line("Legendary Creature — Goblin")
            .colors("R")];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert!(
            result.violations.iter().any(|v| v.message
                == "Two copies of Krenko in the command zone — a deck has one commander.")
        );
    }

    #[test]
    fn a_card_that_cannot_lead_is_reported() {
        let sections = [section(1, "Commander", false)];
        let rows = [entry("a", "Sol Ring", 1, 1, 0).type_line("Artifact")];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("commander", &refs, &sections);
        assert_eq!(
            result
                .violations
                .iter()
                .find(|v| v.rule == DeckRuleId::CommanderEligibility)
                .map(|v| v.message.as_str()),
            Some(
                "Sol Ring can't be your commander — a commander must be a legendary creature \
                 (or a card that says it can be your commander)."
            )
        );
    }

    #[test]
    fn a_legendary_that_is_a_creature_off_the_battlefield_may_lead() {
        let grist = card("g", "Grist, the Hunger Tide")
            .type_line("Legendary Planeswalker — Grist")
            .oracle(
                "As long as Grist, the Hunger Tide isn't on the battlefield, it's a 1/1 \
                 Insect creature in addition to its other types.",
            );
        assert!(can_lead(&grist, CommandZoneKind::Commander));
    }

    #[test]
    fn oathbreaker_needs_a_walker_and_a_spell() {
        let sections = [section(1, "Oathbreaker", false)];
        let rows = [
            entry("w", "Teferi", 1, 1, 0)
                .type_line("Legendary Planeswalker — Teferi")
                .colors("W,U"),
            entry("s", "Time Warp", 1, 1, 0)
                .type_line("Sorcery")
                .colors("U"),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let result = evaluate_deck_rules("oathbreaker", &refs, &sections);
        assert!(
            !result
                .violations
                .iter()
                .any(|v| v.rule == DeckRuleId::CommanderEligibility)
        );
    }
}
