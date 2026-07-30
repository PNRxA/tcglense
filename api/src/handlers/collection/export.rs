//! Collection exports: the whole collection as an Archidekt- or Moxfield-shaped CSV,
//! and the collection browse's card-search results as a plain-text card list.
//!
//! **CSV** ([`export_collection`]): the two shapes mirror the files those services
//! export, so an exported file re-imports
//! cleanly through [`crate::collection_import`] (a round trip): the Archidekt shape carries
//! the `Scryfall ID` column the importer keys off directly, and the Moxfield shape carries
//! the `Edition` (set code) + `Collector Number` pair it resolves by. See the import
//! sniffer (`collection_import::csv_import`) for the exact columns each parser reads.
//!
//! A holding only stores two counts (regular `quantity` + foil `foil_quantity`), so the
//! export emits one row per non-empty finish bucket and fills the provider columns we
//! can't know (condition, language, price, tags) with the neutral defaults a fresh export
//! from those services uses (`NM`/`Near Mint`, `EN`/`English`, blank). Card metadata comes
//! from the joined `cards` row; the few Archidekt columns we don't store (Multiverse Id,
//! MTGO ID) are emitted as `0`, matching Archidekt's own default for a card it can't map.
//!
//! **Text** ([`export_collection_cards`]): the collection browse's mirror of the public
//! catalog's card-search export, streamed through the shared engine
//! ([`crate::handlers::shared::card_export`]) over the very query the browse grid runs.

use axum::extract::State;
use axum::response::Response;
use csv::{QuoteStyle, Terminator, WriterBuilder};
use serde::Deserialize;

use crate::auth::extractor::AuthUser;
use crate::entities::{card, collection_item};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{
    csv_download, narrow_export_statement, render_holdings_export, require_game,
    resolve_holdings_list,
};
use crate::state::AppState;

use super::ListParams;
use super::read::{collection_query, owned_with_cards};

/// Query params for the export: which provider shape to produce.
#[derive(Debug, Deserialize)]
pub struct ExportParams {
    /// `archidekt` (default) or `moxfield`.
    pub format: Option<String>,
}

/// The provider shape an export produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Archidekt,
    Moxfield,
}

impl ExportFormat {
    /// Parse the `format` query value case-insensitively; absent/blank defaults to
    /// Archidekt. Anything else is a `422`.
    fn parse(value: Option<&str>) -> Result<Self, AppError> {
        match value.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("archidekt") => Ok(Self::Archidekt),
            Some("moxfield") => Ok(Self::Moxfield),
            Some(other) => Err(AppError::Validation(format!(
                "unknown export format '{other}' (expected 'archidekt' or 'moxfield')"
            ))),
        }
    }

    /// The provider id, used in the download filename.
    fn slug(self) -> &'static str {
        match self {
            Self::Archidekt => "archidekt",
            Self::Moxfield => "moxfield",
        }
    }

    /// The CSV header row for this shape.
    fn header(self) -> &'static [&'static str] {
        match self {
            Self::Archidekt => ARCHIDEKT_HEADER,
            Self::Moxfield => MOXFIELD_HEADER,
        }
    }

    /// How aggressively to quote fields — Archidekt quotes only when needed, Moxfield
    /// quotes every field, matching the genuine exports.
    fn quote_style(self) -> QuoteStyle {
        match self {
            Self::Archidekt => QuoteStyle::Necessary,
            Self::Moxfield => QuoteStyle::Always,
        }
    }
}

/// Which finish an emitted row is for. A holding with both regular and foil copies emits
/// one row of each.
#[derive(Debug, Clone, Copy)]
enum Finish {
    Regular,
    Foil,
}

const ARCHIDEKT_HEADER: &[&str] = &[
    "Quantity",
    "Name",
    "Finish",
    "Condition",
    "Date Added",
    "Language",
    "Purchase Price",
    "Tags",
    "Edition Name",
    "Edition Code",
    "Multiverse Id",
    "Scryfall ID",
    "MTGO ID",
    "Collector Number",
    "Mana Value",
    "Colors",
    "Identities",
    "Mana cost",
    "Types",
    "Sub-types",
    "Super-types",
    "Rarity",
    "Scryfall Oracle ID",
];

