//! The deck's **mana base** — the colour pips its spells ask for against the sources its
//! library can produce, judged by Frank Karsten's source counts (issue #670).
//!
//! "Do I have enough blue sources to cast `UU` on turn two?" is the single most-asked deck
//! question, and the two inputs it needs are already on every catalog row: the printed
//! `mana_cost` and Scryfall's `produced_mana`. This module folds the deck once into a
//! per-colour ledger — pips demanded, sources present, the number Karsten says a deck this
//! size needs for its most colour-hungry spell — and words the verdict ("short 2 black
//! sources"). Every counted card rides the response, the way the bracket lists its Game
//! Changers, so the number can be audited rather than trusted.
//!
//! **The thresholds are a citation, not a model.** [`KARSTEN_2022`] is the summary table
//! from Frank Karsten's *How Many Sources Do You Need to Consistently Cast Your Spells? A
//! 2022 Update* (TCGplayer Infinite, 2022): the fewest sources of one colour that cast a
//! spell on curve at least `89 + M`% of the time (90% for a one-drop, up to 96% for a
//! seven-drop), on the play, with London mulligans, conditional on drawing enough lands, in
//! a deck with a typical land count (17 / 25 / 35 / 41 for 40 / 60 / 80 / 99 cards), and —
//! for the 99-card column — with Commander's free mulligan and first-turn draw. His
//! simulation is his; what lives here is the published result, so a number in the response
//! can be checked against the article line by line. **One** rule of thumb is his and is
//! applied verbatim: a **multicoloured** card's requirement goes up by one for each of its
//! colours ("for gold cards … increase all requirements by one"). Three more are this
//! module's own conservative simplifications, *not* the article's, and every response says
//! so in its caveats: a mana value beyond a pip group's last row is judged as that row (the
//! table stops at seven and the article says nothing past it — an over-estimate, since the
//! numbers fall as the turn rises); a **hybrid**, Phyrexian or `{2/C}` pip is reported but
//! never counted against a colour — deliberately *narrower* than Karsten, who asks for the
//! table number in *combined* sources across a hybrid's colours ("for hybrid spells, you need
//! to have enough combined sources of either color"), a union requirement this read reports
//! (`hybrid_pips`) but does not compute, and Phyrexian and twobrid costs the article never
//! discusses; and an **X** spell is listed but never sets a colour's number, because Karsten's
//! advice for one — judge it by "the typical amount of lands that you expect to tap to cast
//! the spell" — is a fact only the player has. Everything he weighs fractionally — a tapped
//! land, a fetch, a cantrip, a mana rock — counts here as one source, which is why the
//! caveats say so.
//!
//! **Zones are [`super::rules`]'s answers, never a second list of names.** Which sections are
//! the command zone is [`super::rules::deck_zone`]'s; whether that zone *leads* the deck is
//! [`super::rules::format_leads_with_command_zone`]'s — the same pair the deck list's facets
//! borrow. Demand is the deck a player casts from — the shuffled library **and** the command
//! zone (a commander's own pips are the first thing to check) — and supply is the library
//! alone: a commander is never a source for the 99, however many colours it produces. In a
//! format with no command zone (every deck is seeded with a `Commander` section, so a Modern
//! deck can easily have cards in one) those cards are just part of the 60, as the legality
//! rules already treat them, so they supply mana like any other library card. Maybeboards are
//! out of both (issue #570); a sideboard is out of both too, since nothing in it is cast on
//! curve from the main deck.
//!
//! **Nothing here goes per copy.** Pips and sources are copy-weighted by multiplication over
//! a per-name fold ([`super::fold_by_name`], shared with the bracket), so a deck row's counts —
//! which are caller-controlled — never enter a loop bound.
//!
//! **NULL is not `[]`.** `cards.produced_mana` is NULL on any row not rewritten since the
//! ingest started storing "produces nothing" as an empty string (see
//! [`crate::scryfall::map`]); such a card is reported in `unchecked_count` rather than read as
//! a non-source, the stance the tokens read takes on `token_parts`. Until a bulk import has
//! rewritten the row it says nothing either way, and a verdict built on it would be a
//! confident wrong answer.

use std::collections::BTreeMap;

use serde::Serialize;

use super::rules::{DeckZone, deck_zone, format_deck_size, format_leads_with_command_zone};
use super::stats::type_words;
use super::{DeckAnalysisInput, NameFold, fold_by_name};

/// Where the thresholds come from, on the wire so a client can cite it beside the number.
const KARSTEN_SOURCE: &str = "Frank Karsten, \"How Many Sources Do You Need to Consistently \
                              Cast Your Spells? A 2022 Update\" (TCGplayer Infinite, 2022)";

/// Demanding cards listed per colour. The counts stay exact — a deck's row count is
/// caller-controlled and this response isn't paginated, so the lists are capped the way the
/// bracket caps its counted cards.
const MAX_LISTED_DEMAND: usize = 10;

/// Sources listed per colour: a 99-card deck's forty lands fit, a pathological deck's don't.
const MAX_LISTED_SOURCES: usize = 50;

// ---------- Karsten's table ----------

/// The deck sizes the table has a column for, in the order the columns are stored.
const TABLE_SIZES: [i64; 4] = [60, 80, 99, 40];

/// Karsten's 2022 summary table: `(cost, mana value, pips of the colour, sources needed in a
/// [60, 80, 99, 40]-card deck)`. Copied verbatim from the article's opening table, where
/// `C` stands for any one colour; the cost key is what the response echoes so a reader can
/// find the row. Rows are grouped by pip count and ascend by mana value within a group,
/// which [`threshold`] relies on.
///
/// Source: Frank Karsten, "How Many Sources Do You Need to Consistently Cast Your Spells? A
/// 2022 Update", TCGplayer Infinite, 2022 — the 90%-on-curve (rising to 96% for seven-drops)
/// numbers, on the play, London mulligans, 17/25/35/41 lands, Commander's free mulligan and
/// first-turn draw for the 99-card column.
const KARSTEN_2022: &[(&str, i64, i64, [i64; 4])] = &[
    ("C", 1, 1, [14, 19, 19, 9]),
    ("1C", 2, 1, [13, 18, 19, 9]),
    ("2C", 3, 1, [12, 16, 18, 8]),
    ("3C", 4, 1, [10, 15, 16, 7]),
    ("4C", 5, 1, [9, 14, 15, 6]),
    ("5C", 6, 1, [9, 12, 14, 6]),
    ("CC", 2, 2, [21, 28, 30, 14]),
    ("1CC", 3, 2, [18, 25, 28, 12]),
    ("2CC", 4, 2, [16, 23, 26, 11]),
    ("3CC", 5, 2, [15, 20, 23, 10]),
    ("4CC", 6, 2, [13, 19, 22, 9]),
    ("5CC", 7, 2, [12, 17, 20, 8]),
    ("CCC", 3, 3, [23, 32, 36, 16]),
    ("1CCC", 4, 3, [21, 29, 33, 14]),
    ("2CCC", 5, 3, [19, 26, 30, 13]),
    ("3CCC", 6, 3, [17, 24, 28, 11]),
    ("4CCC", 7, 3, [16, 22, 26, 10]),
    ("CCCC", 4, 4, [24, 34, 39, 17]),
    ("1CCCC", 5, 4, [22, 31, 36, 15]),
];

