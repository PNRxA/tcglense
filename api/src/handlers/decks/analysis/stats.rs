//! Deck **composition** and draw odds — the copy-weighted fold behind the deck page's
//! analytics panel, moved off the client (issue #596).
//!
//! Two folds over the same deck, because the panel answers two different questions:
//!
//! * `deck` — the deck **proper** (maybeboard excluded, issue #570). What "how big / what
//!   colour / what curve is this deck" means.
//! * `library` — only the sections a player would actually shuffle, which is what draw odds
//!   must be computed against. It defaults to everything that isn't a maybeboard, a command
//!   zone, or a sideboard, and a caller can override it (`?sections=`) to test a swap.
//!
//! The odds themselves are hypergeometric — P(at least one copy in N cards seen), without
//! replacement — and are returned as the whole **curve** over 1..=N rather than a single
//! number, so a slider is instant without the client re-deriving the maths that was just
//! moved here.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::handlers::decks::DeckSectionResponse;

use super::rules::DeckZone;
use super::{AnalysisEntry, DeckAnalysisInput};

/// The largest "cards seen" the odds curve runs to — a whole opening hand plus a long
/// game's worth of draw steps. Beyond this the number stops telling a deck builder
/// anything, and the curve stays a fixed, small payload.
const MAX_CARDS_SEEN: i64 = 30;

/// One bar of a distribution (a mana-value bucket, a colour, a card type).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckStatItem {
    /// Stable identifier for the bucket (`"3"`, `"W"`, `"Creature"`).
    pub key: String,
    /// Human label for the bucket (`"3"`, `"White"`, `"Creature"`).
    pub label: String,
    pub count: i64,
    /// Display hint — the hex swatch a colour bar is drawn in, for the buckets that have a
    /// canonical colour (the five mana colours plus colourless). Advisory: a client is free
    /// to ignore it, and a terminal client can use it to pick a nearby ANSI colour.
    pub color: Option<String>,
}

/// How many copies of one card **name** the pool holds — the list a draw-odds selector
/// offers, most-copied first.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckCardOdds {
    pub name: String,
    pub copies: i64,
}

/// The copy-weighted composition of a set of deck entries.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckComposition {
    /// Total copies (regular + foil) across every entry.
    pub total_copies: i64,
    /// Distinct printings — a name held in two arts counts twice.
    pub unique_cards: i64,
    pub land_copies: i64,
    /// Copy-weighted mean mana value over **nonlands**, or null when there are none.
    pub average_mana_value: Option<f64>,
    /// Nonland copies bucketed by mana value, `0`..`6` then `7+`.
    pub mana_curve: Vec<DeckStatItem>,
    /// Copies per colour of colour identity; a card counts once per colour it carries.
    pub colors: Vec<DeckStatItem>,
    /// Copies per card type; a card counts once per type on its front face.
    pub card_types: Vec<DeckStatItem>,
    /// Copies folded by card name, most-copied first.
    pub card_odds: Vec<DeckCardOdds>,
}

/// The hypergeometric draw odds for one card out of the library pool.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDrawOdds {
    /// The card name the odds are for.
    pub name: String,
    /// Copies of that name in the library pool.
    pub copies: i64,
    /// Size of the pool the odds are drawn from.
    pub library_size: i64,
    /// How many cards the `at_least_one` figure assumes were seen (clamped into range).
    pub cards_seen: i64,
    /// P(at least one copy) after seeing `cards_seen` cards.
    pub at_least_one: f64,
    /// The whole curve: `curve[i]` is P(at least one copy) after seeing `i + 1` cards, for
    /// 1..=min(30, library_size). Returned whole so a client can scrub a slider without
    /// another request — and so the maths stays in one place.
    pub curve: Vec<f64>,
}

/// Everything `GET /api/decks/{game}/{deck_id}/stats` answers.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckAnalytics {
    /// Composition of the deck proper — maybeboard sections excluded.
    pub deck: DeckComposition,
    /// Composition of the library pool the odds are drawn from.
    pub library: DeckComposition,
    /// The section ids that made up `library` for this request.
    pub library_section_ids: Vec<i32>,
    /// The section ids the library defaults to when the request names none — everything
    /// that isn't a maybeboard, a command zone, or a sideboard.
    pub default_library_section_ids: Vec<i32>,
    /// Draw odds for the requested card, or the most-copied one when none was named.
    /// Null only when the library pool is empty.
    pub odds: Option<DeckDrawOdds>,
}