const MOXFIELD_HEADER: &[&str] = &[
    "Count",
    "Tradelist Count",
    "Name",
    "Edition",
    "Condition",
    "Language",
    "Foil",
    "Tags",
    "Last Modified",
    "Collector Number",
    "Alter",
    "Proxy",
    "Purchase Price",
];

/// Export collection (CSV)
///
/// `GET /api/collection/{game}/export?format=archidekt|moxfield` -> the signed-in user's
/// whole collection as a downloadable CSV in the chosen provider shape. Unpaginated (an
/// export is the entire collection); holdings whose catalog card row is gone (a catalog
/// re-import removed it) are skipped, as every other collection read does.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/export",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("format" = Option<String>, Query, description = "Provider shape: `archidekt` (default) or `moxfield`"),
    ),
    responses(
        (status = 200, description = "The whole collection as a downloadable CSV in the chosen provider shape.", content_type = "text/csv"),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown export format."),
    ),
)]
pub async fn export_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ExportParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let format = ExportFormat::parse(params.format.as_deref())?;

    let mut rows = owned_with_cards(user.id, &game, None)
        .all(&state.db)
        .await?;
    // Stable, alphabetical order (by card name, then set + collector number) so the file
    // reads well and re-exports deterministically — the genuine exports are name-sorted.
    rows.sort_by(|a, b| sort_key(a.1.as_ref()).cmp(&sort_key(b.1.as_ref())));

    let body = build_csv(format, &rows)?;
    let filename = format!("tcglense-{game}-collection-{}.csv", format.slug());
    csv_download(body, &filename)
}

/// Export collection card search results
///
/// `GET /api/collection/{game}/cards/export` -> the whole result set of the signed-in
/// user's owned-card search as a `.txt` download, honouring the same
/// `q`/`set`/`include_related`/`sort`/`dir` params as `/api/collection/{game}` — the
/// collection browse's mirror of the catalog's card-search export. Lines carry the real
/// owned counts, one line per non-empty finish (`4 Sol Ring (LTC) 284`, foil copies on a
/// second ` *F*`-tagged line), so the file round-trips through the text importer.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/cards/export",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the collection list"),
        ("set" = Option<String>, Query, description = "Optional set-code scope"),
        ("include_related" = Option<bool>, Query, description = "With `set`, span the set's whole group"),
        ("sort" = Option<String>, Query, description = "Sort key (`updated`/`quantity`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `N Name (SET) 123` per owned finish, foil tagged ` *F*`) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A streamed `text/plain` attachment listing every matching owned card — the whole result set, uncapped.", content_type = "text/plain"),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_collection_cards(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    let format = params.export_format()?;
    // The same resolution + query builder as `list_collection`, so the file is provably
    // the search the browse grid rendered, never a second implementation.
    let parts = resolve_holdings_list(&state, game_meta, &game, &params).await?;
    let query = collection_query(
        user.id,
        &game,
        parts.set_codes.as_deref(),
        parts.search,
        parts.sort,
        parts.dir,
        state.dialect(),
    );
    let statement = narrow_export_statement(
        query,
        collection_item::Column::CardId,
        collection_item::Column::Quantity,
        collection_item::Column::FoilQuantity,
    );
    // Scope-free filename (no set code): the holdings `?set=` is a filter param the
    // catalog-style canonicalisation doesn't apply to, and a filename must never carry
    // anything the visitor typed.
    render_holdings_export(
        &state,
        statement,
        format,
        &format!("tcglense-{game}-collection"),
    )
}

/// Order holdings by their card's name, then set code, then collector number (numeric run
/// first, then the raw string as a tiebreaker for suffixed numbers like `12a`). Holdings
/// with no card row sort last; they're skipped when writing anyway.
fn sort_key(card: Option<&card::Model>) -> (String, String, i32, String) {
    match card {
        Some(c) => (
            c.name.to_lowercase(),
            c.set_code.clone(),
            c.collector_number_int.unwrap_or(i32::MAX),
            c.collector_number.clone(),
        ),
        None => (String::new(), String::new(), i32::MAX, String::new()),
    }
}

