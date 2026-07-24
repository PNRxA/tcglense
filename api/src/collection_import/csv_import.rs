//! CSV collection parsers (Archidekt, Moxfield, and Mythic Tools exports).
//!
//! All three services offer a CSV "export collection", but their shapes differ:
//!
//! * **Archidekt** rows carry a **Scryfall ID** (our `cards.external_id`) plus a
//!   **Finish** and a **Quantity**; those three columns are all we read (extra columns
//!   are ignored, so a trimmed or full export both work).
//! * **Moxfield** rows carry **no card id at all** — a printing is identified by its
//!   **Edition** (Scryfall set code) + **Collector Number**, with the finish in a
//!   **Foil** column (blank / `foil` / `etched`) and the owned count in **Count**.
//!   Those rows resolve to catalog cards by `(set_code, collector_number)` (see
//!   [`super::execute_file_import`]).
//! * **Mythic Tools** (issue #572) counts copies in an **Amount** column — a spelling
//!   neither of the others uses, so it's the shape's fingerprint — alongside a
//!   **Scryfall ID**, a **Set Code** + **Collector Number**, and a **Finish**
//!   (`Nonfoil` / `foil` / `etched`). Rows with an id resolve like Archidekt's; rows
//!   without one fall back to `(set code, collector number)` like Moxfield's, because
//!   the app lets a row exist without an id.
//!
//! [`parse_csv`] sniffs which shape an upload is from its header row, in this order:
//! an **Amount** column means Mythic Tools; otherwise a Scryfall-id column means
//! Archidekt; otherwise Edition + Collector Number + Count means Moxfield. The order
//! matters both ways — Mythic Tools also has a Scryfall ID column, and Archidekt's
//! quantity column also accepts a "Count" spelling.
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
//! yields; the Moxfield output is a `Vec<PrintingRow>` that becomes `FetchedHolding`s
//! once its set/number pairs are resolved to external ids, and a Mythic Tools export can
//! yield both. Either way the provider-independent aggregate / resolve / reconcile /
//! apply path is reused as-is.

use super::archidekt::is_foil_finish;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS, Provider};

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

/// Mythic Tools' owned-count column. Neither Archidekt nor Moxfield spells its quantity
/// column this way, so its presence is what identifies a Mythic Tools export. Everything
/// else it needs (Scryfall ID / Set Code / Collector Number / Finish / Name) is already
/// covered by the header lists above.
const MYTHIC_AMOUNT_HEADERS: &[&str] = &["amount"];

/// A parsed collection upload, tagged with the provider whose export shape it matched.
///
/// The two row buckets are how each row identifies its printing, not two different
/// providers: `holdings` are rows that carried a card id (already the engine's normalized
/// shape) and `printings` are rows that must first be resolved from a
/// `(set code, collector number)` pair or a bare name. Archidekt fills only the first,
/// Moxfield only the second, and a Mythic Tools export can fill both.
#[derive(Debug)]
pub(super) struct ParsedCsv {
    pub(super) provider: Provider,
    pub(super) holdings: Vec<FetchedHolding>,
    pub(super) printings: Vec<PrintingRow>,
}

/// One usable row of a collection export that carries no card id, pre-normalized (set code
/// lowercased, number trimmed) but not yet resolved to a catalog card. Produced by the
/// Moxfield and Mythic Tools CSV parsers and by the plain-text list parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrintingRow {
    /// Scryfall set code, lowercased (the catalog stores lowercase codes). `None` when the
    /// source named no printing at all (a bare `3 Counterspell` text line), in which case
    /// the row resolves by name instead.
    pub(crate) set_code: Option<String>,
    /// Collector number as printed, trimmed. Compared exactly — Scryfall numbers can
    /// carry letters/symbols (`"12a"`, `"XLN-217"`) that must not be normalized away.
    pub(crate) collector_number: Option<String>,
    /// Card name. Labels unmatched cards in the import summary, and is the resolution key
    /// itself when the row names no printing.
    pub(crate) name: String,
    pub(crate) foil: bool,
    pub(crate) quantity: i32,
}