/// Query string of the stats read.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct StatsParams {
    /// Comma-separated section ids to use as the shuffled library. Omit for the default
    /// selection; pass empty (`?sections=`) for none.
    pub sections: Option<String>,
    /// Card **name** to compute draw odds for. Defaults to the most-copied card.
    pub card: Option<String>,
    /// How many cards the headline probability assumes were seen (default 7, clamped to
    /// the library size and to 30).
    pub cards_seen: Option<i64>,
}

// ---------- The vocabulary the bars are drawn from ----------

/// Colour identity buckets, in WUBRG-then-colourless order with their display swatches.
const COLORS: &[(&str, &str, &str)] = &[
    ("W", "White", "#e5e7eb"),
    ("U", "Blue", "#3b82f6"),
    ("B", "Black", "#374151"),
    ("R", "Red", "#ef4444"),
    ("G", "Green", "#22c55e"),
    ("C", "Colorless", "#a1a1aa"),
];

/// Card types counted, in display order; anything matching none of them folds into
/// `Other`. Matched **word-exact against the raw type line**, so the capitalisation here is
/// load-bearing.
const CARD_TYPES: &[&str] = &[
    "Creature",
    "Artifact",
    "Enchantment",
    "Instant",
    "Sorcery",
    "Planeswalker",
    "Land",
    "Battle",
];

/// Sections included in draw odds — and in a goldfish hand — by default: the deck proper.
///
/// The zone split is [`super::rules::deck_zone`]'s, deliberately *not* a second list of
/// names. A section this called non-library while the rules called it the command zone
/// would deal an Oathbreaker deck its own oathbreaker and compute every probability against
/// a pool inflated by two — which is what a duplicated table drifts into. Maybeboards are
/// excluded by their **column** rather than by name (issue #570), so a renamed maybeboard
/// stays out and a section merely called "Considering" that the owner kept in the deck
/// stays in.
pub(crate) fn default_library_section_ids(sections: &[DeckSectionResponse]) -> Vec<i32> {
    sections
        .iter()
        .filter(|section| {
            !section.is_maybeboard && super::rules::deck_zone(&section.name) == DeckZone::Main
        })
        .map(|section| section.id)
        .collect()
}

// ---------- Composition ----------