/// The table's most demanding pip count; a spell with more pips of one colour is judged as
/// this many — a floor, and the caveats say so.
const MAX_TABLE_PIPS: i64 = 4;

/// The table column a deck is judged against: the size nearest to `size`, ties going to the
/// larger deck, whose numbers are the higher (safer) floor.
fn table_size_for(size: i64) -> i64 {
    let mut best = TABLE_SIZES[0];
    for candidate in TABLE_SIZES {
        let distance = (candidate - size).abs();
        let best_distance = (best - size).abs();
        if distance < best_distance || (distance == best_distance && candidate > best) {
            best = candidate;
        }
    }
    best
}

/// One table lookup: the row a spell was judged as, its number, and whether the spell had to
/// be clamped onto the table to get there — decided here, where the clamp happens, so the
/// caveat that words it can never disagree with the number it explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Threshold {
    key: &'static str,
    sources: i64,
    /// More pips of the colour than the table's four-pip row: judged as four, a **floor**.
    clamped_pips: bool,
    /// A mana value past the pip group's last row: judged as that row, an **over-estimate**
    /// (within a group the numbers fall as the turn rises). Not reported when the pip clamp
    /// already applies — a cost with more pips than the table knows is a floor whatever its
    /// mana value, and saying "over-estimate" beside it would be false.
    clamped_turn: bool,
}

/// Look up the sources one colour needs for a spell of `mana_value` with `pips` of it, in a
/// deck of `table_size` cards.
///
/// Clamped, in both directions, to the table: more than four pips reads as four, a mana
/// value above the pip group's last row reads as that row, and one below it — impossible for
/// a cost that carries the pips, kept total for a fold that mustn't panic — reads as the
/// first. The two clamps are reported separately because they err in opposite directions.
fn threshold(table_size: i64, mana_value: i64, pips: i64) -> Threshold {
    let column = TABLE_SIZES
        .iter()
        .position(|size| *size == table_size)
        .unwrap_or(0);
    let clamped_pips = pips > MAX_TABLE_PIPS;
    let pips = pips.clamp(1, MAX_TABLE_PIPS);
    let mut chosen: Option<&(&str, i64, i64, [i64; 4])> = None;
    for row in KARSTEN_2022.iter().filter(|row| row.2 == pips) {
        // Rows ascend by mana value within a pip group: keep walking while the row's mana
        // value doesn't exceed the spell's, so the last kept row is the tightest fit.
        if chosen.is_none() || row.1 <= mana_value {
            chosen = Some(row);
        }
    }
    match chosen {
        Some((key, row_mana_value, _, columns)) => Threshold {
            key,
            sources: columns[column],
            clamped_pips,
            clamped_turn: !clamped_pips && *row_mana_value < mana_value,
        },
        None => Threshold {
            key: "C",
            sources: 0,
            clamped_pips,
            clamped_turn: false,
        },
    }
}

// ---------- Mana-cost grammar ----------

/// The five colours plus colourless, in the order the response lists them. `C` is a real
/// requirement — an Eldrazi's `{C}` needs a source of colourless the way `{U}` needs blue —
/// and Scryfall lists it in `produced_mana` like any colour.
const COLORS: &[(char, &str)] = &[
    ('W', "White"),
    ('U', "Blue"),
    ('B', "Black"),
    ('R', "Red"),
    ('G', "Green"),
    ('C', "Colorless"),
];

fn color_label(color: char) -> &'static str {
    COLORS
        .iter()
        .find(|(letter, _)| *letter == color)
        .map_or("Unknown", |(_, label)| label)
}

fn color_letter(symbol: &str) -> Option<char> {
    match symbol {
        "W" => Some('W'),
        "U" => Some('U'),
        "B" => Some('B'),
        "R" => Some('R'),
        "G" => Some('G'),
        "C" => Some('C'),
        _ => None,
    }
}

/// One mana cost, read: its mana value, the pips that **must** be paid in a colour, and the
/// pips that merely *could* be.
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedCost {
    /// The half that was read — the front face of a split or adventure card.
    text: String,
    /// Scryfall's mana value for that half: generic counts itself, `{X}` counts nothing, a
    /// `{2/W}` counts two, every other symbol counts one.
    mana_value: i64,
    /// Pips of exactly one colour: `{B}`, `{C}`.
    hard: BTreeMap<char, i64>,
    /// Pips that name a colour but can be paid another way: each colour of a hybrid
    /// `{W/U}`, the colour of a twobrid `{2/W}` or a Phyrexian `{W/P}`.
    flexible: BTreeMap<char, i64>,
    /// Whether the cost holds an `{X}` (or `{Y}`/`{Z}`): its real mana value is the player's
    /// choice, so the turn it is cast on — and the table row — is not the cost's to state.
    x_cost: bool,
}

/// Read a Scryfall mana cost (`{1}{B}{B}`, `{W/U}{W/U}`, `{2/G}`, `{G/U/P}`, `{X}{R}`).
///
/// A split card's two halves are joined by `//`; only the front half is read, the same
/// choice `CardFacts::front_type_line` makes — the two halves are two spells, and judging
/// the deck against both would double a card's demand. A symbol the grammar doesn't know
/// (`{S}` snow, `{HW}` half, `{CHAOS}`) states no colour and is left out of the mana value,
/// which is the conservative reading: a spell is never reported harder than its cost.
fn parse_cost(cost: &str) -> ParsedCost {
    let front = cost.split("//").next().unwrap_or_default().trim();
    let mut parsed = ParsedCost {
        text: front.to_string(),
        ..ParsedCost::default()
    };
    let mut rest = front;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        let symbol = rest[start + 1..start + len].to_ascii_uppercase();
        rest = &rest[start + len + 1..];

        if let Ok(generic) = symbol.parse::<i64>() {
            parsed.mana_value = parsed.mana_value.saturating_add(generic.max(0));
            continue;
        }
        if symbol.contains('/') {
            let parts: Vec<&str> = symbol.split('/').collect();
            // A twobrid `{2/W}` is two generic or one white; every other slash symbol
            // (hybrid, Phyrexian) is one mana.
            let value = if parts.first() == Some(&"2") { 2 } else { 1 };
            parsed.mana_value = parsed.mana_value.saturating_add(value);
            for part in parts {
                if let Some(color) = color_letter(part) {
                    *parsed.flexible.entry(color).or_default() += 1;
                }
            }
            continue;
        }
        match symbol.as_str() {
            "X" | "Y" | "Z" => parsed.x_cost = true,
            _ => {
                if let Some(color) = color_letter(&symbol) {
                    parsed.mana_value = parsed.mana_value.saturating_add(1);
                    *parsed.hard.entry(color).or_default() += 1;
                }
            }
        }
    }
    parsed
}

// ---------- Wire types ----------