impl PrintingRow {
    /// The row's `(set code, collector number)` pair, when it named a printing.
    pub(crate) fn pair(&self) -> Option<(&str, &str)> {
        Some((self.set_code.as_deref()?, self.collector_number.as_deref()?))
    }
}

/// Parse an uploaded/pasted collection CSV, sniffing the provider from the header row.
///
/// `Ok(Some(parsed))` when the header matched a supported export shape (the rows may still
/// be empty if none was usable — the caller maps that to
/// [`ImportError::EmptyCollection`]). `Ok(None)` when the header matched **no** shape, so
/// the caller can try the plain-text list parser instead. Fails with
/// [`ImportError::InvalidSource`] (→ 422) when the bytes aren't readable as CSV at all, or
/// when a recognised shape is missing a column it needs.
pub(super) fn parse_csv(bytes: &[u8]) -> Result<Option<ParsedCsv>, ImportError> {
    // Strip a leading UTF-8 BOM if present, so the first header ("Quantity") isn't read
    // as "\u{feff}Quantity" and thus fails to match. Some spreadsheet exporters add one.
    let bytes = strip_bom(bytes);

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

    // An "Amount" column pins the shape to Mythic Tools — checked first because its export
    // also carries a Scryfall ID column, which would otherwise read as Archidekt.
    if let Some(amount_idx) = find_column(&headers, MYTHIC_AMOUNT_HEADERS) {
        return parse_mythic_tools_rows(reader, &headers, amount_idx).map(Some);
    }
    // An id column pins the shape to Archidekt — checked before Moxfield because
    // Archidekt's quantity column also matches Moxfield's "Count" spelling.
    if let Some(id_idx) = find_column(&headers, ID_HEADERS) {
        return parse_archidekt_rows(reader, &headers, id_idx).map(|holdings| {
            Some(ParsedCsv {
                provider: Provider::Archidekt,
                holdings,
                printings: Vec::new(),
            })
        });
    }
    // No id column: a Moxfield export identifies printings by set + collector number.
    if let (Some(count_idx), Some(edition_idx), Some(number_idx)) = (
        find_column(&headers, MOX_COUNT_HEADERS),
        find_column(&headers, MOX_EDITION_HEADERS),
        find_column(&headers, MOX_NUMBER_HEADERS),
    ) {
        return parse_moxfield_rows(reader, &headers, count_idx, edition_idx, number_idx).map(
            |printings| {
                Some(ParsedCsv {
                    provider: Provider::Moxfield,
                    holdings: Vec::new(),
                    printings,
                })
            },
        );
    }
    // Not a CSV shape we know. The caller decides what to do next (today: read it as a
    // plain-text card list), so this is deliberately not an error.
    Ok(None)
}

/// Strip a leading UTF-8 BOM, which some spreadsheet exporters prepend.
pub(crate) fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
}

