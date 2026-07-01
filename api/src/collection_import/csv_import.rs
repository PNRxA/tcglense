//! Archidekt CSV collection parser.
//!
//! Archidekt's "Export collection" produces a CSV. Its full export carries ~two dozen
//! columns, but reconstructing a holding needs only three: the **Scryfall ID** (our
//! `cards.external_id`), the **Finish** (regular vs foil), and the **Quantity**. We read
//! just those three by header name and ignore everything else, so an export that keeps
//! extra columns — or drops the ones we don't use — still imports as long as the three
//! required headers are present. (The UI tells the user to export only those three.)
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
//! * Each Scryfall id is length-bounded and non-blank before it's used; a malformed or
//!   pathological id simply doesn't resolve against the catalog (the id is only ever a
//!   bound SQL parameter downstream, never interpolated), so an unknown id is skipped,
//!   never trusted.
//! * Quantities are parsed as integers and non-positive / unparseable rows are dropped;
//!   the reconcile engine additionally clamps the aggregated totals.
//!
//! The output is the same normalized `Vec<FetchedHolding>` a network provider yields, so
//! the provider-independent aggregate / resolve / reconcile / apply path is reused as-is.

use super::archidekt::is_foil_finish;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS};

/// Longest Scryfall id we'll accept from a CSV cell. A Scryfall id is a 36-char UUID;
/// this leaves generous headroom while rejecting a pathologically long cell before it
/// reaches the (chunked, parameterised) catalog lookup.
const MAX_ID_LEN: usize = 64;

/// Header spellings (compared case- and whitespace-insensitively) that name the Scryfall
/// id column. Archidekt writes "Scryfall ID"; the aliases tolerate minor variations.
const ID_HEADERS: &[&str] = &["scryfall id", "scryfall_id", "scryfallid"];
/// Header spellings for the finish column ("Finish").
const FINISH_HEADERS: &[&str] = &["finish"];
/// Header spellings for the quantity column ("Quantity").
const QUANTITY_HEADERS: &[&str] = &["quantity", "qty", "count"];

/// Parse an uploaded Archidekt CSV export into normalized holdings.
///
/// Returns the holdings (possibly empty if no row carried a usable id + quantity — the
/// caller maps an empty result to [`ImportError::EmptyCollection`]). Fails with
/// [`ImportError::InvalidSource`] (→ 422) when the file isn't a CSV we can read or is
/// missing one of the three required columns.
pub fn parse_archidekt_csv(bytes: &[u8]) -> Result<Vec<FetchedHolding>, ImportError> {
    // Strip a leading UTF-8 BOM if present, so the first header ("Quantity") isn't read
    // as "\u{feff}Quantity" and thus fails to match. Some spreadsheet exporters add one.
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        // Tolerate ragged rows (a row with more/fewer fields than the header) rather than
        // erroring — we address the columns we need by index and ignore the rest.
        .flexible(true)
        .from_reader(bytes);

    // Locate the three columns we consume by header name (case/space-insensitive).
    let headers = reader
        .headers()
        .map_err(|_| ImportError::InvalidSource("the uploaded file isn't a readable CSV".into()))?;
    let id_idx = find_column(headers, ID_HEADERS)
        .ok_or_else(|| missing_column("Scryfall ID"))?;
    let finish_idx = find_column(headers, FINISH_HEADERS)
        .ok_or_else(|| missing_column("Finish"))?;
    let quantity_idx = find_column(headers, QUANTITY_HEADERS)
        .ok_or_else(|| missing_column("Quantity"))?;

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut rows_seen = 0usize;
    for record in reader.records() {
        // A read error here means the file isn't valid UTF-8 CSV (e.g. a binary upload).
        // Fail clearly instead of importing a partial, silently-truncated collection.
        let record = record
            .map_err(|_| ImportError::InvalidSource("the CSV could not be parsed".into()))?;

        rows_seen += 1;
        if rows_seen > MAX_IMPORT_ROWS {
            return Err(ImportError::TooLarge {
                count: rows_seen,
                max: MAX_IMPORT_ROWS,
            });
        }

        // A row missing any of our columns (short row) just contributes nothing.
        let external_card_id = match record.get(id_idx).map(str::trim) {
            Some(id) if !id.is_empty() && id.len() <= MAX_ID_LEN => id.to_string(),
            _ => continue,
        };
        let quantity = match record.get(quantity_idx).and_then(parse_quantity) {
            Some(q) if q > 0 => q,
            _ => continue,
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

/// The 0-based index of the first header matching any of `names` (compared with
/// [`normalize`]), or `None` if the CSV has no such column.
fn find_column(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
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
fn parse_quantity(cell: &str) -> Option<i32> {
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

/// A 422 for a CSV that's missing one of the three columns we need.
fn missing_column(name: &str) -> ImportError {
    ImportError::InvalidSource(format!(
        "the CSV is missing a required \"{name}\" column — export your Archidekt collection \
         with the Scryfall ID, Finish, and Quantity columns"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A UUID-shaped Scryfall id for readable fixtures.
    const UID_A: &str = "f369827d-e4cd-4bc7-8c5e-72882eff0908";
    const UID_B: &str = "50a22ad6-d2a4-48a6-91c9-147c946a60a5";

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
        let holdings = parse_archidekt_csv(csv.as_bytes()).expect("parse");
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
        let holdings = parse_archidekt_csv(csv.as_bytes()).expect("parse");
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
        let holdings = parse_archidekt_csv(&bytes).expect("parse with BOM");
        assert_eq!(holdings.len(), 1, "the BOM didn't break header matching");
        assert_eq!(holdings[0].quantity, 1);
    }

    #[test]
    fn missing_a_required_column_is_a_validation_error() {
        // No Finish column.
        let csv = "Quantity,Scryfall ID\n1,f369827d-e4cd-4bc7-8c5e-72882eff0908\n";
        let err = parse_archidekt_csv(csv.as_bytes()).expect_err("missing Finish must fail");
        assert!(matches!(err, ImportError::InvalidSource(_)));
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
        let holdings = parse_archidekt_csv(csv.as_bytes()).expect("parse");
        // Only the last row (a valid id + positive quantity) survives.
        assert_eq!(holdings.len(), 1);
        assert_eq!(holdings[0].external_card_id, UID_A);
        assert_eq!(holdings[0].quantity, 2);
    }

    #[test]
    fn an_empty_or_header_only_csv_yields_no_holdings() {
        // Header only.
        let holdings = parse_archidekt_csv(b"Scryfall ID,Finish,Quantity\n").expect("parse");
        assert!(holdings.is_empty());
    }

    #[test]
    fn a_non_utf8_body_is_rejected_not_partially_imported() {
        // A valid header row, then a byte sequence that isn't valid UTF-8 in a data row.
        let mut bytes = b"Scryfall ID,Finish,Quantity\n".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe, b',', b'N', b',', b'1', b'\n']);
        let err = parse_archidekt_csv(&bytes).expect_err("invalid UTF-8 must fail");
        assert!(matches!(err, ImportError::InvalidSource(_)));
    }

    #[test]
    fn a_huge_quantity_saturates_rather_than_overflowing() {
        let csv = "Scryfall ID,Finish,Quantity\n\
                   f369827d-e4cd-4bc7-8c5e-72882eff0908,Normal,999999999999\n";
        let holdings = parse_archidekt_csv(csv.as_bytes()).expect("parse");
        assert_eq!(holdings[0].quantity, i32::MAX, "saturates, never wraps");
    }
}