/// One spell counted against a colour — a card whose cost holds hard pips of it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckManaDemandCard {
    /// External card id of one printing (for keys and links).
    pub card_id: String,
    pub name: String,
    /// Copies of that name across the deck it's cast from (regular + foil, every section).
    pub quantity: i64,
    /// The cost as read — the front half of a split card.
    pub mana_cost: String,
    /// Pips of this colour in that cost.
    pub pips: i64,
    /// The mana value of that cost — the turn the spell is meant to be cast on, unless
    /// `x_cost` (then it is the value with `X` at zero) or `clamped` (then the table row it
    /// was judged as sits below it).
    pub turn: i64,
    /// The table row this spell was judged as (`"1CC"`), after clamping.
    pub cost_key: String,
    /// Whether the cost also holds hard pips of another colour, which adds one to the
    /// requirement (Karsten's gold-card rule).
    pub gold: bool,
    /// Sources of this colour a deck this size needs to cast it on curve.
    pub sources_needed: i64,
    /// Whether the cost holds an `{X}`. Listed with the number its fixed pips alone imply,
    /// but **never** the card that sets the colour's requirement: how much mana it is cast
    /// for is the player's choice, which is also Karsten's advice for one.
    pub x_cost: bool,
    /// Whether the spell had to be clamped onto the table to be judged: more than four pips
    /// of the colour (judged as four — a floor), or a mana value past its pip group's last
    /// row (judged as that row — an over-estimate). `cost_key` is then not the cost's own
    /// row, and a client must not present it as one.
    pub clamped: bool,
}

/// One card in the library that produces a colour.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckManaSource {
    /// External card id of one printing (for keys and links).
    pub card_id: String,
    pub name: String,
    /// Copies of that name in the library — each one is one source.
    pub quantity: i64,
    /// Whether the card is a land; a nonland producer (a mana creature, a rock) counts the
    /// same here, and the split is reported so a reader can weigh it.
    pub land: bool,
}

/// The verdict on one colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckManaStatus {
    /// The library holds at least the sources the most demanding spell needs.
    Enough,
    /// It holds fewer; `shortfall` says how many fewer.
    Short,
    /// Nothing in the deck has a hard pip of this colour; the row is here for its sources
    /// (or its hybrid pips) alone.
    NoDemand,
    /// The colour is demanded, but only by `X`-cost spells — whose requirement is the
    /// player's to state, so no number is asserted.
    Undecided,
}

/// One colour's ledger: what the deck asks for, what the library gives, and the verdict.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckManaColor {
    /// `W` / `U` / `B` / `R` / `G` / `C`.
    pub color: String,
    /// `"White"`, …, `"Colorless"`.
    pub label: String,
    /// Hard pips of this colour across the deck it's cast from, copy-weighted.
    pub pips: i64,
    /// Pips this colour *could* pay (hybrid, twobrid, Phyrexian), copy-weighted. Reported,
    /// never counted against the colour.
    pub hybrid_pips: i64,
    /// Distinct card names with a hard pip of this colour.
    pub demand_count: i64,
    /// Those cards, most demanding first, capped at 10 (`demand_count` stays exact).
    pub demand: Vec<DeckManaDemandCard>,
    /// Sources in the library, copy-weighted: lands and nonland producers alike.
    pub sources: i64,
    pub land_sources: i64,
    pub nonland_sources: i64,
    /// Distinct card names producing this colour.
    pub source_count: i64,
    /// Those cards, lands first, capped at 50 (`source_count` stays exact).
    pub source_cards: Vec<DeckManaSource>,
    /// Sources the most demanding spell needs — the largest `sources_needed` in `demand` —
    /// or null when nothing demands the colour.
    pub sources_needed: Option<i64>,
    /// `max(0, sources_needed - sources)`.
    pub shortfall: i64,
    pub status: DeckManaStatus,
    /// One ready-to-render sentence: "Short 2 black sources", "Enough blue sources".
    pub verdict: String,
}

/// Everything `GET /api/decks/{game}/{deck_id}/mana` answers.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckManaBase {
    /// Copies in the deck the spells are cast from: the library plus the command zone.
    pub deck_size: i64,
    /// The table column the deck was judged against: 40, 60, 80 or 99 — the format's deck
    /// size when it states one, else the nearest to `deck_size`.
    pub table_size: i64,
    /// Copies in the library — the sources' pool.
    pub library_size: i64,
    /// Land copies in the library.
    pub land_count: i64,
    /// One ledger per colour the deck demands or produces, in WUBRG-then-colourless order.
    pub colors: Vec<DeckManaColor>,
    /// Distinct cards in the library whose catalog row hasn't been checked for what it
    /// produces (it predates the empty-string convention, which migration 79 backfilled and
    /// every bulk import writes). While this is non-zero every source count is a floor.
    pub unchecked_count: i64,
    /// What the numbers assume. Never empty — a threshold is only honest beside its model.
    pub caveats: Vec<String>,
    /// Where the thresholds come from.
    pub source: String,
}

// ---------- The fold ----------

/// One colour's ledger while it's being built.
#[derive(Default)]
struct ColorLedger {
    pips: i64,
    hybrid_pips: i64,
    demand: Vec<DeckManaDemandCard>,
    sources: Vec<DeckManaSource>,
}

/// The library's and the command zone's section ids: which sections are the zone is
/// [`deck_zone`]'s answer, whether the zone leads this deck's format is
/// [`format_leads_with_command_zone`]'s — a `Commander` section in a Modern deck is library.
/// A maybeboard is neither, by its column; a sideboard is neither, by its name.
///
/// Deliberately borrows both answers rather than [`super::stats::default_library_section_ids`]
/// alone: that selection is the *shuffle* (a card filed under `Commander` starts a Modern game
/// in hand no more than in the command zone), while a source is a source wherever the 60 is
/// kept.
fn zone_sections(format: Option<&str>, input: &DeckAnalysisInput) -> (Vec<i32>, Vec<i32>) {
    let leads = format_leads_with_command_zone(format);
    let mut library = Vec::new();
    let mut command = Vec::new();
    for section in &input.sections {
        if section.is_maybeboard {
            continue;
        }
        match deck_zone(&section.name) {
            DeckZone::Main => library.push(section.id),
            DeckZone::Command if leads => command.push(section.id),
            DeckZone::Command => library.push(section.id),
            DeckZone::Sideboard => {}
        }
    }
    (library, command)
}

fn is_land(fold: &NameFold<'_>) -> bool {
    type_words(fold.facts.type_line.as_deref()).contains("Land")
}

