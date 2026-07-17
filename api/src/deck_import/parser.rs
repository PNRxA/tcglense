//! Uploaded Archidekt/Moxfield deck-list parsers (CSV and Moxfield-style plain text).

use regex::Regex;

use crate::collection_import::archidekt::is_foil_finish as archidekt_foil;
use crate::collection_import::csv_import::{find_column, parse_quantity, read_record};
use crate::collection_import::{ImportError, Provider};

use super::{DeckCardRow, DeckImportFileFormat, MAX_DECK_IMPORT_ROWS, ParsedDeck};

const ID_HEADERS: &[&str] = &["scryfall id", "scryfall_id", "scryfallid"];
const QUANTITY_HEADERS: &[&str] = &["quantity", "qty", "count"];
const NAME_HEADERS: &[&str] = &["name", "card", "card name"];
const FINISH_HEADERS: &[&str] = &["finish", "foil"];
const CATEGORY_HEADERS: &[&str] = &["categories", "category", "section", "board"];
const SET_HEADERS: &[&str] = &["edition", "edition code", "set", "set code"];
const NUMBER_HEADERS: &[&str] = &["collector number", "collector_number", "cn"];
const BOARD_HEADERS: &[&str] = &["board", "section", "category", "categories"];
const MAX_CELL: usize = 200;

pub fn parse_file(
    provider: Provider,
    format: DeckImportFileFormat,
    name: String,
    bytes: &[u8],
) -> Result<ParsedDeck, ImportError> {
    let rows = match format {
        DeckImportFileFormat::Csv => parse_csv(provider, bytes)?,
        DeckImportFileFormat::Text => parse_text(bytes)?,
    };
    Ok(ParsedDeck {
        provider,
        name,
        format: None,
        rows,
    })
}

fn parse_csv(provider: Provider, bytes: &[u8]) -> Result<Vec<DeckCardRow>, ImportError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|_| ImportError::InvalidSource("the uploaded file isn't a readable CSV".into()))?
        .clone();
    let quantity_idx = find_column(&headers, QUANTITY_HEADERS)
        .ok_or_else(|| missing_column("Quantity / Count"))?;
    let name_idx = find_column(&headers, NAME_HEADERS);
    let finish_idx = find_column(&headers, FINISH_HEADERS);
    let section_idx = find_column(
        &headers,
        match provider {
            Provider::Archidekt => CATEGORY_HEADERS,
            Provider::Moxfield => BOARD_HEADERS,
        },
    );
    let id_idx = find_column(&headers, ID_HEADERS);
    let set_idx = find_column(&headers, SET_HEADERS);
    let number_idx = find_column(&headers, NUMBER_HEADERS);

    if provider == Provider::Archidekt && id_idx.is_none() {
        return Err(missing_column("Scryfall ID"));
    }
    if provider == Provider::Moxfield
        && name_idx.is_none()
        && (set_idx.is_none() || number_idx.is_none())
    {
        return Err(missing_column("Name (or Edition + Collector Number)"));
    }

    let mut rows = Vec::new();
    let mut seen = 0usize;
    for record in reader.records() {
        let record = read_record(record, &mut seen)?;
        if seen > MAX_DECK_IMPORT_ROWS {
            return Err(ImportError::TooLarge {
                count: seen,
                max: MAX_DECK_IMPORT_ROWS,
            });
        }
        let Some(quantity) = record
            .get(quantity_idx)
            .and_then(parse_quantity)
            .filter(|quantity| *quantity > 0)
        else {
            continue;
        };
        let value = |idx: Option<usize>| {
            idx.and_then(|i| record.get(i))
                .map(str::trim)
                .filter(|cell| !cell.is_empty() && cell.chars().count() <= MAX_CELL)
        };
        let card_name = value(name_idx).unwrap_or("").to_string();
        let external_card_id = value(id_idx).map(str::to_string);
        let set_code = value(set_idx).map(str::to_ascii_lowercase);
        let collector_number = value(number_idx).map(str::to_string);
        if external_card_id.is_none()
            && (set_code.is_none() || collector_number.is_none())
            && card_name.is_empty()
        {
            continue;
        }
        rows.push(DeckCardRow {
            section: value(section_idx)
                .and_then(primary_section)
                .unwrap_or("Mainboard")
                .to_string(),
            card_name,
            external_card_id,
            set_code,
            collector_number,
            foil: value(finish_idx).is_some_and(|finish| match provider {
                Provider::Archidekt => archidekt_foil(false, Some(finish)),
                Provider::Moxfield => !finish.eq_ignore_ascii_case("nonfoil"),
            }),
            quantity,
        });
    }
    Ok(rows)
}

