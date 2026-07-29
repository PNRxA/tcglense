//! Plain-text export of a card search's results.
//!
//! The catalog browse grids page 60 cards at a time, so "the results of my search" is
//! something a visitor can see but can't easily *take with them* — copying 40 pages of
//! tiles by hand is not a workflow. These two endpoints hand back the whole result set
//! as one `.txt` download, using the exact same filtered/sorted query the grid renders
//! (`super::cards::all_cards_query` / `super::sets::set_cards_query`), so the file is
//! provably the same search rather than a second implementation that can drift.
//!
//! Two shapes, both deliberately the dumbest thing that works:
//!
//! * `text` (default) — `1 Name (SET) 123`, one line per printing. That's the same
//!   line grammar the deck export's Moxfield text format emits and the one
//!   [`crate::deck_import::parser`] / [`crate::collection_import::text_list`] read back,
//!   so an exported search pastes straight into this app's own importers and into every
//!   deck site that accepts a text list. The leading `1` is a quantity the format
//!   requires, not a claim about how many the visitor owns.
//! * `names` — bare card names, de-duplicated across printings. A search that matches
//!   nine printings of `Sol Ring` is, to a human building a list, one card; this shape
//!   is for pasting into a spreadsheet or another search box.
//!
//! **Bounded, and honest about it.** A bare `q`-less export would be the entire
//! catalog (hundreds of thousands of rows), so the response is capped at
//! [`MAX_EXPORT_CARDS`]. When the search matches more than that, the file ends with a
//! `#` comment saying exactly how many matches were left out — a truncated export that
//! *looks* complete would be worse than no export at all. `#` is the comment marker
//! every decklist parser in this repo (and Archidekt/Moxfield) already skips, so the
//! note never corrupts a paste.

use axum::extract::State;
use axum::response::Response;
use sea_orm::{PaginatorTrait, Select};
use std::collections::HashSet;

use crate::entities::card;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{load_set, require_game, text_download};
use crate::state::AppState;

use super::ListParams;
use super::cards::all_cards_query;
use super::sets::set_cards_query;

/// Most cards a single export may contain.
///
/// Sized to cover any realistic search (the largest MTG set is well under 1,000 cards,
/// and a broad filter like `t:creature c:r` lands in the low thousands) while keeping
/// the response a few hundred KB rather than the whole catalog. Beyond this the export
/// truncates and says so.
///
/// The SPA mirrors this in `web/src/lib/api/catalog.ts` (`MAX_EXPORT_CARDS`) to state the
/// cap in the export menu — change both. Drift there is cosmetic, not dangerous: this
/// value is the only enforcement, and the body names the omitted count regardless.
const MAX_EXPORT_CARDS: u64 = 10_000;

/// The plain-text shape an export produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportFormat {
    /// `1 Name (SET) 123` per printing — importable by this app and by deck sites.
    Text,
    /// Bare card names, de-duplicated across printings.
    Names,
}

impl ExportFormat {
    /// Parse the `?format=` param. Absent/blank is [`ExportFormat::Text`]; anything
    /// unrecognised is a 422 (consistent with a malformed `q` or `sort`, and with the
    /// collection/deck exports) rather than a silent fall back to the default.
    pub(super) fn parse(value: Option<&str>) -> Result<Self, AppError> {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("") | Some("text") => Ok(Self::Text),
            Some("names") => Ok(Self::Names),
            Some(other) => Err(AppError::Validation(format!(
                "unknown card export format '{other}' (expected 'text' or 'names')"
            ))),
        }
    }

    /// The filename slug for this shape.
    fn slug(self) -> &'static str {
        match self {
            Self::Text => "cards",
            Self::Names => "card-names",
        }
    }
}

/// Export card search results
///
/// `GET /api/games/{game}/cards/export` -> the whole result set of the all-cards search
/// as a `.txt` download, honouring the same `q`/`name`/`sort`/`dir` params as
/// `/api/games/{game}/cards`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/export",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the card list"),
        ("name" = Option<String>, Query, description = "Optional exact-name filter (matched literally)"),
        ("sort" = Option<String>, Query, description = "Sort key (`name`/`number`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `1 Name (SET) 123` per printing) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A `text/plain` attachment listing every matching card, capped at 10,000 rows (a `#` comment states how many matches were omitted).", content_type = "text/plain"),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_cards(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    let format = params.export_format()?;
    let query = all_cards_query(&game, game_meta, &params, state.dialect())?;
    render_export(&state, query, format, &format!("tcglense-{game}")).await
}