/// Parse the data rows of an Archidekt-shaped CSV (a Scryfall id per row).
fn parse_archidekt_rows(
    mut reader: csv::Reader<&[u8]>,
    headers: &csv::StringRecord,
    id_idx: usize,
) -> Result<Vec<FetchedHolding>, ImportError> {
    let finish_idx =
        find_column(headers, FINISH_HEADERS).ok_or_else(|| missing_column("Finish"))?;
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

/// Parse the data rows of a Mythic Tools-shaped CSV.
///
/// Its export is a hybrid: every row has an **Amount**, and identifies its printing by a
/// **Scryfall ID** when the app has one, falling back to **Set Code** + **Collector
/// Number**. So a single export can yield both id-keyed holdings and pair-keyed rows, and
/// each row takes whichever key it actually carries. A row with neither is skipped.
///
/// Unlike the other two shapes nothing here is a required column beyond Amount: an export
/// with only ids and one with only set/number are both legitimate, and the "no usable
/// rows" case is already reported as an empty collection by the caller.
fn parse_mythic_tools_rows(
    mut reader: csv::Reader<&[u8]>,
    headers: &csv::StringRecord,
    amount_idx: usize,
) -> Result<ParsedCsv, ImportError> {
    let id_idx = find_column(headers, ID_HEADERS);
    let set_idx = find_column(headers, MOX_EDITION_HEADERS);
    let number_idx = find_column(headers, MOX_NUMBER_HEADERS);
    let finish_idx = find_column(headers, MOX_FOIL_HEADERS);
    let name_idx = find_column(headers, MOX_NAME_HEADERS);
    if id_idx.is_none() && (set_idx.is_none() || number_idx.is_none()) {
        return Err(ImportError::InvalidSource(
            "the Mythic Tools export needs either a \"Scryfall ID\" column or both a \
             \"Set Code\" and a \"Collector Number\" column — re-export with the default \
             columns selected"
                .to_string(),
        ));
    }

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut printings: Vec<PrintingRow> = Vec::new();
    let mut rows_seen = 0usize;
    for record in reader.records() {
        let record = read_record(record, &mut rows_seen)?;

        let Some(quantity) = positive_quantity(&record, amount_idx) else {
            continue;
        };
        let foil = is_foil_finish(false, finish_idx.and_then(|idx| record.get(idx)));

        // An id is the exact key, so prefer it and skip the pair lookup entirely.
        if let Some(external_card_id) =
            id_idx.and_then(|idx| bounded_cell(&record, idx, MAX_ID_LEN))
        {
            holdings.push(FetchedHolding {
                external_card_id,
                foil,
                quantity,
            });
            continue;
        }

        let set_code = set_idx
            .and_then(|idx| bounded_cell(&record, idx, MAX_SET_OR_NUMBER_LEN))
            .map(|set| set.to_ascii_lowercase());
        let collector_number =
            number_idx.and_then(|idx| bounded_cell(&record, idx, MAX_SET_OR_NUMBER_LEN));
        // Neither key: nothing to resolve against, so the row contributes nothing.
        if set_code.is_none() || collector_number.is_none() {
            continue;
        }
        printings.push(PrintingRow {
            set_code,
            collector_number,
            name: bounded_name(&record, name_idx),
            foil,
            quantity,
        });
    }

    Ok(ParsedCsv {
        provider: Provider::MythicTools,
        holdings,
        printings,
    })
}

/// Parse the data rows of a Moxfield-shaped CSV (set code + collector number per row).
fn parse_moxfield_rows(
    mut reader: csv::Reader<&[u8]>,
    headers: &csv::StringRecord,
    count_idx: usize,
    edition_idx: usize,
    number_idx: usize,
) -> Result<Vec<PrintingRow>, ImportError> {
    // The Foil column is required: silently importing a foil collection as regular
    // copies would corrupt counts, and Moxfield's export always includes it.
    let foil_idx = find_column(headers, MOX_FOIL_HEADERS).ok_or_else(|| missing_column("Foil"))?;
    // Name and Proxy are optional refinements — an export without them still imports.
    let name_idx = find_column(headers, MOX_NAME_HEADERS);
    let proxy_idx = find_column(headers, MOX_PROXY_HEADERS);

    let mut rows: Vec<PrintingRow> = Vec::new();
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

        let Some(set_code) = bounded_cell(&record, edition_idx, MAX_SET_OR_NUMBER_LEN) else {
            continue;
        };
        let Some(collector_number) = bounded_cell(&record, number_idx, MAX_SET_OR_NUMBER_LEN)
        else {
            continue;
        };
        let Some(quantity) = positive_quantity(&record, count_idx) else {
            continue;
        };
        // Blank = regular; any other value (`foil`, `etched`, …) is a foil finish in our
        // two-bucket model — the same rule as Archidekt's modifier (see `is_foil_finish`).
        let foil = is_foil_finish(false, record.get(foil_idx));

        rows.push(PrintingRow {
            set_code: Some(set_code.to_ascii_lowercase()),
            collector_number: Some(collector_number),
            name: bounded_name(&record, name_idx),
            foil,
            quantity,
        });
    }

    Ok(rows)
}