fn parse_text(bytes: &[u8]) -> Result<Vec<DeckCardRow>, ImportError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ImportError::InvalidSource("the uploaded deck list is not UTF-8 text".into())
    })?;
    // Moxfield's copy/export form is `1 Card Name (SET) 123 *F*`. Set/number are
    // optional here so simple `4 Lightning Bolt` lists still import by exact name.
    let printing = Regex::new(r"(?i)^(.+?)\s+\(([a-z0-9]+)\)\s+(\S+)$")
        .map_err(|_| ImportError::InvalidSource("couldn't initialize deck parser".into()))?;
    let mut section = "Mainboard".to_string();
    let mut rows = Vec::new();
    let mut lines_seen = 0usize;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = known_section_header(line) {
            section = header.to_string();
            continue;
        }
        let Some((quantity_token, remainder)) = line.split_once(char::is_whitespace) else {
            if let Some(header) = custom_section_header(line) {
                section = header;
            }
            continue;
        };
        let quantity_token = quantity_token.trim_end_matches(['x', 'X']);
        let Some(quantity) = parse_quantity(quantity_token).filter(|q| *q > 0) else {
            if let Some(header) = custom_section_header(line) {
                section = header;
            }
            continue;
        };
        lines_seen += 1;
        if lines_seen > MAX_DECK_IMPORT_ROWS {
            return Err(ImportError::TooLarge {
                count: lines_seen,
                max: MAX_DECK_IMPORT_ROWS,
            });
        }
        let mut card = remainder.trim();
        let foil = ["*F*", "*E*", "[foil]", "[etched]"]
            .iter()
            .find(|suffix| {
                card.to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
            })
            .is_some_and(|suffix| {
                card = card[..card.len() - suffix.len()].trim_end();
                true
            });
        let (card_name, set_code, collector_number) = match printing.captures(card) {
            Some(captures) => (
                captures[1].trim().to_string(),
                Some(captures[2].to_ascii_lowercase()),
                Some(captures[3].to_string()),
            ),
            None => (card.to_string(), None, None),
        };
        if card_name.is_empty() || card_name.chars().count() > MAX_CELL {
            continue;
        }
        rows.push(DeckCardRow {
            section: section.clone(),
            card_name,
            external_card_id: None,
            set_code,
            collector_number,
            foil,
            quantity,
        });
    }
    Ok(rows)
}

fn known_section_header(line: &str) -> Option<&'static str> {
    let normalized = line
        .trim_matches(['~', '/', ':', '[', ']'])
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "deck" | "main" | "mainboard" => Some("Mainboard"),
        "commander" | "commanders" => Some("Commander"),
        "sideboard" => Some("Sideboard"),
        "maybeboard" | "considering" => Some("Maybeboard"),
        "companion" | "companions" => Some("Companion"),
        _ => None,
    }
}

fn custom_section_header(line: &str) -> Option<String> {
    // A bracket-wrapped line is our own export's escape for names the bare grammar would
    // misread (see [`render_text_section_header`]): strip exactly the one wrapping pair so
    // bracket/trim characters belonging to the name survive verbatim.
    let header = match line
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        Some(inner) => inner.trim(),
        None => line.trim_matches(['~', '/', ':', '[', ']']).trim(),
    };
    (!header.is_empty() && header.chars().count() <= MAX_CELL).then(|| header.to_string())
}

/// Render `name` as a plain-text section header line that [`parse_text`] reads back as the
/// same section. A bare name is ambiguous when the grammar above claims it for something
/// else — a leading positive quantity ("2 Drops") reads as a card row, a leading `#` as a
/// comment, and edge characters from the header trim set (`~ / : [ ]`) get eaten — so those
/// wrap in one bracket pair, which [`custom_section_header`] strips back off verbatim.
/// Interior line breaks (storable through the section API) flatten to spaces so a header
/// can never leak an extra card-shaped line into the list. Names that reduce to a standard
/// board alias ("Deck", "Considering", …) still normalize to that board on re-import —
/// [`known_section_header`]'s deliberate normalization, not an escape gap. Everything else
/// stays bare, matching Moxfield's own copy/paste shape.
pub fn render_text_section_header(name: &str) -> String {
    let name = name.replace(['\r', '\n'], " ");
    let leading_quantity = name
        .split_once(char::is_whitespace)
        .and_then(|(quantity_token, _)| parse_quantity(quantity_token.trim_end_matches(['x', 'X'])))
        .is_some_and(|quantity| quantity > 0);
    let ambiguous = leading_quantity
        || name.starts_with(['#', '~', '/', ':', '[', ']'])
        || name.ends_with(['~', '/', ':', '[', ']']);
    if ambiguous { format!("[{name}]") } else { name }
}

fn primary_section(value: &str) -> Option<&str> {
    value
        .split([',', ';', '|'])
        .map(str::trim)
        .find(|section| !section.is_empty())
}

