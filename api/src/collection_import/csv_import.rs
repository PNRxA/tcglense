//! CSV collection parsers (Archidekt + Moxfield exports).
//!
//! Both services offer a CSV "export collection", but their shapes differ:
//!
//! * **Archidekt** rows carry a **Scryfall ID** (our `cards.external_id`) plus a
//!   **Finish** and a **Quantity**; those three columns are all we read (extra columns
//!   are ignored, so a trimmed or full export both work).
//! * **Moxfield** rows carry **no card id at all** — a printing is identified by its
//!   **Edition** (Scryfall set code) + **Collector Number**, with the finish in a
//!   **Foil** column (blank / `foil` / `etched`) and the owned count in **Count**.
//!   Those rows resolve to catalog cards by `(set_code, collector_number)` (see
//!   [`super::execute_csv_import`]).
//!
//! [`parse_csv`] sniffs which shape an upload is from its header row: a Scryfall-id
//! column means Archidekt; otherwise Edition + Collector Number + Count means Moxfield.
//! (Detection must run in that order — Archidekt's quantity column also accepts a
//! "Count" spelling.)
//!
//! Every byte here is an untrusted upload, so parsing is deliberately defensive:
//!
//! * The `csv` crate handles quoting / escaping / embedded newlines, so a comma or
//!   newline inside a card name can't desync the columns.
//! * A record read error (e.g. invalid UTF-8 / a binary file masquerading as CSV) fails
//!   the whole import with a clear 422 rather than being silently reinterpreted.
//! * The number of rows we'll process is capped at [`MAX_IMPORT_ROWS`]; the request-body
//!   size is capped separately by the handler's body limit, so parse work is bounded
//!   twice over.
//! * Each id / set code / collector number is length-bounded and non-blank before it's
//!   used; a malformed or pathological value simply doesn't resolve against the catalog
//!   (values are only ever bound SQL parameters downstream, never interpolated), so an
//!   unknown one is skipped, never trusted.
//! * Quantities are parsed as integers and non-positive / unparseable rows are dropped;
//!   the reconcile engine additionally clamps the aggregated totals.
//!
//! The Archidekt output is the same normalized `Vec<FetchedHolding>` a network provider
//! yields; the Moxfield output is a `Vec<MoxfieldCsvRow>` that becomes `FetchedHolding`s
//! once its set/number pairs are resolved to external ids. Either way the
//! provider-independent aggregate / resolve / reconcile / apply path is reused as-is.

use super::archidekt::is_foil_finish;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS};

/// Longest Scryfall id we'll accept from a CSV cell. A Scryfall id is a 36-char UUID;
/// this leaves generous headroom while rejecting a pathologically long cell before it
/// reaches the (chunked, parameterised) catalog lookup.
const MAX_ID_LEN: usize = 64;

/// Longest set code / collector number we'll accept from a Moxfield CSV cell. Scryfall
/// set codes run 3–6 chars and collector numbers ~1–8 (`"XLN-217"`, `"12a"`); 32 leaves
/// generous headroom while bounding a pathological cell.
const MAX_SET_OR_NUMBER_LEN: usize = 32;

/// Longest card name we'll carry from a Moxfield CSV cell (names are only used to label
/// unmatched cards in the summary, so a pathological one is truncated, not rejected).
const MAX_NAME_LEN: usize = 200;

/// Header spellings (compared case- and whitespace-insensitively) that name the Scryfall
/// id column. Archidekt writes "Scryfall ID"; the aliases tolerate minor variations.
const ID_HEADERS: &[&str] = &["scryfall id", "scryfall_id", "scryfallid"];
/// Header spellings for the finish column ("Finish").
const FINISH_HEADERS: &[&str] = &["finish"];
/// Header spellings for the quantity column ("Quantity").
const QUANTITY_HEADERS: &[&str] = &["quantity", "qty", "count"];