/// Export set card search results
///
/// `GET /api/games/{game}/sets/{code}/cards/export` -> the whole result set of a set's
/// card search as a `.txt` download, honouring the same `q`/`include_related`/`sort`/`dir`
/// params as `/api/games/{game}/sets/{code}/cards`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/sets/{code}/cards/export",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("code" = String, Path, description = "Set code, e.g. `neo`"),
        ("q" = Option<String>, Query, description = "Optional Scryfall-style search filter — the same grammar as the set card list"),
        ("include_related" = Option<bool>, Query, description = "Span the set's whole group (root + related sub-sets)"),
        ("sort" = Option<String>, Query, description = "Sort key (`number`/`name`/`rarity`/`released`/`cmc`/`price`)"),
        ("dir" = Option<String>, Query, description = "Sort direction (`asc`/`desc`)"),
        ("format" = Option<String>, Query, description = "`text` (default, `1 Name (SET) 123` per printing) or `names` (de-duplicated card names)"),
    ),
    responses(
        (status = 200, description = "A `text/plain` attachment listing every matching card in the set, capped at 10,000 rows (a `#` comment states how many matches were omitted).", content_type = "text/plain"),
        (status = 404, description = "Unknown game or set."),
        (status = 422, description = "Malformed search query, sort, or export format."),
    ),
)]
pub async fn export_set_cards(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    Query(params): Query<ListParams>,
) -> Result<Response, AppError> {
    let game_meta = require_game(&game)?;
    // Resolve the set before the format, so an unknown set is a 404 rather than a 422
    // about a typo'd `?format=` — the same ordering the deck export uses.
    let set = load_set(&state, &game, &code).await?;
    let format = params.export_format()?;
    let query = set_cards_query(&state, &game, game_meta, &set, &params).await?;
    // `set.code` is the catalog's own value (the path segment may differ in case), so
    // the filename can't carry anything the visitor typed.
    render_export(
        &state,
        query,
        format,
        &format!("tcglense-{game}-{}", set.code),
    )
    .await
}

/// Run the (already filtered + sorted) query, render it, and wrap it as a download.
///
/// Uses the paginator rather than a bare `limit` so the total is the *unclamped* match
/// count — that's what lets the truncation note say how many rows were left out.
async fn render_export(
    state: &AppState,
    query: Select<card::Entity>,
    format: ExportFormat,
    filename_prefix: &str,
) -> Result<Response, AppError> {
    let paginator = query.paginate(&state.db, MAX_EXPORT_CARDS);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(0).await?;

    let body = render(&rows, total, format);
    text_download(body, &format!("{filename_prefix}-{}.txt", format.slug()))
}

/// Render the fetched rows (plus the unclamped `total`) as the export body.
fn render(rows: &[card::Model], total: u64, format: ExportFormat) -> String {
    let mut output = String::new();
    match format {
        ExportFormat::Text => {
            for card in rows {
                output.push_str(&text_row(card));
            }
        }
        ExportFormat::Names => {
            // De-duplicate across printings while keeping the query's order: the first
            // time a name appears is where it belongs in the sorted list.
            let mut seen: HashSet<&str> = HashSet::with_capacity(rows.len());
            for card in rows {
                if seen.insert(card.name.as_str()) {
                    output.push_str(&card.name);
                    output.push('\n');
                }
            }
        }
    }
    if let Some(note) = truncation_note(rows.len(), total) {
        output.push_str(&note);
    }
    output
}

/// One printing as `1 Name (SET) 123` — the deck export's text row without the finish
/// marker (a catalog printing isn't a holding, so there's no foil/regular to state).
fn text_row(card: &card::Model) -> String {
    format!(
        "1 {} ({}) {}\n",
        card.name,
        card.set_code.to_ascii_uppercase(),
        card.collector_number
    )
}