/// The words on a card's front type line, as the raw line spells them. Case-sensitive by
/// design: the buckets above are the printed type names.
fn type_words(type_line: Option<&str>) -> HashSet<String> {
    let front = type_line
        .unwrap_or_default()
        .split("//")
        .next()
        .unwrap_or_default();
    front
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Fold a set of deck entries into their copy-weighted composition.
pub(crate) fn compose(entries: &[&AnalysisEntry]) -> DeckComposition {
    let mut unique_ids: BTreeSet<&str> = BTreeSet::new();
    let mut colors: BTreeMap<&str, i64> = BTreeMap::new();
    let mut types: BTreeMap<String, i64> = BTreeMap::new();
    let mut odds: BTreeMap<&str, i64> = BTreeMap::new();
    let mut curve = [0i64; 8];
    let mut total_copies = 0i64;
    let mut land_copies = 0i64;
    let mut mana_value_copies = 0i64;
    let mut mana_value_total = 0f64;

    for entry in entries {
        let copies = entry.copies();
        if copies == 0 {
            continue;
        }
        total_copies += copies;
        unique_ids.insert(entry.facts.id.as_str());
        *odds.entry(entry.facts.name.as_str()).or_default() += copies;

        // A colour counts once per entry however many times it appears on the row.
        let identity: BTreeSet<&str> = entry
            .facts
            .color_identity
            .iter()
            .map(String::as_str)
            .collect();
        if identity.is_empty() {
            *colors.entry("C").or_default() += copies;
        }
        for color in identity {
            *colors.entry(color).or_default() += copies;
        }

        let words = type_words(entry.facts.type_line.as_deref());
        let matched: Vec<&str> = CARD_TYPES
            .iter()
            .copied()
            .filter(|t| words.contains(*t))
            .collect();
        if matched.is_empty() {
            *types.entry("Other".to_string()).or_default() += copies;
        }
        for card_type in matched {
            *types.entry(card_type.to_string()).or_default() += copies;
        }

        let is_land = words.contains("Land");
        if is_land {
            land_copies += copies;
        }
        if let Some(mana_value) = entry.facts.cmc.filter(|v| v.is_finite())
            && !is_land
        {
            mana_value_copies += copies;
            mana_value_total += mana_value * copies as f64;
            let bucket = mana_value.floor().clamp(0.0, 7.0) as usize;
            curve[bucket] += copies;
        }
    }

    let mut card_odds: Vec<DeckCardOdds> = odds
        .into_iter()
        .map(|(name, copies)| DeckCardOdds {
            name: name.to_string(),
            copies,
        })
        .collect();
    // Most copies first, then by name. Compared case-insensitively before falling back to
    // the raw name, which is how the browser's `localeCompare` ordered this list.
    card_odds.sort_by(|left, right| {
        right
            .copies
            .cmp(&left.copies)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    DeckComposition {
        total_copies,
        unique_cards: unique_ids.len() as i64,
        land_copies,
        average_mana_value: (mana_value_copies > 0)
            .then(|| mana_value_total / mana_value_copies as f64),
        mana_curve: curve
            .iter()
            .enumerate()
            .map(|(index, count)| DeckStatItem {
                key: index.to_string(),
                label: if index == 7 {
                    "7+".to_string()
                } else {
                    index.to_string()
                },
                count: *count,
                color: None,
            })
            .collect(),
        colors: COLORS
            .iter()
            .filter_map(|(key, label, color)| {
                let count = colors.get(key).copied().unwrap_or(0);
                (count > 0).then(|| DeckStatItem {
                    key: (*key).to_string(),
                    label: (*label).to_string(),
                    count,
                    color: Some((*color).to_string()),
                })
            })
            .collect(),
        card_types: CARD_TYPES
            .iter()
            .copied()
            .chain(std::iter::once("Other"))
            .filter_map(|card_type| {
                let count = types.get(card_type).copied().unwrap_or(0);
                (count > 0).then(|| DeckStatItem {
                    key: card_type.to_string(),
                    label: card_type.to_string(),
                    count,
                    color: None,
                })
            })
            .collect(),
        card_odds,
    }
}

/// Hypergeometric P(at least one of `copies` in `cards_seen` draws) from a `deck_size`
/// pool, without replacement. Computed as the complement of drawing only misses, which
/// needs no factorials and cannot overflow.
pub(crate) fn draw_probability(deck_size: i64, copies: i64, cards_seen: i64) -> f64 {
    if deck_size <= 0 || copies <= 0 || cards_seen <= 0 {
        return 0.0;
    }
    let bounded_copies = copies.min(deck_size);
    let draws = cards_seen.min(deck_size);
    let mut miss = 1.0f64;
    for draw in 0..draws {
        let misses_left = deck_size - bounded_copies - draw;
        if misses_left <= 0 {
            return 1.0;
        }
        miss *= misses_left as f64 / (deck_size - draw) as f64;
    }
    1.0 - miss
}

// ---------- Entry point ----------

/// Parse a comma-separated `sections=` list into section ids, rejecting anything that
/// isn't a number rather than silently dropping it (a typo would otherwise read as a
/// smaller, plausible-looking library).
pub(crate) fn parse_section_ids(raw: &str) -> Result<Vec<i32>, AppError> {
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<i32>()
                .map_err(|_| AppError::Validation(format!("invalid section id: {part}")))
        })
        .collect()
}

/// Compute a deck's analytics: composition of the deck proper, composition of the library
/// pool, and the draw-odds curve for one card in it.
pub(crate) fn analyse_stats(
    input: &DeckAnalysisInput,
    params: &StatsParams,
) -> Result<DeckAnalytics, AppError> {
    let default_library_section_ids = default_library_section_ids(&input.sections);
    let library_section_ids = match params.sections.as_deref() {
        Some(raw) => parse_section_ids(raw)?,
        None => default_library_section_ids.clone(),
    };

    let deck = compose(&input.deck_proper());
    let library = compose(&input.in_sections(&library_section_ids));

    // The requested card, else the most-copied one — the same default the panel picks.
    let selected = params
        .card
        .as_deref()
        .and_then(|name| library.card_odds.iter().find(|item| item.name == name))
        .or_else(|| library.card_odds.first());

    let odds = selected.map(|item| {
        let max_cards_seen = library.total_copies.clamp(1, MAX_CARDS_SEEN);
        let cards_seen = params.cards_seen.unwrap_or(7).clamp(1, max_cards_seen);
        DeckDrawOdds {
            name: item.name.clone(),
            copies: item.copies,
            library_size: library.total_copies,
            cards_seen,
            at_least_one: draw_probability(library.total_copies, item.copies, cards_seen),
            curve: (1..=max_cards_seen)
                .map(|seen| draw_probability(library.total_copies, item.copies, seen))
                .collect(),
        }
    });

    Ok(DeckAnalytics {
        deck,
        library,
        library_section_ids,
        default_library_section_ids,
        odds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{entry, section};

    #[test]
    fn folds_copies_colours_types_and_curve() {
        let rows = [
            entry("a", "Bear", 1, 2, 0)
                .type_line("Creature — Bear")
                .colors("G")
                .cmc(2.0),
            entry("b", "Forest", 1, 10, 0)
                .type_line("Basic Land — Forest")
                .colors("")
                .cmc(0.0),
            entry("c", "Bolt", 1, 3, 1)
                .type_line("Instant")
                .colors("R")
                .cmc(1.0),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let stats = compose(&refs);

        assert_eq!(stats.total_copies, 2 + 10 + 4);
        assert_eq!(stats.unique_cards, 3);
        assert_eq!(stats.land_copies, 10);
        // Lands are excluded from the curve and the mean: (2*2 + 1*4) / 6.
        assert!((stats.average_mana_value.unwrap() - 8.0 / 6.0).abs() < 1e-9);
        assert_eq!(stats.mana_curve[1].count, 4);
        assert_eq!(stats.mana_curve[2].count, 2);
        assert_eq!(
            stats.mana_curve[0].count, 0,
            "the land does not bucket at 0"
        );

        let by_key = |items: &[DeckStatItem], key: &str| {
            items.iter().find(|i| i.key == key).map(|i| i.count)
        };
        assert_eq!(by_key(&stats.colors, "G"), Some(2));
        assert_eq!(by_key(&stats.colors, "R"), Some(4));
        assert_eq!(by_key(&stats.colors, "C"), Some(10));
        assert_eq!(by_key(&stats.card_types, "Creature"), Some(2));
        assert_eq!(by_key(&stats.card_types, "Land"), Some(10));
        assert_eq!(by_key(&stats.card_types, "Instant"), Some(4));

        // Odds fold by name, most-copied first.
        assert_eq!(stats.card_odds[0].name, "Forest");
        assert_eq!(stats.card_odds[0].copies, 10);
    }

    #[test]
    fn buckets_seven_and_above_together() {
        let rows = [
            entry("a", "Big", 1, 1, 0).type_line("Creature").cmc(7.0),
            entry("b", "Bigger", 1, 1, 0)
                .type_line("Creature")
                .cmc(12.0),
        ];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let stats = compose(&refs);
        assert_eq!(stats.mana_curve[7].label, "7+");
        assert_eq!(stats.mana_curve[7].count, 2);
    }

    #[test]
    fn a_typeless_card_folds_into_other() {
        let rows = [entry("a", "Mystery", 1, 1, 0).colors("W")];
        let refs: Vec<&AnalysisEntry> = rows.iter().collect();
        let stats = compose(&refs);
        assert_eq!(stats.card_types.len(), 1);
        assert_eq!(stats.card_types[0].key, "Other");
    }

    #[test]
    fn hypergeometric_matches_the_known_values() {
        // Four copies in a 60-card deck, seven cards seen: the number every deck builder
        // knows.
        assert!((draw_probability(60, 4, 7) - 0.3995).abs() < 5e-4);
        assert_eq!(draw_probability(60, 60, 1), 1.0);
        assert_eq!(draw_probability(0, 4, 7), 0.0);
        assert_eq!(draw_probability(60, 0, 7), 0.0);
        assert_eq!(draw_probability(60, 4, 0), 0.0);
    }

    #[test]
    fn library_defaults_exclude_maybeboards_and_the_command_zone() {
        let sections = [
            section(1, "Commander", false),
            section(2, "Creatures", false),
            section(3, "Sideboard", false),
            section(4, "Considering", false),
            section(5, "Cuts", true),
            // The four the old hand-written list missed, and the reason this now derives
            // from `deck_zone`: an oathbreaker dealt into its own opening hand is a wrong
            // hand, not a cosmetic difference.
            section(6, "Oathbreaker", false),
            section(7, "Oathbreakers", false),
            section(8, "Side Board", false),
            section(9, "Command Zone", false),
        ];
        // "Considering" is in — it's a name, not the column (issue #570).
        assert_eq!(default_library_section_ids(&sections), vec![2, 4]);
    }

    #[test]
    fn rejects_a_section_list_that_is_not_numbers() {
        assert!(parse_section_ids("1,2,3").is_ok());
        assert_eq!(parse_section_ids("").unwrap(), Vec::<i32>::new());
        assert!(parse_section_ids("1,two").is_err());
    }
}