/// Moxfield header spellings: the owned count ("Count"), the set code ("Edition"), the
/// collector number, the finish ("Foil": blank / `foil` / `etched`), and the optional
/// name (summary labels) and proxy flag (proxies aren't real cards, so they're skipped).
const MOX_COUNT_HEADERS: &[&str] = &["count"];
const MOX_EDITION_HEADERS: &[&str] = &["edition", "edition code", "set", "set code"];
const MOX_NUMBER_HEADERS: &[&str] = &["collector number", "collector_number", "cn"];
const MOX_FOIL_HEADERS: &[&str] = &["foil", "finish"];
const MOX_NAME_HEADERS: &[&str] = &["name"];
const MOX_PROXY_HEADERS: &[&str] = &["proxy"];

/// A parsed CSV upload, tagged with which provider's export shape it matched.
#[derive(Debug)]
pub(super) enum ParsedCsv {
    /// Archidekt rows resolve by Scryfall id, so they're already normalized holdings.
    Archidekt(Vec<FetchedHolding>),
    /// Moxfield rows carry no card id; they resolve by `(set_code, collector_number)`.
    Moxfield(Vec<MoxfieldCsvRow>),
}

/// One usable row of a Moxfield collection export, pre-normalized (set code lowercased,
/// number trimmed) but not yet resolved to a catalog card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoxfieldCsvRow {
    /// Scryfall set code, lowercased (the catalog stores lowercase codes).
    pub(crate) set_code: String,
    /// Collector number as printed, trimmed. Compared exactly — Scryfall numbers can
    /// carry letters/symbols (`"12a"`, `"XLN-217"`) that must not be normalized away.
    pub(crate) collector_number: String,
    /// Card name, only used to label unmatched cards in the import summary.
    pub(crate) name: String,
    pub(crate) foil: bool,
    pub(crate) quantity: i32,
}

/// Parse an uploaded collection CSV, sniffing the provider from the header row.
///
/// Returns the parsed rows (possibly empty if no row was usable — the caller maps an
/// empty result to [`ImportError::EmptyCollection`]). Fails with
/// [`ImportError::InvalidSource`] (→ 422) when the file isn't a CSV we can read or its
/// header matches neither export shape.
pub(super) fn parse_csv(bytes: &[u8]) -> Result<ParsedCsv, ImportError> {
    // Strip a leading UTF-8 BOM if present, so the first header ("Quantity") isn't read
    // as "\u{feff}Quantity" and thus fails to match. Some spreadsheet exporters add one.
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        // Tolerate ragged rows (a row with more/fewer fields than the header) rather than
        // erroring — we address the columns we need by index and ignore the rest.
        .flexible(true)
        .from_reader(bytes);

    let headers = reader
        .headers()
        .map_err(|_| ImportError::InvalidSource("the uploaded file isn't a readable CSV".into()))?
        .clone();

    // An id column pins the shape to Archidekt — checked first because Archidekt's
    // quantity column also matches Moxfield's "Count" spelling.
    if let Some(id_idx) = find_column(&headers, ID_HEADERS) {
        return parse_archidekt_rows(reader, &headers, id_idx).map(ParsedCsv::Archidekt);
    }
    // No id column: a Moxfield export identifies printings by set + collector number.
    if let (Some(count_idx), Some(edition_idx), Some(number_idx)) = (
        find_column(&headers, MOX_COUNT_HEADERS),
        find_column(&headers, MOX_EDITION_HEADERS),
        find_column(&headers, MOX_NUMBER_HEADERS),
    ) {
        return parse_moxfield_rows(reader, &headers, count_idx, edition_idx, number_idx)
            .map(ParsedCsv::Moxfield);
    }
    Err(ImportError::InvalidSource(
        "the CSV doesn't look like a collection export we support — export from Archidekt \
         (with the Scryfall ID, Finish, and Quantity columns) or use Moxfield's standard \
         collection export (with the Count, Edition, Collector Number, and Foil columns)"
            .to_string(),
    ))
}