/// The trailing `# …` note when the cap cut the export short, or `None` when the file
/// is the complete result set. Never silently truncate: a file that looks like the whole
/// search but isn't is worse than one that says so.
fn truncation_note(exported: usize, total: u64) -> Option<String> {
    let omitted = total.saturating_sub(exported as u64);
    (omitted > 0).then(|| {
        format!(
            "# {omitted} more matching cards were not exported (the export is capped at \
             {MAX_EXPORT_CARDS} cards — narrow your search to include them).\n"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::card_model;

    fn card(id: i32, name: &str, set_code: &str, number: &str) -> card::Model {
        card::Model {
            name: name.to_string(),
            set_code: set_code.to_string(),
            collector_number: number.to_string(),
            ..card_model(id)
        }
    }

    #[test]
    fn format_parses_case_insensitively_and_defaults_to_text() {
        assert_eq!(ExportFormat::parse(None).unwrap(), ExportFormat::Text);
        assert_eq!(ExportFormat::parse(Some("")).unwrap(), ExportFormat::Text);
        assert_eq!(
            ExportFormat::parse(Some("  TEXT ")).unwrap(),
            ExportFormat::Text
        );
        assert_eq!(
            ExportFormat::parse(Some("Names")).unwrap(),
            ExportFormat::Names
        );
        // An unrecognised shape is a 422, not a silent default.
        assert!(matches!(
            ExportFormat::parse(Some("csv")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn text_rows_are_quantity_name_set_number() {
        let rows = [
            card(1, "Sol Ring", "ltc", "284"),
            card(2, "Lightning Bolt", "2x2", "117"),
        ];
        assert_eq!(
            render(&rows, 2, ExportFormat::Text),
            "1 Sol Ring (LTC) 284\n1 Lightning Bolt (2X2) 117\n"
        );
    }

    #[test]
    fn names_dedupe_across_printings_and_keep_query_order() {
        let rows = [
            card(1, "Sol Ring", "ltc", "284"),
            card(2, "Sol Ring", "c21", "263"),
            card(3, "Arcane Signet", "eld", "331"),
            card(4, "Sol Ring", "cmr", "472"),
        ];
        assert_eq!(
            render(&rows, 4, ExportFormat::Names),
            "Sol Ring\nArcane Signet\n"
        );
    }

    #[test]
    fn a_complete_export_carries_no_comment() {
        let rows = [card(1, "Sol Ring", "ltc", "284")];
        for format in [ExportFormat::Text, ExportFormat::Names] {
            assert!(
                !render(&rows, 1, format).contains('#'),
                "an untruncated export should be pure card lines"
            );
        }
    }

    #[test]
    fn a_truncated_export_states_how_many_were_omitted() {
        let rows = [card(1, "Sol Ring", "ltc", "284")];
        let body = render(&rows, 1_234, ExportFormat::Text);
        assert!(body.starts_with("1 Sol Ring (LTC) 284\n"));
        assert!(
            body.contains("# 1233 more matching cards were not exported"),
            "got: {body}"
        );
        // The note is a comment line, so it never re-imports as a card.
        assert!(
            body.trim_end()
                .lines()
                .next_back()
                .unwrap()
                .starts_with('#')
        );
    }

    #[test]
    fn the_truncation_note_counts_matches_not_rendered_lines() {
        // `names` folds printings together, so the note must still be derived from the
        // match total vs. the rows *fetched* — not the (smaller) de-duplicated line count.
        let rows = [
            card(1, "Sol Ring", "ltc", "284"),
            card(2, "Sol Ring", "c21", "263"),
        ];
        assert_eq!(
            render(&rows, 5, ExportFormat::Names),
            "Sol Ring\n# 3 more matching cards were not exported (the export is capped at \
             10000 cards — narrow your search to include them).\n"
        );
    }

    #[test]
    fn a_total_below_the_row_count_cannot_underflow() {
        // `num_items` and `fetch_page` are two queries; a concurrent delete between them
        // could make the total the smaller of the two. Saturating means no phantom note.
        assert_eq!(truncation_note(5, 2), None);
    }

    #[test]
    fn filename_slugs_differ_per_shape() {
        assert_eq!(ExportFormat::Text.slug(), "cards");
        assert_eq!(ExportFormat::Names.slug(), "card-names");
    }
}
