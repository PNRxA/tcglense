//! The plain-text card-list line grammar, shared by the deck and collection importers.
//!
//! Nearly every Magic app — Moxfield's copy/paste box, Archidekt's text export, MTGA, and
//! Mythic Tools' TXT export of a box/binder/list — writes a card as
//!
//! ```text
//! 1 Sol Ring (C21) 263
//! 4x Lightning Bolt (2XM) 129 *F*
//! 2 Aang, Air Nomad [TLE] 146 [foil]
//! 3 Counterspell
//! ```
//!
//! i.e. a quantity, a name, an optional `(SET) number` printing key, and an optional foil
//! marker. This module owns exactly that one line's grammar so the deck importer
//! ([`crate::deck_import::parser`], which additionally tracks section headers) and the
//! collection importer ([`super::execute_file_import`], which has no sections) read the
//! same dialect instead of each growing their own.
//!
//! Everything here is pure and total: a line that isn't a card row is reported as such
//! ([`TextListLine::NotACard`]) so the caller can decide what it means — a section header
//! for a deck, a line to ignore for a collection — and a card-shaped line whose name is
//! unusable still *counts* as a row ([`TextListLine::Card(None)`]) so the caller's row cap
//! bounds work by lines consumed, not by lines that happened to be well-formed.

use regex::Regex;
use std::sync::OnceLock;

use super::csv_import::parse_quantity;

/// Longest card name we'll carry off a text line. Matches the CSV parsers' cell bound; a
/// longer "name" is a pathological line, not a card.
pub(crate) const MAX_LIST_NAME: usize = 200;

/// Foil markers a text list may append to a card line, in the spellings the apps emit.
/// Matched case-insensitively; any of them puts the row in our single "foil" bucket
/// (etched included — the same two-bucket model the CSV parsers use).
const FOIL_SUFFIXES: &[&str] = &["*F*", "*E*", "[foil]", "[etched]"];

/// One card line, already split into its parts. `set_code` / `collector_number` are
/// present only when the line carried a `(SET) number` printing key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextListRow {
    pub(crate) name: String,
    /// Set code, lowercased (the catalog stores lowercase codes).
    pub(crate) set_code: Option<String>,
    /// Collector number as printed, trimmed — compared exactly (`"12a"`, `"XLN-217"`).
    pub(crate) collector_number: Option<String>,
    pub(crate) foil: bool,
    pub(crate) quantity: i32,
}

/// What one line of a text list is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextListLine {
    /// The line doesn't start with a positive quantity, so it isn't a card row. The deck
    /// importer treats these as section headers; the collection importer ignores them.
    NotACard,
    /// A card-shaped line: it starts with a positive quantity, so it consumes one row of
    /// the caller's budget. `None` when the rest of the line held no usable name.
    Card(Option<TextListRow>),
}

/// `Name (SET) 123` — the printing key every app appends after the card name. The set code
/// is alphanumeric; the collector number is the rest of the line (numbers can carry
/// letters and dashes, e.g. `12a` / `XLN-217`). Bracketed set codes (`Name [SET] 123`) are
/// accepted too — some exports, Mythic Tools' among them, use those; the delimiters must
/// match, so `Name (SET] 123` is read as a plain name, not a printing key.
fn printing_patterns() -> &'static [Regex; 2] {
    static PATTERNS: OnceLock<[Regex; 2]> = OnceLock::new();
    // Both literals are valid regexes, so these compile on first use and never again.
    PATTERNS.get_or_init(|| {
        [
            Regex::new(r"(?i)^(.+?)\s+\(([a-z0-9]+)\)\s+(\S+)$").expect("static regex compiles"),
            Regex::new(r"(?i)^(.+?)\s+\[([a-z0-9]+)\]\s+(\S+)$").expect("static regex compiles"),
        ]
    })
}