fn plural(count: i64, one: &str, many: &str) -> String {
    if count == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// The card that sets a colour's number: the most demanding **non-`X`** spell, since an
/// `X` spell's real cost is the player's choice. `ledger.demand` is already sorted most
/// demanding first.
fn decisive(ledger: &ColorLedger) -> Option<&DeckManaDemandCard> {
    ledger.demand.iter().find(|card| !card.x_cost)
}

/// Word one colour's verdict.
fn verdict(color: char, ledger: &ColorLedger, sources: i64, status: DeckManaStatus) -> String {
    let label = color_label(color).to_lowercase();
    match (status, decisive(ledger)) {
        (DeckManaStatus::NoDemand, _) if ledger.hybrid_pips > 0 => format!(
            "Only hybrid pips ask for {label} — {} {} it could pay, none it must.",
            ledger.hybrid_pips,
            plural(ledger.hybrid_pips, "pip", "pips")
        ),
        (DeckManaStatus::NoDemand, _) => format!("Nothing in this deck needs {label} mana."),
        (DeckManaStatus::Undecided, _) => format!(
            "Only X-cost {} for {label}: how many sources that takes depends on X.",
            plural(ledger.demand.len() as i64, "spell asks", "spells ask")
        ),
        (DeckManaStatus::Enough, Some(card)) => format!(
            "Enough {label} sources: {sources} of the {} needed.",
            card.sources_needed
        ),
        (DeckManaStatus::Short, Some(card)) => {
            let needed = card.sources_needed;
            let short = needed - sources;
            format!(
                "Short {short} {label} {}: {sources} of {needed} needed for {}.",
                plural(short, "source", "sources"),
                card.name
            )
        }
        // Unreachable: Enough and Short are only assigned when a decisive card exists.
        (_, None) => format!("Nothing in this deck needs {label} mana."),
    }
}

/// Fold a deck into its mana base.
pub(crate) fn analyse_mana(format: Option<&str>, input: &DeckAnalysisInput) -> DeckManaBase {
    let (library_ids, command_ids) = zone_sections(format, input);
    let library = fold_by_name(&input.in_sections(&library_ids));
    let played_ids: Vec<i32> = library_ids.iter().chain(&command_ids).copied().collect();
    let played = fold_by_name(&input.in_sections(&played_ids));

    let deck_size: i64 = played.iter().map(|fold| fold.copies).sum();
    let library_size: i64 = library.iter().map(|fold| fold.copies).sum();
    let land_count: i64 = library
        .iter()
        .filter(|fold| is_land(fold))
        .map(|fold| fold.copies)
        .sum();
    // A format that states its deck size picks the column outright (Commander's 100 is the
    // 99-card column, which is the one Karsten computed with the free mulligan and draw); a
    // deck with no such format is judged as the size it is.
    let table_size = table_size_for(format_deck_size(format).unwrap_or(deck_size));

    let mut ledgers: BTreeMap<char, ColorLedger> = BTreeMap::new();
    // Decided in the fold, over every demanding card, never re-derived from the capped lists.
    let mut any_clamped_pips = false;
    let mut any_clamped_turn = false;
    let mut any_x_cost = false;

    // Demand: every hard pip in the deck a player casts from.
    for fold in &played {
        let Some(cost) = fold.facts.mana_cost.as_deref() else {
            continue;
        };
        let parsed = parse_cost(cost);
        let gold = parsed.hard.len() > 1;
        for (&color, &pips) in &parsed.hard {
            let mana_value = parsed.mana_value.max(pips);
            let row = threshold(table_size, mana_value, pips);
            any_clamped_pips |= row.clamped_pips;
            any_clamped_turn |= row.clamped_turn;
            any_x_cost |= parsed.x_cost;
            let ledger = ledgers.entry(color).or_default();
            ledger.pips = ledger.pips.saturating_add(pips.saturating_mul(fold.copies));
            ledger.demand.push(DeckManaDemandCard {
                card_id: fold.card_id.clone(),
                name: fold.facts.name.clone(),
                quantity: fold.copies,
                mana_cost: parsed.text.clone(),
                pips,
                turn: mana_value,
                cost_key: row.key.to_string(),
                gold,
                sources_needed: row.sources + i64::from(gold),
                x_cost: parsed.x_cost,
                clamped: row.clamped_pips || row.clamped_turn,
            });
        }
        for (&color, &pips) in &parsed.flexible {
            let ledger = ledgers.entry(color).or_default();
            ledger.hybrid_pips = ledger
                .hybrid_pips
                .saturating_add(pips.saturating_mul(fold.copies));
        }
    }

    // Supply: what the library produces. A NULL column is counted, not read as nothing.
    let mut unchecked_count = 0;
    for fold in &library {
        let Some(produced) = fold.facts.produced_mana.as_deref() else {
            unchecked_count += 1;
            continue;
        };
        let land = is_land(fold);
        let mut seen: Vec<char> = Vec::new();
        for color in produced
            .iter()
            .filter_map(|letter| color_letter(&letter.to_ascii_uppercase()))
        {
            if seen.contains(&color) {
                continue;
            }
            seen.push(color);
            ledgers
                .entry(color)
                .or_default()
                .sources
                .push(DeckManaSource {
                    card_id: fold.card_id.clone(),
                    name: fold.facts.name.clone(),
                    quantity: fold.copies,
                    land,
                });
        }
    }

    let colors: Vec<DeckManaColor> = COLORS
        .iter()
        .filter_map(|(color, label)| {
            let mut ledger = ledgers.remove(color)?;

            // Most demanding first, so the first row is the one the verdict names.
            ledger.demand.sort_by(|a, b| {
                b.sources_needed
                    .cmp(&a.sources_needed)
                    .then_with(|| b.pips.cmp(&a.pips))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let demand_count = ledger.demand.len() as i64;
            // The number is the most demanding spell's — skipping X spells, whose real cost
            // is the player's. They stay in `demand` so the reader sees them.
            let sources_needed = decisive(&ledger).map(|card| card.sources_needed);

            // Lands first, then the most copies, then by name.
            ledger.sources.sort_by(|a, b| {
                b.land
                    .cmp(&a.land)
                    .then_with(|| b.quantity.cmp(&a.quantity))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let source_count = ledger.sources.len() as i64;
            let land_sources: i64 = ledger
                .sources
                .iter()
                .filter(|s| s.land)
                .map(|s| s.quantity)
                .sum();
            let nonland_sources: i64 = ledger
                .sources
                .iter()
                .filter(|s| !s.land)
                .map(|s| s.quantity)
                .sum();
            let sources = land_sources + nonland_sources;

            let shortfall = sources_needed.map_or(0, |needed| (needed - sources).max(0));
            let status = match sources_needed {
                None if ledger.demand.is_empty() => DeckManaStatus::NoDemand,
                None => DeckManaStatus::Undecided,
                Some(_) if shortfall > 0 => DeckManaStatus::Short,
                Some(_) => DeckManaStatus::Enough,
            };
            let verdict = verdict(*color, &ledger, sources, status);

            let mut demand = ledger.demand;
            demand.truncate(MAX_LISTED_DEMAND);
            let mut source_cards = ledger.sources;
            source_cards.truncate(MAX_LISTED_SOURCES);

            Some(DeckManaColor {
                color: color.to_string(),
                label: (*label).to_string(),
                pips: ledger.pips,
                hybrid_pips: ledger.hybrid_pips,
                demand_count,
                demand,
                sources,
                land_sources,
                nonland_sources,
                source_count,
                source_cards,
                sources_needed,
                shortfall,
                status,
                verdict,
            })
        })
        .collect();

    let mut caveats = vec![
        format!(
            "Thresholds are Frank Karsten's 2022 numbers for a {table_size}-card deck: the \
             sources of one colour that cast a spell on curve 90% of the time for a one-drop, \
             rising to 96% for a seven-drop, assuming a typical land count and enough lands \
             drawn. A tapped land, a fetch land, a mana creature or a rock each count as one \
             full source here; Karsten weighs some of them as less."
        ),
        "A multicoloured card needs one more source of each of its colours (Karsten's \
         gold-card rule). Hybrid, Phyrexian and {2/C} pips are listed but never counted \
         against a colour — a simplification of this read, not the article's: Karsten asks \
         for the table number in combined sources across a hybrid's colours."
            .to_string(),
        "Only the shuffled library supplies mana: a commander's own pips are demand, but its \
         section is never a source for the rest of the deck."
            .to_string(),
    ];
    if any_x_cost {
        caveats.push(
            "An X-cost spell is listed with the number its fixed pips imply but never sets a \
             colour's requirement: Karsten judges one by the lands you expect to tap for it, \
             which only you know."
                .to_string(),
        );
    }
    if any_clamped_pips {
        caveats.push(
            "A spell with more than four pips of one colour is judged as the table's four-pip \
             row — a floor, since the table stops there."
                .to_string(),
        );
    }
    if any_clamped_turn {
        caveats.push(
            "A spell whose mana value is past the last row for its pip count is judged as that \
             row, which over-states what it needs — a spell cast a turn later is only ever \
             easier to have the colours for."
                .to_string(),
        );
    }
    if unchecked_count > 0 {
        caveats.push(format!(
            "{unchecked_count} {} in the library {} been checked for what it produces yet \
             — card data is still syncing, so every source count is a floor.",
            plural(unchecked_count, "card", "cards"),
            plural(unchecked_count, "hasn't", "haven't"),
        ));
    }

    DeckManaBase {
        deck_size,
        table_size,
        library_size,
        land_count,
        colors,
        unchecked_count,
        caveats,
        source: KARSTEN_SOURCE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{deck, entry, section};
    use super::*;

    fn cost(text: &str) -> ParsedCost {
        parse_cost(text)
    }

    /// A lookup as `(row, sources)`, for the cases that aren't about the clamp flags.
    fn t(table_size: i64, mana_value: i64, pips: i64) -> (&'static str, i64) {
        let row = threshold(table_size, mana_value, pips);
        (row.key, row.sources)
    }

    fn color<'a>(base: &'a DeckManaBase, color: &str) -> &'a DeckManaColor {
        base.colors
            .iter()
            .find(|c| c.color == color)
            .unwrap_or_else(|| panic!("no {color} ledger in {:?}", base.colors))
    }

    // ----- The grammar -----

    #[test]
    fn reads_hard_pips_and_the_mana_value() {
        let parsed = cost("{1}{B}{B}");
        assert_eq!(parsed.mana_value, 3);
        assert_eq!(parsed.hard.get(&'B'), Some(&2));
        assert!(parsed.flexible.is_empty());
        assert_eq!(parsed.text, "{1}{B}{B}");
    }

    #[test]
    fn hybrid_twobrid_and_phyrexian_pips_are_flexible_not_hard() {
        let hybrid = cost("{W/U}{W/U}");
        assert!(hybrid.hard.is_empty());
        assert_eq!(hybrid.flexible.get(&'W'), Some(&2));
        assert_eq!(hybrid.flexible.get(&'U'), Some(&2));
        assert_eq!(hybrid.mana_value, 2);

        let twobrid = cost("{2/G}{2/G}");
        assert_eq!(twobrid.flexible.get(&'G'), Some(&2));
        assert_eq!(twobrid.mana_value, 4, "a twobrid is two mana each");

        let phyrexian = cost("{1}{G/U/P}");
        assert_eq!(phyrexian.flexible.get(&'G'), Some(&1));
        assert_eq!(phyrexian.flexible.get(&'U'), Some(&1));
        assert!(phyrexian.hard.is_empty());
        assert_eq!(phyrexian.mana_value, 2);
    }

    #[test]
    fn x_counts_nothing_and_colourless_is_a_real_pip() {
        let fireball = cost("{X}{R}");
        assert_eq!(fireball.mana_value, 1);
        assert_eq!(fireball.hard.get(&'R'), Some(&1));

        let eldrazi = cost("{3}{C}");
        assert_eq!(eldrazi.mana_value, 4);
        assert_eq!(eldrazi.hard.get(&'C'), Some(&1));
    }

    #[test]
    fn a_split_card_is_read_by_its_front_half_only() {
        let parsed = cost("{1}{R} // {1}{U}");
        assert_eq!(parsed.text, "{1}{R}");
        assert_eq!(parsed.hard.get(&'R'), Some(&1));
        assert_eq!(parsed.hard.get(&'U'), None);
        assert_eq!(parsed.mana_value, 2);
    }

    #[test]
    fn an_unknown_symbol_states_no_colour_and_an_empty_cost_nothing() {
        let snow = cost("{S}{S}");
        assert!(snow.hard.is_empty());
        assert_eq!(snow.mana_value, 0);
        assert_eq!(cost(""), ParsedCost::default());
        assert_eq!(
            cost("{1}{B").hard.get(&'B'),
            None,
            "an unclosed brace is skipped"
        );
    }

    // ----- The table -----

    #[test]
    fn the_table_is_pinned_to_the_article() {
        // A handful of rows read straight off the article's summary, one per pip group.
        assert_eq!(t(60, 3, 2), ("1CC", 18));
        assert_eq!(t(99, 2, 2), ("CC", 30));
        assert_eq!(t(40, 1, 1), ("C", 9));
        assert_eq!(t(80, 5, 4), ("1CCCC", 31));
        assert_eq!(t(60, 4, 3), ("1CCC", 21));
        // The whole constant, against a second copy typed from the article's summary table
        // (columns 60 / 80 / 99 / 40) — an edit to one number fails here rather than shipping.
        let expected: &[(&str, [i64; 4])] = &[
            ("C", [14, 19, 19, 9]),
            ("1C", [13, 18, 19, 9]),
            ("2C", [12, 16, 18, 8]),
            ("3C", [10, 15, 16, 7]),
            ("4C", [9, 14, 15, 6]),
            ("5C", [9, 12, 14, 6]),
            ("CC", [21, 28, 30, 14]),
            ("1CC", [18, 25, 28, 12]),
            ("2CC", [16, 23, 26, 11]),
            ("3CC", [15, 20, 23, 10]),
            ("4CC", [13, 19, 22, 9]),
            ("5CC", [12, 17, 20, 8]),
            ("CCC", [23, 32, 36, 16]),
            ("1CCC", [21, 29, 33, 14]),
            ("2CCC", [19, 26, 30, 13]),
            ("3CCC", [17, 24, 28, 11]),
            ("4CCC", [16, 22, 26, 10]),
            ("CCCC", [24, 34, 39, 17]),
            ("1CCCC", [22, 31, 36, 15]),
        ];
        assert_eq!(KARSTEN_2022.len(), expected.len());
        for ((key, mana_value, pips, columns), (expected_key, expected_columns)) in
            KARSTEN_2022.iter().zip(expected)
        {
            assert_eq!(key, expected_key);
            assert_eq!(columns, expected_columns, "{key}");
            // The key spells the row: its generic part plus one `C` per pip.
            let generic = mana_value - pips;
            let spelled = format!(
                "{}{}",
                if generic > 0 {
                    generic.to_string()
                } else {
                    String::new()
                },
                "C".repeat(*pips as usize)
            );
            assert_eq!(*key, spelled, "row key must match its mana value and pips");
        }
    }

    #[test]
    fn every_pip_group_has_rows_and_the_table_clamps() {
        for pips in 1..=MAX_TABLE_PIPS {
            assert!(
                KARSTEN_2022.iter().any(|row| row.2 == pips),
                "pip group {pips}"
            );
        }
        // Past the last row of a group: the last row, never nothing — and flagged as the
        // turn clamp, which is an over-estimate.
        assert_eq!(t(60, 9, 1), ("5C", 9));
        assert_eq!(t(60, 12, 2), ("5CC", 12));
        let seven_drop_one_pip = threshold(60, 7, 1);
        assert!(
            seven_drop_one_pip.clamped_turn,
            "the one-pip group ends at six"
        );
        assert!(!seven_drop_one_pip.clamped_pips);
        let six_drop_four_pips = threshold(60, 6, 4);
        assert!(
            six_drop_four_pips.clamped_turn,
            "the four-pip group ends at five"
        );
        assert!(
            !threshold(60, 6, 1).clamped_turn,
            "a row's own mana value isn't a clamp"
        );
        assert!(!threshold(60, 3, 2).clamped_turn);
        // More pips than the table knows: the four-pip group, flagged as the pip clamp.
        assert_eq!(t(60, 5, 6), ("1CCCC", 22));
        let five_pips = threshold(60, 5, 6);
        assert!(five_pips.clamped_pips && !five_pips.clamped_turn);
        // A mana value below the group's first row: the first row, not a panic, no clamp.
        assert_eq!(t(60, 1, 3), ("CCC", 23));
        assert!(!threshold(60, 1, 3).clamped_turn);
        // An unknown table size falls back to the 60-card column.
        assert_eq!(t(17, 1, 1), ("C", 14));
    }

    #[test]
    fn the_table_column_is_the_nearest_size() {
        assert_eq!(table_size_for(100), 99);
        assert_eq!(table_size_for(99), 99);
        assert_eq!(table_size_for(60), 60);
        assert_eq!(table_size_for(58), 60);
        assert_eq!(table_size_for(40), 40);
        assert_eq!(table_size_for(0), 40);
        assert_eq!(table_size_for(50), 60, "a tie goes to the larger deck");
        assert_eq!(table_size_for(75), 80);
        assert_eq!(table_size_for(250), 99);
    }

    // ----- The fold -----

    /// A Commander deck: the commander's pips are demand, its section is not supply; lands
    /// are copy-weighted; the verdict names the hungriest spell.
    #[test]
    fn a_commander_deck_is_judged_against_the_99_card_column() {
        let sections = vec![
            section(1, "Commander", false),
            section(2, "Lands", false),
            section(3, "Spells", false),
        ];
        let input = deck(
            sections,
            vec![
                entry("cmd", "Zur the Enchanter", 1, 1, 0)
                    .mana_cost("{1}{W}{U}{B}")
                    .produces("W,U,B"),
                entry("swamp", "Swamp", 2, 20, 0)
                    .type_line("Basic Land — Swamp")
                    .produces("B"),
                entry("island", "Island", 2, 5, 0)
                    .type_line("Basic Land — Island")
                    .produces("U"),
                entry("rock", "Arcane Signet", 3, 1, 0)
                    .type_line("Artifact")
                    .mana_cost("{2}")
                    .produces("W,U,B"),
                entry("necro", "Necropotence", 3, 1, 0)
                    .mana_cost("{B}{B}{B}")
                    .produces(""),
                entry("rhystic", "Rhystic Study", 3, 1, 0)
                    .mana_cost("{2}{U}")
                    .produces(""),
            ],
        );

        let base = analyse_mana(Some("Commander"), &input);
        assert_eq!(base.table_size, 99);
        assert_eq!(base.deck_size, 29, "library plus the commander");
        assert_eq!(base.library_size, 28);
        assert_eq!(base.land_count, 25);
        assert_eq!(base.unchecked_count, 0);

        let black = color(&base, "B");
        assert_eq!(black.pips, 4, "three from Necropotence, one from Zur");
        assert_eq!(black.sources, 21, "twenty Swamps and the Signet");
        assert_eq!(black.land_sources, 20);
        assert_eq!(black.nonland_sources, 1);
        // Necropotence is the hungriest: CCC on turn three in a 99-card deck is 36.
        assert_eq!(black.sources_needed, Some(36));
        assert_eq!(black.demand[0].name, "Necropotence");
        assert_eq!(black.demand[0].cost_key, "CCC");
        assert!(!black.demand[0].gold);
        assert_eq!(black.shortfall, 15);
        assert_eq!(black.status, DeckManaStatus::Short);
        assert_eq!(
            black.verdict,
            "Short 15 black sources: 21 of 36 needed for Necropotence."
        );

        // The commander's own pips are demand, with the gold-card +1 — but the commander
        // is never a source, so white has the Signet alone.
        let white = color(&base, "W");
        assert_eq!(white.pips, 1);
        assert_eq!(white.sources, 1);
        assert_eq!(white.demand[0].name, "Zur the Enchanter");
        assert!(white.demand[0].gold);
        // 1CCC-ish? No: one white pip in a four-drop is the 3C row (16) plus one for gold.
        assert_eq!(white.demand[0].cost_key, "3C");
        assert_eq!(white.sources_needed, Some(17));

        let blue = color(&base, "U");
        assert_eq!(blue.sources, 6);
        // Rhystic Study (2C on turn three: 18) outranks Zur's gold 3C (17).
        assert_eq!(blue.sources_needed, Some(18));
        assert_eq!(blue.demand[0].name, "Rhystic Study");
    }

    #[test]
    fn maybeboards_and_sideboards_are_neither_demand_nor_supply() {
        let sections = vec![
            section(1, "Deck", false),
            section(2, "Sideboard", false),
            section(3, "Considering", true),
        ];
        let input = deck(
            sections,
            vec![
                entry("bolt", "Lightning Bolt", 1, 4, 0)
                    .mana_cost("{R}")
                    .produces(""),
                entry("mtn", "Mountain", 1, 20, 0)
                    .type_line("Basic Land — Mountain")
                    .produces("R"),
                entry("sb", "Rest in Peace", 2, 2, 0).mana_cost("{1}{W}"),
                entry("plains", "Plains", 2, 4, 0)
                    .type_line("Basic Land — Plains")
                    .produces("W"),
                entry("maybe", "Counterspell", 3, 1, 0).mana_cost("{U}{U}"),
                entry("island", "Island", 3, 4, 0)
                    .type_line("Basic Land — Island")
                    .produces("U"),
            ],
        );
        let base = analyse_mana(Some("Modern"), &input);
        assert_eq!(base.table_size, 60, "the format states the size");
        assert_eq!(base.deck_size, 24);
        assert_eq!(
            base.colors
                .iter()
                .map(|c| c.color.as_str())
                .collect::<Vec<_>>(),
            vec!["R"],
            "white and blue live only outside the deck"
        );
        let red = color(&base, "R");
        assert_eq!(red.pips, 4);
        assert_eq!(red.sources, 20);
        assert_eq!(red.sources_needed, Some(14), "C on turn one, 60 cards");
        assert_eq!(red.status, DeckManaStatus::Enough);
        assert_eq!(red.verdict, "Enough red sources: 20 of the 14 needed.");
    }

    #[test]
    fn a_deck_without_a_format_is_judged_as_the_size_it_is() {
        let input = deck(
            vec![section(1, "Deck", false)],
            vec![
                entry("elf", "Llanowar Elves", 1, 4, 0)
                    .type_line("Creature — Elf Druid")
                    .mana_cost("{G}")
                    .produces("G"),
                entry("forest", "Forest", 1, 16, 0)
                    .type_line("Basic Land — Forest")
                    .produces("G"),
                entry("spell", "Giant Growth", 1, 20, 0).mana_cost("{G}"),
            ],
        );
        let base = analyse_mana(None, &input);
        assert_eq!(base.deck_size, 40);
        assert_eq!(base.table_size, 40);
        let green = color(&base, "G");
        assert_eq!(green.sources, 20, "the Elves count as sources too");
        assert_eq!(green.land_sources, 16);
        assert_eq!(green.nonland_sources, 4);
        assert_eq!(green.source_count, 2);
        assert!(green.source_cards[0].land, "lands are listed first");
        assert_eq!(green.sources_needed, Some(9));
    }

    #[test]
    fn a_null_produced_mana_column_is_unchecked_not_a_non_source() {
        let input = deck(
            vec![section(1, "Deck", false)],
            vec![
                // Checked, produces nothing: not a source and not unchecked.
                entry("bear", "Grizzly Bears", 1, 4, 0)
                    .type_line("Creature — Bear")
                    .mana_cost("{1}{G}")
                    .produces(""),
                // Never rewritten since the convention arrived.
                entry("maze", "Maze of Ith", 1, 1, 0).type_line("Land"),
                entry("forest", "Forest", 1, 10, 0)
                    .type_line("Basic Land — Forest")
                    .produces("G"),
            ],
        );
        let base = analyse_mana(None, &input);
        assert_eq!(base.unchecked_count, 1);
        assert_eq!(color(&base, "G").sources, 10);
        assert!(
            base.caveats
                .iter()
                .any(|c| c.contains("hasn't been checked")),
            "{:?}",
            base.caveats
        );
    }

    #[test]
    fn hybrid_pips_are_reported_and_never_decisive() {
        let input = deck(
            vec![section(1, "Deck", false)],
            vec![
                entry("kitchen", "Kitchen Finks", 1, 4, 0)
                    .mana_cost("{1}{G/W}{G/W}")
                    .produces(""),
                entry("plains", "Plains", 1, 10, 0)
                    .type_line("Basic Land — Plains")
                    .produces("W"),
            ],
        );
        let base = analyse_mana(Some("Modern"), &input);
        let white = color(&base, "W");
        assert_eq!(white.pips, 0);
        assert_eq!(white.hybrid_pips, 8);
        assert_eq!(white.status, DeckManaStatus::NoDemand);
        assert_eq!(white.sources_needed, None);
        assert_eq!(
            white.verdict,
            "Only hybrid pips ask for white — 8 pips it could pay, none it must."
        );
        let green = color(&base, "G");
        assert_eq!(green.hybrid_pips, 8);
        assert_eq!(green.sources, 0);
        assert_eq!(green.status, DeckManaStatus::NoDemand);
    }

    #[test]
    fn a_card_in_two_printings_is_one_card_with_the_copies_of_both() {
        let input = deck(
            vec![section(1, "Deck", false)],
            vec![
                entry("bolt-a", "Lightning Bolt", 1, 2, 0)
                    .mana_cost("{R}")
                    .produces(""),
                entry("bolt-b", "Lightning Bolt", 1, 1, 1)
                    .mana_cost("{R}")
                    .produces(""),
                entry("mtn-a", "Mountain", 1, 3, 0)
                    .type_line("Basic Land — Mountain")
                    .produces("R"),
                entry("mtn-b", "Mountain", 1, 0, 2)
                    .type_line("Basic Land — Mountain")
                    .produces("R"),
            ],
        );
        let base = analyse_mana(None, &input);
        let red = color(&base, "R");
        assert_eq!(red.demand_count, 1);
        assert_eq!(red.demand[0].quantity, 4);
        assert_eq!(red.pips, 4);
        assert_eq!(red.source_count, 1);
        assert_eq!(red.sources, 5);
    }

    #[test]
    fn the_lists_are_capped_but_the_counts_stay_exact() {
        let mut entries = Vec::new();
        for i in 0..60 {
            entries.push(
                entry(&format!("l{i}"), &format!("Land {i:02}"), 1, 1, 0)
                    .type_line("Land")
                    .produces("B"),
            );
            entries
                .push(entry(&format!("s{i}"), &format!("Spell {i:02}"), 1, 1, 0).mana_cost("{B}"));
        }
        let base = analyse_mana(None, &deck(vec![section(1, "Deck", false)], entries));
        let black = color(&base, "B");
        assert_eq!(black.source_count, 60);
        assert_eq!(black.source_cards.len(), MAX_LISTED_SOURCES);
        assert_eq!(black.sources, 60);
        assert_eq!(black.demand_count, 60);
        assert_eq!(black.demand.len(), MAX_LISTED_DEMAND);
        assert_eq!(black.pips, 60);
    }

    #[test]
    fn an_empty_deck_still_states_its_model() {
        let base = analyse_mana(Some("Commander"), &deck(vec![], vec![]));
        assert!(base.colors.is_empty());
        assert_eq!(base.deck_size, 0);
        assert_eq!(base.table_size, 99);
        assert!(!base.caveats.is_empty());
        assert!(base.source.contains("Karsten"));
    }

    #[test]
    fn the_clamp_caveats_appear_only_when_the_table_was_clamped_and_say_which_way() {
        let plain = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![entry("a", "A", 1, 1, 0).mana_cost("{1}{U}")],
            ),
        );
        assert!(!plain.caveats.iter().any(|c| c.contains("last row")));
        assert!(!plain.caveats.iter().any(|c| c.contains("four-pip")));
        assert!(!color(&plain, "U").demand[0].clamped);

        // A mana value past its pip group's last row: the over-estimate wording, and the
        // card says it was clamped so a client never presents the row as the cost's own.
        let late = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![entry("p", "Primal Surge", 1, 1, 0).mana_cost("{8}{G}{G}")],
            ),
        );
        assert!(late.caveats.iter().any(|c| c.contains("over-states")));
        assert!(!late.caveats.iter().any(|c| c.contains("four-pip")));
        assert_eq!(color(&late, "G").demand[0].cost_key, "5CC");
        assert!(color(&late, "G").demand[0].clamped);

        // The one-pip group ends at six, so a seven-mana single-pip spell is clamped too —
        // which a fixed "past seven" test would have missed.
        let seven = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![entry("s", "Sphinx", 1, 1, 0).mana_cost("{6}{U}")],
            ),
        );
        assert!(seven.caveats.iter().any(|c| c.contains("over-states")));
        assert_eq!(color(&seven, "U").demand[0].cost_key, "5C");

        // More than four pips: the floor wording, and only that.
        let pips = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![entry("k", "Khalni Hydra", 1, 1, 0).mana_cost("{G}{G}{G}{G}{G}{G}{G}{G}")],
            ),
        );
        assert!(pips.caveats.iter().any(|c| c.contains("four-pip")));
        assert!(!pips.caveats.iter().any(|c| c.contains("over-states")));
        assert!(color(&pips, "G").demand[0].clamped);

        // The clamp is decided in the fold, not read off the capped list: a clamped spell
        // sorted past the tenth listed card still raises the caveat.
        let mut entries: Vec<_> = (0..12)
            .map(|i| {
                entry(&format!("h{i}"), &format!("Hungry {i:02}"), 1, 1, 0)
                    .mana_cost("{B}{B}{B}{B}")
            })
            .collect();
        entries.push(entry("late", "Late", 1, 1, 0).mana_cost("{9}{B}"));
        let buried = analyse_mana(None, &deck(vec![section(1, "Deck", false)], entries));
        assert_eq!(color(&buried, "B").demand.len(), MAX_LISTED_DEMAND);
        assert!(!color(&buried, "B").demand.iter().any(|c| c.name == "Late"));
        assert!(buried.caveats.iter().any(|c| c.contains("over-states")));
    }

    /// An X spell's real cost is the player's choice, so it is listed but never the card
    /// that sets a colour's number — Karsten's own advice is to judge one by the lands you
    /// expect to tap for it.
    #[test]
    fn an_x_spell_is_listed_but_never_decisive() {
        let sections = vec![section(1, "Deck", false)];
        let with_other = analyse_mana(
            Some("Commander"),
            &deck(
                sections.clone(),
                vec![
                    entry("ex", "Exsanguinate", 1, 1, 0).mana_cost("{X}{B}{B}"),
                    entry("sign", "Sign in Blood", 1, 1, 0).mana_cost("{B}{B}"),
                    entry("swamp", "Swamp", 1, 30, 0)
                        .type_line("Basic Land — Swamp")
                        .produces("B"),
                ],
            ),
        );
        let black = color(&with_other, "B");
        assert_eq!(black.demand_count, 2);
        let ex = black
            .demand
            .iter()
            .find(|c| c.name == "Exsanguinate")
            .unwrap();
        assert!(ex.x_cost);
        assert_eq!(
            ex.cost_key, "CC",
            "listed with the number its fixed pips imply"
        );
        // Sign in Blood (CC on turn two, 30 in a 99-card deck) decides — not Exsanguinate,
        // which would otherwise tie it and be sorted first.
        assert_eq!(black.sources_needed, Some(30));
        assert_eq!(black.status, DeckManaStatus::Enough);
        assert_eq!(black.verdict, "Enough black sources: 30 of the 30 needed.");
        assert!(with_other.caveats.iter().any(|c| c.contains("X-cost")));

        // X spells alone: the colour is demanded, but no number is asserted.
        let alone = analyse_mana(
            Some("Commander"),
            &deck(
                sections,
                vec![
                    entry("ex", "Exsanguinate", 1, 1, 0).mana_cost("{X}{B}{B}"),
                    entry("swamp", "Swamp", 1, 5, 0)
                        .type_line("Basic Land — Swamp")
                        .produces("B"),
                ],
            ),
        );
        let black = color(&alone, "B");
        assert_eq!(black.pips, 2);
        assert_eq!(black.sources_needed, None);
        assert_eq!(black.shortfall, 0);
        assert_eq!(black.status, DeckManaStatus::Undecided);
        assert_eq!(
            black.verdict,
            "Only X-cost spell asks for black: how many sources that takes depends on X."
        );
    }

    /// Every deck is seeded with a `Commander` section; in a format with no command zone the
    /// cards in it are just part of the 60 — demand, and supply too.
    #[test]
    fn a_commander_section_supplies_mana_in_a_format_with_no_command_zone() {
        let sections = vec![section(1, "Commander", false), section(2, "Deck", false)];
        let cards = vec![
            entry("island", "Island", 1, 4, 0)
                .type_line("Basic Land — Island")
                .produces("U"),
            entry("island2", "Island", 2, 10, 0)
                .type_line("Basic Land — Island")
                .produces("U"),
            entry("delver", "Delver of Secrets", 2, 4, 0)
                .mana_cost("{U}")
                .produces(""),
        ];

        let modern = analyse_mana(Some("Modern"), &deck(sections.clone(), cards.clone()));
        assert_eq!(
            modern.library_size, 18,
            "the Commander section is library here"
        );
        assert_eq!(modern.land_count, 14);
        let blue = color(&modern, "U");
        assert_eq!(blue.sources, 14);
        assert_eq!(blue.status, DeckManaStatus::Enough, "{blue:?}");

        // The same rows in Commander: the zone leads, so its cards are demand only.
        let commander = analyse_mana(Some("Commander"), &deck(sections, cards));
        assert_eq!(commander.library_size, 14);
        assert_eq!(color(&commander, "U").sources, 10);
    }

    /// A colour the library produces but nothing pays — the shape almost every Commander
    /// deck has for the colour its lands happen to make.
    #[test]
    fn a_colour_with_sources_and_no_demand_says_so() {
        let base = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![
                    entry("bolt", "Lightning Bolt", 1, 4, 0)
                        .mana_cost("{R}")
                        .produces(""),
                    entry("mtn", "Mountain", 1, 8, 0)
                        .type_line("Basic Land — Mountain")
                        .produces("R"),
                    entry("wastes", "Wastes", 1, 2, 0)
                        .type_line("Basic Land")
                        .produces("C"),
                ],
            ),
        );
        let colorless = color(&base, "C");
        assert_eq!(colorless.sources, 2);
        assert_eq!(colorless.pips, 0);
        assert_eq!(colorless.status, DeckManaStatus::NoDemand);
        assert_eq!(
            colorless.verdict,
            "Nothing in this deck needs colorless mana."
        );
    }

    /// A type line is read by its front face, like every other reader of one: a modal
    /// double-faced card whose back is a land is a nonland source (it is cast as its front
    /// or played as its back — either way a source), and a land // land pathway is a land.
    #[test]
    fn a_double_faced_type_line_is_read_by_its_front() {
        let base = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![
                    entry(
                        "mdfc",
                        "Shatterskull Smashing // Shatterskull, the Hammer Pass",
                        1,
                        2,
                        0,
                    )
                    .type_line("Sorcery // Land")
                    .mana_cost("{X}{R}{R}")
                    .produces("R"),
                    entry("path", "Riverglide Pathway // Lavaglide Pathway", 1, 3, 0)
                        .type_line("Land // Land")
                        .produces("U,R"),
                ],
            ),
        );
        assert_eq!(base.land_count, 3);
        let red = color(&base, "R");
        assert_eq!(red.sources, 5);
        assert_eq!(red.land_sources, 3);
        assert_eq!(red.nonland_sources, 2);
    }

    #[test]
    fn a_produced_mana_list_is_read_case_insensitively_and_once_per_colour() {
        let base = analyse_mana(
            None,
            &deck(
                vec![section(1, "Deck", false)],
                vec![
                    entry("odd", "Odd Land", 1, 2, 0)
                        .type_line("Land")
                        .produces("g,G,w"),
                ],
            ),
        );
        assert_eq!(
            color(&base, "G").sources,
            2,
            "one source per copy, not per listing"
        );
        assert_eq!(color(&base, "W").sources, 2);
    }
}