/// Serialize the owned holdings to a CSV string in the given shape. One row per non-empty
/// finish bucket; a holding whose card row is missing is skipped.
fn build_csv(
    format: ExportFormat,
    rows: &[(collection_item::Model, Option<card::Model>)],
) -> Result<String, AppError> {
    // Genuine Archidekt/Moxfield exports use RFC 4180 CRLF terminators; match them so an
    // exported file is byte-shaped like the real thing (the csv writer defaults to LF).
    let mut writer = WriterBuilder::new()
        .quote_style(format.quote_style())
        .terminator(Terminator::CRLF)
        .from_writer(Vec::new());
    writer.write_record(format.header()).map_err(csv_err)?;

    for (item, card) in rows {
        let Some(card) = card else { continue };
        if item.quantity > 0 {
            write_row(
                &mut writer,
                format,
                item,
                card,
                Finish::Regular,
                item.quantity,
            )?;
        }
        if item.foil_quantity > 0 {
            write_row(
                &mut writer,
                format,
                item,
                card,
                Finish::Foil,
                item.foil_quantity,
            )?;
        }
    }

    let bytes = writer.into_inner().map_err(|e| {
        AppError::Internal(format!("failed to finalize export CSV: {}", e.into_error()))
    })?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::Internal("export CSV was not valid UTF-8".into()))
}

/// Write one holding-finish row in the given shape.
fn write_row(
    writer: &mut csv::Writer<Vec<u8>>,
    format: ExportFormat,
    item: &collection_item::Model,
    card: &card::Model,
    finish: Finish,
    count: i32,
) -> Result<(), AppError> {
    let record = match format {
        ExportFormat::Archidekt => archidekt_record(item, card, finish, count),
        ExportFormat::Moxfield => moxfield_record(item, card, finish, count),
    };
    writer.write_record(&record).map_err(csv_err)
}

/// Build the 23-column Archidekt row for one holding-finish.
fn archidekt_record(
    item: &collection_item::Model,
    card: &card::Model,
    finish: Finish,
    count: i32,
) -> Vec<String> {
    let (types, subtypes, supertypes) = split_type_line(card.type_line.as_deref());
    let finish = match finish {
        Finish::Regular => "Normal",
        Finish::Foil => "Foil",
    };
    vec![
        count.to_string(),
        card.name.clone(),
        finish.to_string(),
        "NM".to_string(),
        item.created_at.format("%Y-%m-%d").to_string(),
        "EN".to_string(),
        String::new(), // Purchase Price — not tracked
        String::new(), // Tags — not tracked
        card.set_name.clone(),
        card.set_code.clone(),
        "0".to_string(), // Multiverse Id — not stored
        card.external_id.clone(),
        "0".to_string(), // MTGO ID — not stored
        card.collector_number.clone(),
        format_mana_value(card.cmc),
        colors_to_names(card.colors.as_deref()),
        colors_to_names(card.color_identity.as_deref()),
        card.mana_cost.clone().unwrap_or_default(),
        types,
        subtypes,
        supertypes,
        card.rarity.clone().unwrap_or_default(),
        card.oracle_id.clone().unwrap_or_default(),
    ]
}

/// Build the 13-column Moxfield row for one holding-finish.
fn moxfield_record(
    item: &collection_item::Model,
    card: &card::Model,
    finish: Finish,
    count: i32,
) -> Vec<String> {
    let count = count.to_string();
    let foil = match finish {
        Finish::Regular => "",
        Finish::Foil => "foil",
    };
    vec![
        count.clone(),
        count, // Tradelist Count mirrors Count in a genuine "haves" export
        card.name.clone(),
        card.set_code.clone(),
        "Near Mint".to_string(),
        "English".to_string(),
        foil.to_string(),
        String::new(), // Tags — not tracked
        item.updated_at.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
        card.collector_number.clone(),
        "False".to_string(), // Alter — not tracked
        "False".to_string(), // Proxy — not tracked
        String::new(),       // Purchase Price — not tracked
    ]
}

/// Format a mana value (Scryfall `cmc`) the way Archidekt does: an integer when whole
/// (`5.0` -> `5`), otherwise the decimal (`0.5` -> `0.5`); blank when unknown.
fn format_mana_value(cmc: Option<f64>) -> String {
    match cmc {
        None => String::new(),
        Some(v) if v.fract() == 0.0 => format!("{}", v as i64),
        Some(v) => format!("{v}"),
    }
}