/// Classify one already-trimmed, non-empty, non-comment line of a text card list.
///
/// The caller is responsible for skipping blank / `#` comment lines and for enforcing its
/// own row cap over the [`TextListLine::Card`] results.
pub(crate) fn parse_line(line: &str) -> TextListLine {
    let Some((quantity_token, remainder)) = line.split_once(char::is_whitespace) else {
        return TextListLine::NotACard;
    };
    // "4x Lightning Bolt" is as common as "4 Lightning Bolt".
    let quantity_token = quantity_token.trim_end_matches(['x', 'X']);
    let Some(quantity) = parse_quantity(quantity_token).filter(|quantity| *quantity > 0) else {
        return TextListLine::NotACard;
    };

    let mut card = remainder.trim();
    let lowered = card.to_ascii_lowercase();
    let foil = FOIL_SUFFIXES
        .iter()
        .find(|suffix| lowered.ends_with(&suffix.to_ascii_lowercase()))
        .is_some_and(|suffix| {
            card = card[..card.len() - suffix.len()].trim_end();
            true
        });

    let printing = printing_patterns()
        .iter()
        .find_map(|pattern| pattern.captures(card));
    let (name, set_code, collector_number) = match printing {
        Some(captures) => (
            captures[1].trim().to_string(),
            Some(captures[2].to_ascii_lowercase()),
            Some(captures[3].to_string()),
        ),
        None => (card.to_string(), None, None),
    };
    if name.is_empty() || name.chars().count() > MAX_LIST_NAME {
        return TextListLine::Card(None);
    }
    TextListLine::Card(Some(TextListRow {
        name,
        set_code,
        collector_number,
        foil,
        quantity,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(line: &str) -> TextListRow {
        match parse_line(line) {
            TextListLine::Card(Some(row)) => row,
            other => panic!("expected a card row for {line:?}, got {other:?}"),
        }
    }

    #[test]
    fn parses_quantity_name_printing_and_foil() {
        let row = card("1 Atraxa, Praetors' Voice (2X2) 190 *E*");
        assert_eq!(row.quantity, 1);
        assert_eq!(row.name, "Atraxa, Praetors' Voice");
        assert_eq!(row.set_code.as_deref(), Some("2x2"), "set codes lowercase");
        assert_eq!(row.collector_number.as_deref(), Some("190"));
        assert!(row.foil, "etched is a foil in our two-bucket model");
    }

    #[test]
    fn accepts_the_x_quantity_spelling_and_bracketed_set_codes() {
        let row = card("4x Lightning Bolt [2XM] 129 [Foil]");
        assert_eq!(row.quantity, 4);
        assert_eq!(row.name, "Lightning Bolt");
        assert_eq!(row.set_code.as_deref(), Some("2xm"));
        assert_eq!(row.collector_number.as_deref(), Some("129"));
        assert!(row.foil, "the marker is matched case-insensitively");
    }

    #[test]
    fn a_name_only_line_carries_no_printing_key() {
        let row = card("3 Counterspell");
        assert_eq!(row.quantity, 3);
        assert_eq!(row.name, "Counterspell");
        assert_eq!(row.set_code, None);
        assert_eq!(row.collector_number, None);
        assert!(!row.foil);
    }

    #[test]
    fn keeps_letters_and_dashes_in_a_collector_number() {
        assert_eq!(
            card("1 Sol Ring (XLN) 217a").collector_number.as_deref(),
            Some("217a")
        );
        assert_eq!(
            card("1 Sol Ring (PLST) XLN-217")
                .collector_number
                .as_deref(),
            Some("XLN-217")
        );
    }

    #[test]
    fn a_line_without_a_leading_positive_quantity_is_not_a_card() {
        // The deck importer reads these as section headers; the collection importer skips
        // them. Either way they must never consume a row of the caller's budget.
        assert_eq!(parse_line("Commander"), TextListLine::NotACard);
        assert_eq!(parse_line("Sol Ring"), TextListLine::NotACard);
        assert_eq!(parse_line("0 Sol Ring"), TextListLine::NotACard);
        assert_eq!(parse_line("-2 Sol Ring"), TextListLine::NotACard);
    }

    #[test]
    fn a_card_shaped_line_with_an_unusable_name_still_counts_as_a_row() {
        // Counted (so it's bounded by the caller's cap) but carries nothing to import.
        assert_eq!(
            parse_line("1 *F*"),
            TextListLine::Card(None),
            "name is only a marker"
        );
        let long = format!("1 {}", "n".repeat(MAX_LIST_NAME + 1));
        assert_eq!(parse_line(&long), TextListLine::Card(None));
    }

    #[test]
    fn mismatched_printing_delimiters_are_not_a_printing_key() {
        // `(TLE]` isn't a set code either app would write — read it as part of the name
        // rather than inventing a printing the user didn't ask for.
        let row = card("1 Sol Ring (TLE] 146");
        assert_eq!(row.set_code, None);
        assert_eq!(row.name, "Sol Ring (TLE] 146");
    }

    #[test]
    fn a_huge_quantity_saturates_rather_than_overflowing() {
        assert_eq!(card("999999999999 Sol Ring").quantity, i32::MAX);
    }
}