fn missing_column(name: &str) -> ImportError {
    ImportError::InvalidSource(format!(
        "the deck CSV is missing a required \"{name}\" column"
    ))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    #[test]
    fn parses_archidekt_categories_and_scryfall_ids() {
        let rows = parse_csv(
            Provider::Archidekt,
            b"Quantity,Name,Finish,Scryfall ID,Categories\n2,Sol Ring,Foil,uid-a,Ramp\n",
        )
        .expect("csv");
        assert_eq!(rows[0].section, "Ramp");
        assert_eq!(rows[0].external_card_id.as_deref(), Some("uid-a"));
        assert!(rows[0].foil);
    }

    #[test]
    fn uses_the_primary_archidekt_category() {
        let rows = parse_csv(
            Provider::Archidekt,
            b"Quantity,Name,Finish,Scryfall ID,Categories\n2,Sol Ring,,uid-a,\"Ramp, Draw\"\n",
        )
        .expect("csv");
        assert_eq!(rows[0].section, "Ramp");
    }

    #[test]
    fn parses_moxfield_csv_board_and_printing_tuple() {
        let rows = parse_csv(
            Provider::Moxfield,
            b"Count,Name,Edition,Foil,Collector Number,Board\n1,Sol Ring,C21,,263,Sideboard\n",
        )
        .expect("csv");
        assert_eq!(rows[0].section, "Sideboard");
        assert_eq!(rows[0].set_code.as_deref(), Some("c21"));
        assert!(!rows[0].foil);
    }

    #[test]
    fn parses_plain_text_boards_printings_and_foil() {
        let rows = parse_text(
            b"Commander\n1 Atraxa, Praetors' Voice (2X2) 190 *E*\n\nCustom pile\n4 Lightning Bolt\n",
        )
        .expect("text");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].section, "Commander");
        assert_eq!(rows[0].set_code.as_deref(), Some("2x2"));
        assert!(rows[0].foil);
        assert_eq!(rows[1].section, "Custom pile");
        assert_eq!(rows[1].card_name, "Lightning Bolt");
    }

    /// Section names the text grammar would misread — a leading quantity is a card row, a
    /// leading '#' a comment, edge trim-set characters get eaten — must export bracketed
    /// and parse back to the same section; unambiguous names stay bare.
    #[test]
    fn renders_ambiguous_section_headers_so_they_round_trip() {
        assert_eq!(render_text_section_header("Ramp"), "Ramp");
        assert_eq!(render_text_section_header("2 Drops"), "[2 Drops]");
        assert_eq!(render_text_section_header("3x Spells"), "[3x Spells]");
        assert_eq!(render_text_section_header("# Notes"), "[# Notes]");
        assert_eq!(render_text_section_header("Ramp:"), "[Ramp:]");
        // The escape is injective: a literal "[2 Drops]" cannot collide with the escaped
        // form of "2 Drops", so the two sections stay distinct on re-import.
        assert_eq!(render_text_section_header("[2 Drops]"), "[[2 Drops]]");
        // A stored line break flattens so a header can never leak a card-shaped line.
        assert_eq!(
            render_text_section_header("Notes\n4 Lightning Bolt"),
            "Notes 4 Lightning Bolt"
        );

        // Every rendered header parses back as exactly one section carrying the original
        // name; none is mistaken for a card row or dropped (which would misfile the card
        // into the previous section).
        let names = [
            "2 Drops",
            "3x Spells",
            "# Notes",
            "Ramp:",
            "[2 Drops]",
            "2 Drops [v2]",
            ":",
            "[]",
            "~",
        ];
        for name in names {
            let text = format!(
                "{}\n4 Cathar Commando (MID) 8\n",
                render_text_section_header(name)
            );
            let rows = parse_text(text.as_bytes()).expect("text");
            assert_eq!(
                rows.len(),
                1,
                "header for {name:?} must not parse as a card row"
            );
            assert_eq!(
                rows[0].section, name,
                "{name:?} must survive the round trip"
            );
            assert_eq!(rows[0].card_name, "Cathar Commando");
            assert_eq!(rows[0].quantity, 4);
        }
    }

    #[test]
    fn rejects_csv_above_the_deck_row_cap() {
        let mut csv = String::from("Quantity,Name,Scryfall ID,Categories\n");
        for index in 0..=MAX_DECK_IMPORT_ROWS {
            writeln!(csv, "1,Card {index},uid-{index},Mainboard").expect("write row");
        }
        let err = parse_csv(Provider::Archidekt, csv.as_bytes()).expect_err("oversized CSV");
        assert!(matches!(
            err,
            ImportError::TooLarge { count, max }
                if count == MAX_DECK_IMPORT_ROWS + 1 && max == MAX_DECK_IMPORT_ROWS
        ));
    }

    #[test]
    fn rejects_plain_text_above_the_deck_row_cap() {
        let mut text = String::new();
        for index in 0..=MAX_DECK_IMPORT_ROWS {
            writeln!(text, "1 Card {index}").expect("write row");
        }
        let err = parse_text(text.as_bytes()).expect_err("oversized text");
        assert!(matches!(
            err,
            ImportError::TooLarge { count, max }
                if count == MAX_DECK_IMPORT_ROWS + 1 && max == MAX_DECK_IMPORT_ROWS
        ));
    }
}