/// A record cell as an owned, trimmed `String` — or `None` when it's missing, blank, or
/// longer than `max` (a pathological cell is dropped before it reaches a catalog lookup).
fn bounded_cell(record: &csv::StringRecord, idx: usize, max: usize) -> Option<String> {
    match record.get(idx).map(str::trim) {
        Some(cell) if !cell.is_empty() && cell.len() <= max => Some(cell.to_string()),
        _ => None,
    }
}

/// The row's card name, truncated to [`MAX_NAME_LEN`]. Empty when the export has no name
/// column or the cell is blank — names only label unmatched cards, so a missing one is
/// never fatal.
fn bounded_name(record: &csv::StringRecord, name_idx: Option<usize>) -> String {
    name_idx
        .and_then(|idx| record.get(idx))
        .map(str::trim)
        .unwrap_or("")
        .chars()
        .take(MAX_NAME_LEN)
        .collect()
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

    /// Parse a fixture whose header must match a supported shape.
    fn parse_known(bytes: &[u8]) -> Result<ParsedCsv, ImportError> {
        Ok(parse_csv(bytes)?.expect("fixture header must match a supported export shape"))
    }

    /// Unwrap an Archidekt-detected parse (most fixtures here are Archidekt-shaped).
    fn parse_archidekt(bytes: &[u8]) -> Result<Vec<FetchedHolding>, ImportError> {
        let parsed = parse_known(bytes)?;
        assert_eq!(parsed.provider, Provider::Archidekt, "sniffed shape");
        assert!(parsed.printings.is_empty(), "Archidekt rows are id-keyed");
        Ok(parsed.holdings)
    }

    /// Unwrap a Moxfield-detected parse.
    fn parse_moxfield(bytes: &[u8]) -> Result<Vec<PrintingRow>, ImportError> {
        let parsed = parse_known(bytes)?;
        assert_eq!(parsed.provider, Provider::Moxfield, "sniffed shape");
        assert!(parsed.holdings.is_empty(), "Moxfield rows are pair-keyed");
        Ok(parsed.printings)
    }

    /// A pair-keyed row, for readable fixture assertions.
    fn printing(
        set_code: &str,
        collector_number: &str,
        name: &str,
        foil: bool,
        quantity: i32,
    ) -> PrintingRow {
        PrintingRow {
            set_code: Some(set_code.to_string()),
            collector_number: Some(collector_number.to_string()),
            name: name.to_string(),
            foil,
            quantity,
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
        assert_eq!(
            a.quantity, 2,
            "the embedded comma in the name didn't desync columns"
        );
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
        assert!(
            a.foil,
            "Etched (any non-Normal finish) is a foil in our model"
        );
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
    fn an_unrecognisable_header_is_reported_as_no_match_not_an_error() {
        // Neither an id column, nor an Amount column, nor Moxfield's set/number columns.
        // The caller falls back to the plain-text list parser, so this must not error.
        let csv = "Name,Description\nSol Ring,Fast mana\n";
        assert!(
            parse_csv(csv.as_bytes())
                .expect("no shape is not an error")
                .is_none(),
            "an unknown header leaves the decision to the caller"
        );
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
            printing("tle", "146", "Aang, A Lot to Learn", true, 1)
        );
        assert_eq!(
            rows[1].set_code.as_deref(),
            Some("tla"),
            "set codes are lowercased"
        );
        assert_eq!(rows[1].collector_number.as_deref(), Some("203"));
        assert!(!rows[1].foil, "a blank Foil cell is a regular card");
        assert_eq!(
            rows[1].quantity, 2,
            "Count (not Tradelist Count) is the owned count"
        );
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

    // ---- Mythic Tools exports (issue #572) ----

    /// The header row of a Mythic Tools collection export.
    const MYTHIC_HEADER: &str = "Amount,Name,Set Code,Set Name,Collector Number,Condition,\
                                 Finish,Language,Extra Info,Assigned Price,Notes,Scryfall ID";

    #[test]
    fn parses_a_real_shaped_mythic_tools_export() {
        let csv = format!(
            "{MYTHIC_HEADER}\n\
             2,\"Aang, Air Nomad\",tle,Avatar Eternal,146,NM,Nonfoil,en,,,,{UID_A}\n\
             1,Sol Ring,c21,Commander 2021,263,NM,foil,en,Signed,,,{UID_B}\n"
        );
        let parsed = parse_known(csv.as_bytes()).expect("parse");
        assert_eq!(
            parsed.provider,
            Provider::MythicTools,
            "the Amount column identifies the shape even though Scryfall ID is present too"
        );
        assert!(
            parsed.printings.is_empty(),
            "every row carried an id, so none needs a pair lookup"
        );
        assert_eq!(parsed.holdings.len(), 2);
        assert_eq!(parsed.holdings[0].external_card_id, UID_A);
        assert_eq!(parsed.holdings[0].quantity, 2);
        assert!(
            !parsed.holdings[0].foil,
            "\"Nonfoil\" is a regular card, not an unrecognised (and therefore foil) finish"
        );
        assert!(parsed.holdings[1].foil);
    }

    #[test]
    fn mythic_tools_rows_without_an_id_fall_back_to_set_and_number() {
        // The app allows a row with no Scryfall ID; it still names the printing.
        let csv = format!(
            "{MYTHIC_HEADER}\n\
             3,Counterspell,TLE,Avatar Eternal,146,NM,etched,en,,,,\n\
             1,Sol Ring,c21,Commander 2021,263,NM,Nonfoil,en,,,,{UID_B}\n"
        );
        let parsed = parse_known(csv.as_bytes()).expect("parse");
        assert_eq!(
            parsed.holdings.len(),
            1,
            "only the row that carried an id is id-keyed"
        );
        assert_eq!(
            parsed.printings,
            vec![printing("tle", "146", "Counterspell", true, 3)],
            "the id-less row keeps its (set, number) key, lowercased"
        );
    }

    #[test]
    fn mythic_tools_rows_with_neither_key_or_a_bad_amount_are_skipped() {
        let csv = format!(
            "{MYTHIC_HEADER}\n\
             1,No Keys,,Avatar Eternal,,NM,Nonfoil,en,,,,\n\
             0,Zero,tle,Avatar Eternal,146,NM,Nonfoil,en,,,,{UID_A}\n\
             x,Bad,tle,Avatar Eternal,147,NM,Nonfoil,en,,,,{UID_A}\n\
             4,Keeper,tle,Avatar Eternal,148,NM,Nonfoil,en,,,,{UID_A}\n"
        );
        let parsed = parse_known(csv.as_bytes()).expect("parse");
        assert!(parsed.printings.is_empty());
        assert_eq!(parsed.holdings.len(), 1);
        assert_eq!(parsed.holdings[0].quantity, 4);
    }

    #[test]
    fn a_mythic_tools_export_with_no_card_key_at_all_is_a_validation_error() {
        // Amount identified the shape, but nothing in it names a card — refuse with an
        // actionable message rather than importing an empty collection.
        let csv = "Amount,Name,Condition\n1,Sol Ring,NM\n";
        let err = parse_csv(csv.as_bytes()).expect_err("no card key must fail");
        let ImportError::InvalidSource(msg) = err else {
            panic!("expected InvalidSource");
        };
        assert!(msg.contains("Mythic Tools"), "names the format: {msg}");
    }
}