/// Map our comma-joined colour letters (`"W,U"`) to Archidekt's full colour names
/// (`"White,Blue"`). Unknown/empty entries are dropped; an empty/absent value yields `""`.
fn colors_to_names(colors: Option<&str>) -> String {
    let Some(colors) = colors else {
        return String::new();
    };
    colors
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(color_name)
        .collect::<Vec<_>>()
        .join(",")
}

/// A single Scryfall colour letter -> its full name. Unknown letters pass through as-is.
fn color_name(letter: &str) -> &str {
    match letter {
        "W" => "White",
        "U" => "Blue",
        "B" => "Black",
        "R" => "Red",
        "G" => "Green",
        other => other,
    }
}

/// MTG supertypes — the fixed leading words that Archidekt splits into its own column.
const SUPERTYPES: &[&str] = &[
    "Basic",
    "Legendary",
    "Ongoing",
    "Snow",
    "World",
    "Host",
    "Elite",
];

/// Split a Scryfall type line into Archidekt's `(Types, Sub-types, Super-types)` columns
/// (each comma-joined). The left of the em dash holds supertypes + card types; the right
/// holds subtypes. For a multi-faced card (`"A — B // C — D"`) only the front face is
/// used, matching Archidekt. A type line without an em dash has no subtypes.
fn split_type_line(type_line: Option<&str>) -> (String, String, String) {
    let Some(line) = type_line else {
        return (String::new(), String::new(), String::new());
    };
    let front = line.split("//").next().unwrap_or(line).trim();
    let (left, right) = match front.split_once('—') {
        Some((left, right)) => (left.trim(), right.trim()),
        None => (front, ""),
    };

    let mut supertypes = Vec::new();
    let mut types = Vec::new();
    for word in left.split_whitespace() {
        if SUPERTYPES.iter().any(|s| s.eq_ignore_ascii_case(word)) {
            supertypes.push(word);
        } else {
            types.push(word);
        }
    }
    let subtypes: Vec<&str> = right.split_whitespace().collect();

    (types.join(","), subtypes.join(","), supertypes.join(","))
}