/// Parse the data rows of an Archidekt-shaped CSV (a Scryfall id per row).
fn parse_archidekt_rows(
    mut reader: csv::Reader<&[u8]>,
    headers: &csv::StringRecord,
    id_idx: usize,
) -> Result<Vec<FetchedHolding>, ImportError> {
    let finish_idx = find_column(headers, FINISH_HEADERS).ok_or_else(|| missing_column("Finish"))?;
    let quantity_idx =
        find_column(headers, QUANTITY_HEADERS).ok_or_else(|| missing_column("Quantity"))?;

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut rows_seen = 0usize;
    for record in reader.records() {
        let record = read_record(record, &mut rows_seen)?;

        // A row missing any of our columns (short row) just contributes nothing.
        let external_card_id = match record.get(id_idx).map(str::trim) {
            Some(id) if !id.is_empty() && id.len() <= MAX_ID_LEN => id.to_string(),
            _ => continue,
        };
        let Some(quantity) = positive_quantity(&record, quantity_idx) else {
            continue;
        };
        let foil = is_foil_finish(false, record.get(finish_idx));

        holdings.push(FetchedHolding {
            external_card_id,
            foil,
            quantity,
        });
    }

    Ok(holdings)
}

/// Parse the data rows of a Moxfield-shaped CSV (set code + collector number per row).
fn parse_moxfield_rows(
    mut reader: csv::Reader<&[u8]>,
    headers: &csv::StringRecord,
    count_idx: usize,
    edition_idx: usize,
    number_idx: usize,
) -> Result<Vec<MoxfieldCsvRow>, ImportError> {
    // The Foil column is required: silently importing a foil collection as regular
    // copies would corrupt counts, and Moxfield's export always includes it.
    let foil_idx = find_column(headers, MOX_FOIL_HEADERS).ok_or_else(|| missing_column("Foil"))?;
    // Name and Proxy are optional refinements — an export without them still imports.
    let name_idx = find_column(headers, MOX_NAME_HEADERS);
    let proxy_idx = find_column(headers, MOX_PROXY_HEADERS);

    let mut rows: Vec<MoxfieldCsvRow> = Vec::new();
    let mut rows_seen = 0usize;
    for record in reader.records() {
        let record = read_record(record, &mut rows_seen)?;

        // A proxy isn't a real card — importing it would inflate the collection (and
        // set completion) with copies the user doesn't own.
        if let Some(idx) = proxy_idx
            && record
                .get(idx)
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        {
            continue;
        }

        let set_code = match record.get(edition_idx).map(str::trim) {
            Some(s) if !s.is_empty() && s.len() <= MAX_SET_OR_NUMBER_LEN => {
                s.to_ascii_lowercase()
            }
            _ => continue,
        };
        let collector_number = match record.get(number_idx).map(str::trim) {
            Some(n) if !n.is_empty() && n.len() <= MAX_SET_OR_NUMBER_LEN => n.to_string(),
            _ => continue,
        };
        let Some(quantity) = positive_quantity(&record, count_idx) else {
            continue;
        };
        // Blank = regular; any other value (`foil`, `etched`, …) is a foil finish in our
        // two-bucket model — the same rule as Archidekt's modifier (see `is_foil_finish`).
        let foil = is_foil_finish(false, record.get(foil_idx));
        let name = name_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .unwrap_or("")
            .chars()
            .take(MAX_NAME_LEN)
            .collect();

        rows.push(MoxfieldCsvRow {
            set_code,
            collector_number,
            name,
            foil,
            quantity,
        });
    }

    Ok(rows)
}

/// Unwrap one CSV record, mapping a read error (e.g. invalid UTF-8 / a binary upload) to
/// a clear 422 — never a partial, silently-truncated import — and enforcing the row cap.
pub(crate) fn read_record(
    record: Result<csv::StringRecord, csv::Error>,
    rows_seen: &mut usize,
) -> Result<csv::StringRecord, ImportError> {
    let record =
        record.map_err(|_| ImportError::InvalidSource("the CSV could not be parsed".into()))?;
    *rows_seen += 1;
    if *rows_seen > MAX_IMPORT_ROWS {
        return Err(ImportError::TooLarge {
            count: *rows_seen,
            max: MAX_IMPORT_ROWS,
        });
    }
    Ok(record)
}

/// The row's quantity cell as a positive count, or `None` (skip the row) when blank,
/// unparseable, or non-positive.
fn positive_quantity(record: &csv::StringRecord, idx: usize) -> Option<i32> {
    match record.get(idx).and_then(parse_quantity) {
        Some(q) if q > 0 => Some(q),
        _ => None,
    }
}

/// The 0-based index of the first header matching any of `names` (compared with
/// [`normalize`]), or `None` if the CSV has no such column.
pub(crate) fn find_column(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|h| names.contains(&normalize(h).as_str()))
}