/// A CSV writer error is always an internal fault here (we write to an in-memory buffer,
/// so there's no I/O to fail and every record matches the header width).
fn csv_err(error: csv::Error) -> AppError {
    AppError::Internal(format!("failed to build export CSV: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::collection_item;
    use sea_orm::prelude::DateTimeUtc;

    fn at(s: &str) -> DateTimeUtc {
        s.parse().unwrap()
    }

    /// A holding with the given counts and timestamps, over card id `card_id`.
    fn holding(card_id: i32, quantity: i32, foil_quantity: i32) -> collection_item::Model {
        collection_item::Model {
            id: card_id,
            user_id: 1,
            game: "mtg".into(),
            card_id,
            quantity,
            foil_quantity,
            created_at: at("2026-06-24T10:00:00Z"),
            updated_at: at("2026-07-01T21:57:39.367Z"),
        }
    }

    /// A card with the export-relevant fields set; the rest defaulted.
    fn card(id: i32, name: &str, type_line: &str) -> card::Model {
        card::Model {
            external_id: format!("sf-{id}"),
            oracle_id: Some(format!("or-{id}")),
            name: name.into(),
            set_code: "tla".into(),
            set_name: "Avatar: The Last Airbender".into(),
            collector_number: id.to_string(),
            collector_number_int: Some(id),
            rarity: Some("rare".into()),
            mana_cost: Some("{2}{W}".into()),
            cmc: Some(3.0),
            type_line: Some(type_line.into()),
            colors: Some("W".into()),
            color_identity: Some("W,G".into()),
            ..crate::test_support::card_model(id)
        }
    }

    #[test]
    fn export_format_parses_case_insensitively_and_defaults() {
        assert_eq!(ExportFormat::parse(None).unwrap(), ExportFormat::Archidekt);
        assert_eq!(
            ExportFormat::parse(Some("")).unwrap(),
            ExportFormat::Archidekt
        );
        assert_eq!(
            ExportFormat::parse(Some(" Archidekt ")).unwrap(),
            ExportFormat::Archidekt
        );
        assert_eq!(
            ExportFormat::parse(Some("MOXFIELD")).unwrap(),
            ExportFormat::Moxfield
        );
        assert!(matches!(
            ExportFormat::parse(Some("deckbox")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn mana_value_formats_whole_numbers_without_a_decimal() {
        assert_eq!(format_mana_value(None), "");
        assert_eq!(format_mana_value(Some(0.0)), "0");
        assert_eq!(format_mana_value(Some(5.0)), "5");
        assert_eq!(format_mana_value(Some(0.5)), "0.5");
    }

    #[test]
    fn colors_map_to_full_names() {
        assert_eq!(colors_to_names(None), "");
        assert_eq!(colors_to_names(Some("")), "");
        assert_eq!(colors_to_names(Some("W")), "White");
        assert_eq!(colors_to_names(Some("W,G")), "White,Green");
        assert_eq!(colors_to_names(Some("U,B,R")), "Blue,Black,Red");
    }

    #[test]
    fn type_line_splits_into_types_subtypes_supertypes() {
        assert_eq!(
            split_type_line(Some("Legendary Creature — Human Avatar Ally")),
            (
                "Creature".into(),
                "Human,Avatar,Ally".into(),
                "Legendary".into()
            )
        );
        assert_eq!(
            split_type_line(Some("Basic Land — Forest")),
            ("Land".into(), "Forest".into(), "Basic".into())
        );
        // No em dash -> no subtypes.
        assert_eq!(
            split_type_line(Some("Enchantment")),
            ("Enchantment".into(), String::new(), String::new())
        );
        // Multi-faced: only the front face is used.
        assert_eq!(
            split_type_line(Some("Creature — Human // Creature — Spirit")),
            ("Creature".into(), "Human".into(), String::new())
        );
        assert_eq!(
            split_type_line(None),
            (String::new(), String::new(), String::new())
        );
    }

    #[test]
    fn archidekt_csv_has_the_right_header_and_a_row_per_finish() {
        let rows = vec![(
            holding(1, 2, 1),
            Some(card(
                1,
                "Aang, Air Nomad",
                "Legendary Creature — Human Avatar",
            )),
        )];
        let csv = build_csv(ExportFormat::Archidekt, &rows).unwrap();
        let lines: Vec<&str> = csv.lines().collect();

        assert_eq!(lines[0], ARCHIDEKT_HEADER.join(","));
        // A regular row (2 copies) then a foil row (1 copy), both for the same card.
        assert_eq!(
            lines[1],
            "2,\"Aang, Air Nomad\",Normal,NM,2026-06-24,EN,,,Avatar: The Last Airbender,tla,0,sf-1,0,1,3,White,\"White,Green\",{2}{W},Creature,\"Human,Avatar\",Legendary,rare,or-1"
        );
        assert!(lines[2].starts_with("1,\"Aang, Air Nomad\",Foil,NM,"));
        assert_eq!(lines.len(), 3);
        // CRLF line terminators, matching a genuine Archidekt export.
        assert!(csv.contains("\r\n"));
    }

    #[test]
    fn moxfield_csv_quotes_every_field_and_mirrors_count_into_tradelist() {
        let rows = vec![(
            holding(5, 3, 0),
            Some(card(5, "Aang's Iceberg", "Enchantment")),
        )];
        let csv = build_csv(ExportFormat::Moxfield, &rows).unwrap();
        let lines: Vec<&str> = csv.lines().collect();

        assert_eq!(
            lines[0],
            "\"Count\",\"Tradelist Count\",\"Name\",\"Edition\",\"Condition\",\"Language\",\"Foil\",\"Tags\",\"Last Modified\",\"Collector Number\",\"Alter\",\"Proxy\",\"Purchase Price\""
        );
        assert_eq!(
            lines[1],
            "\"3\",\"3\",\"Aang's Iceberg\",\"tla\",\"Near Mint\",\"English\",\"\",\"\",\"2026-07-01 21:57:39.367000\",\"5\",\"False\",\"False\",\"\""
        );
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn zero_count_buckets_and_missing_cards_are_skipped() {
        let rows = vec![
            // Foil-only holding: no regular row.
            (holding(1, 0, 4), Some(card(1, "Foil Only", "Artifact"))),
            // Card row gone (catalog re-import): skipped entirely.
            (holding(2, 9, 0), None),
        ];
        let csv = build_csv(ExportFormat::Archidekt, &rows).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2, "header + one foil row only");
        assert!(lines[1].starts_with("4,Foil Only,Foil,"));
    }
}