/// Normalise a header cell for matching: trim surrounding whitespace and lowercase it, so
/// "Scryfall ID", " scryfall id " and "SCRYFALL ID" all compare equal.
fn normalize(header: &str) -> String {
    header.trim().to_ascii_lowercase()
}

/// Parse a quantity cell to a positive-or-zero `i32`. Trims first; `None` for a blank or
/// unparseable cell (so the row is skipped) — a huge count saturates to `i32::MAX` and is
/// clamped again by the reconcile engine.
pub(crate) fn parse_quantity(cell: &str) -> Option<i32> {
    let cell = cell.trim();
    if cell.is_empty() {
        return None;
    }
    // Accept a plain integer; a decimal or garbage cell is treated as "no quantity".
    match cell.parse::<i64>() {
        Ok(n) => Some(n.clamp(0, i64::from(i32::MAX)) as i32),
        Err(_) => None,
    }
}

/// A 422 for a CSV whose shape we recognised but which is missing a column we need.
fn missing_column(name: &str) -> ImportError {
    ImportError::InvalidSource(format!(
        "the CSV is missing a required \"{name}\" column — export from Archidekt with the \
         Scryfall ID, Finish, and Quantity columns, or use Moxfield's standard collection \
         export (Count, Edition, Collector Number, Foil)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A UUID-shaped Scryfall id for readable fixtures.
    const UID_A: &str = "f369827d-e4cd-4bc7-8c5e-72882eff0908";
    const UID_B: &str = "50a22ad6-d2a4-48a6-91c9-147c946a60a5";

    /// Unwrap an Archidekt-detected parse (most fixtures here are Archidekt-shaped).
    fn parse_archidekt(bytes: &[u8]) -> Result<Vec<FetchedHolding>, ImportError> {
        match parse_csv(bytes)? {
            ParsedCsv::Archidekt(holdings) => Ok(holdings),
            ParsedCsv::Moxfield(_) => panic!("fixture unexpectedly sniffed as Moxfield"),
        }
    }

    /// Unwrap a Moxfield-detected parse.
    fn parse_moxfield(bytes: &[u8]) -> Result<Vec<MoxfieldCsvRow>, ImportError> {
        match parse_csv(bytes)? {
            ParsedCsv::Moxfield(rows) => Ok(rows),
            ParsedCsv::Archidekt(_) => panic!("fixture unexpectedly sniffed as Archidekt"),
        }
    }

    fn holding<'a>(holdings: &'a [FetchedHolding], id: &str) -> Option<&'a FetchedHolding> {
        holdings.iter().find(|h| h.external_card_id == id)
    }

    #[test]
    fn parses_the_three_required_columns_and_ignores_the_rest() {
        // A trimmed-down slice of a real Archidekt export: the three columns we use are
        // interleaved with columns we ignore, and a card name contains a comma (quoted).
        let csv = "Quantity,Name,Finish,Condition,Scryfall ID,Rarity\r\n\
                   2,\"Aang, Air Nomad\",Foil,NM,f369827d-e4cd-4bc7-8c5e-72882eff0908,rare\r\n\
                   3,Sol Ring,Normal,NM,50a22ad6-d2a4-48a6-91c9-147c946a60a5,uncommon\r\n";
        let holdings = parse_archidekt(csv.as_bytes()).expect("parse");
        assert_eq!(holdings.len(), 2);
        let a = holding(&holdings, UID_A).expect("uid a present");
        assert!(a.foil, "\"Foil\" finish is a foil");
        assert_eq!(a.quantity, 2, "the embedded comma in the name didn't desync columns");
        let b = holding(&holdings, UID_B).expect("uid b present");
        assert!(!b.foil, "\"Normal\" finish is a regular card");
        assert_eq!(b.quantity, 3);
    }

    #[test]
    fn header_matching_is_case_and_whitespace_insensitive_and_column_order_free() {
        // Columns in a different order, with odd casing / spacing on the headers.
        let csv = " SCRYFALL ID , finish ,QUANTITY\n\
                    f369827d-e4cd-4bc7-8c5e-72882eff0908,Etched,4\n";
        let holdings = parse_archidekt(csv.as_bytes()).expect("parse");
        assert_eq!(holdings.len(), 1);
        let a = &holdings[0];
        assert_eq!(a.external_card_id, UID_A);
        assert!(a.foil, "Etched (any non-Normal finish) is a foil in our model");
        assert_eq!(a.quantity, 4);
    }

    #[test]
    fn strips_a_leading_utf8_bom() {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(
            b"Quantity,Finish,Scryfall ID\n1,Normal,f369827d-e4cd-4bc7-8c5e-72882eff0908\n",
        );
        let holdings = parse_archidekt(&bytes).expect("parse with BOM");
        assert_eq!(holdings.len(), 1, "the BOM didn't break header matching");
        assert_eq!(holdings[0].quantity, 1);
    }

    #[test]
    fn missing_a_required_column_is_a_validation_error() {
        // A Scryfall ID column pins the shape to Archidekt, but there's no Finish column.
        let csv = "Quantity,Scryfall ID\n1,f369827d-e4cd-4bc7-8c5e-72882eff0908\n";
        let err = parse_csv(csv.as_bytes()).expect_err("missing Finish must fail");
        assert!(matches!(err, ImportError::InvalidSource(_)));
    }

    #[test]
    fn an_unrecognisable_header_names_both_supported_formats() {
        // Neither an id column nor Moxfield's set/number columns.
        let csv = "Name,Amount\nSol Ring,3\n";
        let err = parse_csv(csv.as_bytes()).expect_err("unknown shape must fail");
        let ImportError::InvalidSource(msg) = err else {
            panic!("expected InvalidSource");
        };
        assert!(msg.contains("Archidekt"), "names the Archidekt format: {msg}");
        assert!(msg.contains("Moxfield"), "names the Moxfield format: {msg}");
    }

    #[test]
    fn skips_rows_with_blank_bad_or_oversized_ids_and_non_positive_quantities() {
        let long_id = "a".repeat(MAX_ID_LEN + 1);
        let csv = format!(
            "Scryfall ID,Finish,Quantity\n\
             ,Normal,1\n\
             {long_id},Normal,1\n\
             f369827d-e4cd-4bc7-8c5e-72882eff0908,Normal,0\n\
             50a22ad6-d2a4-48a6-91c9-147c946a60a5,Normal,-3\n\
             50a22ad6-d2a4-48a6-91c9-147c946a60a5,Normal,notanumber\n\
             f369827d-e4cd-4bc7-8c5e-72882eff0908,Normal,2\n"
        );
        let holdings = parse_archidekt(csv.as_bytes()).expect("parse");
        // Only the last row (a valid id + positive quantity) survives.
        assert_eq!(holdings.len(), 1);
        assert_eq!(holdings[0].external_card_id, UID_A);
        assert_eq!(holdings[0].quantity, 2);
    }

    #[test]
    fn an_empty_or_header_only_csv_yields_no_holdings() {
        // Header only.
        let holdings = parse_archidekt(b"Scryfall ID,Finish,Quantity\n").expect("parse");
        assert!(holdings.is_empty());
    }

    #[test]
    fn a_non_utf8_body_is_rejected_not_partially_imported() {
        // A valid header row, then a byte sequence that isn't valid UTF-8 in a data row.
        let mut bytes = b"Scryfall ID,Finish,Quantity\n".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe, b',', b'N', b',', b'1', b'\n']);
        let err = parse_csv(&bytes).expect_err("invalid UTF-8 must fail");
        assert!(matches!(err, ImportError::InvalidSource(_)));
    }

    #[test]
    fn a_huge_quantity_saturates_rather_than_overflowing() {
        let csv = "Scryfall ID,Finish,Quantity\n\
                   f369827d-e4cd-4bc7-8c5e-72882eff0908,Normal,999999999999\n";
        let holdings = parse_archidekt(csv.as_bytes()).expect("parse");
        assert_eq!(holdings[0].quantity, i32::MAX, "saturates, never wraps");
    }

    // ---- Moxfield-shaped exports ----

    /// The header row of a real Moxfield collection export.
    const MOX_HEADER: &str = "\"Count\",\"Tradelist Count\",\"Name\",\"Edition\",\"Condition\",\
                              \"Language\",\"Foil\",\"Tags\",\"Last Modified\",\
                              \"Collector Number\",\"Alter\",\"Proxy\",\"Purchase Price\"";

    #[test]
    fn parses_a_real_shaped_moxfield_export() {
        // Real rows from a Moxfield export: quoted cells, a blank Foil (regular), a
        // "foil" Foil, and a double-faced name with an embedded comma + slashes.
        let csv = format!(
            "{MOX_HEADER}\n\
             \"1\",\"1\",\"Aang, A Lot to Learn\",\"tle\",\"Near Mint\",\"English\",\"foil\",\"\",\"2026-07-01\",\"146\",\"False\",\"False\",\"\"\n\
             \"2\",\"0\",\"Aang, at the Crossroads // Aang, Destined Savior\",\"TLA\",\"Near Mint\",\"English\",\"\",\"\",\"2026-07-01\",\"203\",\"False\",\"False\",\"\"\n"
        );
        let rows = parse_moxfield(csv.as_bytes()).expect("parse");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            MoxfieldCsvRow {
                set_code: "tle".to_string(),
                collector_number: "146".to_string(),
                name: "Aang, A Lot to Learn".to_string(),
                foil: true,
                quantity: 1,
            }
        );
        assert_eq!(rows[1].set_code, "tla", "set codes are lowercased");
        assert_eq!(rows[1].collector_number, "203");
        assert!(!rows[1].foil, "a blank Foil cell is a regular card");
        assert_eq!(rows[1].quantity, 2, "Count (not Tradelist Count) is the owned count");
    }

    #[test]
    fn moxfield_etched_finish_is_a_foil() {
        let csv = "Count,Name,Edition,Foil,Collector Number\n\
                   1,Sol Ring,c21,etched,263\n";
        let rows = parse_moxfield(csv.as_bytes()).expect("parse");
        assert!(rows[0].foil, "etched is a foil in our two-bucket model");
    }

    #[test]
    fn moxfield_proxies_are_skipped() {
        let csv = "Count,Name,Edition,Foil,Collector Number,Proxy\n\
                   1,Black Lotus,lea,,232,True\n\
                   1,Sol Ring,c21,,263,False\n";
        let rows = parse_moxfield(csv.as_bytes()).expect("parse");
        assert_eq!(rows.len(), 1, "the proxy row is dropped");
        assert_eq!(rows[0].name, "Sol Ring");
    }

    #[test]
    fn moxfield_rows_with_blank_or_oversized_keys_or_bad_counts_are_skipped() {
        let long = "x".repeat(MAX_SET_OR_NUMBER_LEN + 1);
        let csv = format!(
            "Count,Name,Edition,Foil,Collector Number\n\
             1,No Set,,,1\n\
             1,No Number,tle,,\n\
             1,Long Set,{long},,1\n\
             0,Zero Count,tle,,2\n\
             x,Bad Count,tle,,3\n\
             2,Keeper,tle,,146\n"
        );
        let rows = parse_moxfield(csv.as_bytes()).expect("parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Keeper");
        assert_eq!(rows[0].quantity, 2);
    }

    #[test]
    fn moxfield_without_a_foil_column_is_a_validation_error() {
        // Set + number + count but no Foil: refuse rather than silently import a foil
        // collection as regular copies.
        let csv = "Count,Name,Edition,Collector Number\n1,Sol Ring,c21,263\n";
        let err = parse_csv(csv.as_bytes()).expect_err("missing Foil must fail");
        assert!(matches!(err, ImportError::InvalidSource(_)));
    }

    #[test]
    fn an_archidekt_export_with_a_count_column_still_sniffs_as_archidekt() {
        // "Count" is both Moxfield's quantity header and an Archidekt quantity alias;
        // the Scryfall ID column must win the detection.
        let csv = "Count,Edition,Collector Number,Finish,Scryfall ID\n\
                   3,tle,146,Normal,f369827d-e4cd-4bc7-8c5e-72882eff0908\n";
        let holdings = parse_archidekt(csv.as_bytes()).expect("parse");
        assert_eq!(holdings[0].external_card_id, UID_A);
        assert_eq!(holdings[0].quantity, 3);
    }
}
