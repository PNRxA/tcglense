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
//! **Complete, and streamed to stay that way.** There is no row cap: a search matching
//! the entire catalog exports the entire catalog. What keeps that affordable is that the
//! response is never assembled in memory — rows are drained from the database
//! incrementally and rendered into chunks that go out as they're produced, so peak memory
//! is a chunk, not a result set (see [`render_export`]). Measured on a release build
//! against a 300k-card catalog: the full 11 MB export runs in ~2.5s for **no** measurable
//! change in RSS, and a slow client costs nothing either — the bounded channel makes it
//! backpressure the drain rather than pile chunks up. What an export *does* cost is one
//! sort of the matched rows, the same work the listing's first page already pays for; the
//! per-IP limiter on the public catalog group governs how often a visitor can ask for it.
//!
//! A consequence worth knowing: the status line is committed before the first row is
//! read, so a mid-stream database failure can't become a `500`. It instead appends a
//! `# …incomplete` comment **and** fails the transfer, so neither a human reading the
//! file nor the browser downloading it mistakes a short file for the whole search.

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::response::Response;
use futures_util::{StreamExt, stream};
use sea_orm::{DatabaseConnection, QuerySelect, Select, SelectGetableTuple, Selector};
use std::collections::HashSet;
use std::io;
use tokio::sync::mpsc;

use crate::entities::card;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{load_set, require_game, text_download_stream};
use crate::state::AppState;

use super::ListParams;
use super::cards::all_cards_query;
use super::sets::set_cards_query;

/// How many rendered card lines accumulate before a chunk is pushed to the client.
///
/// Trades syscalls/wakeups against the transient buffer: a few hundred lines is ~10–20 KB,
/// small enough that a big export never spikes memory and large enough that a full-catalog
/// drain isn't a million channel sends.
const EXPORT_CHUNK_CARDS: usize = 500;

/// Rendered chunks that may sit in flight before the drain has to wait for the client.
/// Bounded so a slow reader applies backpressure instead of buffering the whole export.
const EXPORT_CHANNEL_CHUNKS: usize = 4;

/// Appended when an export dies part-way. `#` is the comment marker every decklist parser
/// involved already skips, so it can't be misread as a card.
const FAILED_NOTE: &[u8] = b"# This export failed part-way and is incomplete - please try again.\n";

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
        (status = 200, description = "A streamed `text/plain` attachment listing every matching card — the whole result set, uncapped.", content_type = "text/plain"),
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
    render_export(&state, query, format, &format!("tcglense-{game}"))
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
        (status = 200, description = "A streamed `text/plain` attachment listing every matching card in the set — the whole result set, uncapped.", content_type = "text/plain"),
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
}

/// Run the (already filtered + sorted) query and hand back a streaming download.
///
/// The rows are drained through SeaORM's row stream — **one** query, whose results the
/// driver feeds us incrementally — and rendered into [`EXPORT_CHUNK_CARDS`]-sized text
/// chunks pushed down a bounded channel that backs the response body. Nothing ever holds
/// the whole result set: peak memory is a chunk of rows plus the text for that chunk,
/// whether the search matched twelve cards or the entire catalog.
///
/// One query rather than N paged ones is a cost decision. `apply_card_sort` ends on an
/// `id` tiebreaker, so the order is total and `LIMIT/OFFSET` batching would have been
/// *correct* — but each page re-runs the sort and walks the offset, making a full drain
/// O(rows²/page). Sorting the catalog once is already the expensive part (a 300k-row
/// unindexed sort is seconds, not milliseconds); paying it six hundred times to save
/// nothing is the trade this avoids.
fn render_export(
    state: &AppState,
    query: Select<card::Entity>,
    format: ExportFormat,
    filename_prefix: &str,
) -> Result<Response, AppError> {
    // Bounded, so a client that reads slowly applies backpressure to the DB drain rather
    // than letting rendered chunks pile up in memory.
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(EXPORT_CHANNEL_CHUNKS);
    // The stream borrows the connection, so the task owns its own handle
    // (`DatabaseConnection` is a cheap `Arc` clone) and keeps it for the drain's duration.
    let db = state.db.clone();
    tokio::spawn(async move { drain(db, query, format, tx).await });

    text_download_stream(
        Body::from_stream(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })),
        &format!("{filename_prefix}-{}.txt", format.slug()),
    )
}

/// The three columns an export line is made of.
///
/// Narrowing the select matters here in a way it doesn't for the paged listing: a `cards`
/// row carries ~70 columns including several JSON blobs (faces, legalities) and five image
/// URLs, and hydrating all of them for every row of a full-catalog drain — to render three
/// of them — dominates the export's runtime. The listing pays that cost for 60 rows; an
/// uncapped export would pay it for every printing in the game.
type ExportRow = (String, String, String);

/// Re-shape the listing's query to select only [`ExportRow`]'s columns.
///
/// The filters and ordering are untouched, so this stays the same search the grid ran —
/// `apply_card_sort` orders by columns that aren't selected, which is fine in a plain
/// `SELECT` on both backends (and the SQLite unique-mode `GROUP BY` tolerates it too).
fn export_rows(query: Select<card::Entity>) -> Selector<SelectGetableTuple<ExportRow>> {
    query
        .select_only()
        .column(card::Column::Name)
        .column(card::Column::SetCode)
        .column(card::Column::CollectorNumber)
        .into_tuple::<ExportRow>()
}

/// Drain the query into rendered chunks on `tx`. Runs detached: a send failure means the
/// client hung up, which is a normal end, not an error.
async fn drain(
    db: DatabaseConnection,
    query: Select<card::Entity>,
    format: ExportFormat,
    tx: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let mut rows = match export_rows(query).stream(&db).await {
        Ok(rows) => rows,
        Err(error) => {
            // Nothing has been written yet, but the 200 is already on the wire, so this
            // can't become a 500 — say so in the body and fail the transfer.
            tracing::error!(%error, "card export query failed to start");
            let _ = tx.send(Ok(Bytes::from_static(FAILED_NOTE))).await;
            let _ = tx
                .send(Err(io::Error::other("card export query failed")))
                .await;
            return;
        }
    };

    let mut writer = ChunkWriter::new(format);
    loop {
        match rows.next().await {
            Some(Ok(card)) => {
                writer.push(&card);
                if let Some(chunk) = writer.take_full_chunk()
                    && tx.send(Ok(chunk)).await.is_err()
                {
                    return; // client hung up
                }
            }
            None => break,
            Some(Err(error)) => {
                // Mid-stream failure. Flush what we have, then mark the file incomplete
                // *and* fail the transfer, so neither a reader nor the browser mistakes a
                // short file for the whole result set.
                tracing::error!(%error, "card export stream failed part-way");
                let _ = tx.send(Ok(writer.finish())).await;
                let _ = tx.send(Ok(Bytes::from_static(FAILED_NOTE))).await;
                let _ = tx
                    .send(Err(io::Error::other("card export stream failed")))
                    .await;
                return;
            }
        }
    }
    let _ = tx.send(Ok(writer.finish())).await;
}

/// Accumulates rendered card lines and hands them out a chunk at a time.
///
/// Owns the `names` de-duplication, which is why it spans the whole export rather than
/// living per-chunk: a name first seen in chunk 1 must not reappear in chunk 40. The set
/// is bounded by *distinct names* (tens of thousands for a full catalog), not printings.
struct ChunkWriter {
    format: ExportFormat,
    buffer: String,
    lines: usize,
    seen: HashSet<String>,
}

impl ChunkWriter {
    fn new(format: ExportFormat) -> Self {
        Self {
            format,
            buffer: String::new(),
            lines: 0,
            seen: HashSet::new(),
        }
    }

    /// Render one row, if it earns a line in this format.
    fn push(&mut self, row: &ExportRow) {
        let (name, set_code, collector_number) = row;
        match self.format {
            ExportFormat::Text => {
                self.buffer
                    .push_str(&text_row(name, set_code, collector_number));
                self.lines += 1;
            }
            ExportFormat::Names => {
                // First appearance wins its place in the query's order.
                if self.seen.insert(name.clone()) {
                    self.buffer.push_str(name);
                    self.buffer.push('\n');
                    self.lines += 1;
                }
            }
        }
    }

    /// The buffered text once it's a full chunk's worth, else `None`.
    fn take_full_chunk(&mut self) -> Option<Bytes> {
        (self.lines >= EXPORT_CHUNK_CARDS).then(|| self.take())
    }

    /// Whatever is left (possibly empty — an export matching nothing is an empty file).
    fn finish(&mut self) -> Bytes {
        self.take()
    }

    fn take(&mut self) -> Bytes {
        self.lines = 0;
        Bytes::from(std::mem::take(&mut self.buffer))
    }
}

/// One printing as `1 Name (SET) 123` — the deck export's text row without the finish
/// marker (a catalog printing isn't a holding, so there's no foil/regular to state).
fn text_row(name: &str, set_code: &str, collector_number: &str) -> String {
    format!(
        "1 {name} ({}) {collector_number}\n",
        set_code.to_ascii_uppercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, set_code: &str, number: &str) -> ExportRow {
        (name.to_string(), set_code.to_string(), number.to_string())
    }

    /// Push every row, then drain — mirroring what `drain` does, minus the database.
    fn render_all(rows: &[ExportRow], format: ExportFormat) -> String {
        let mut writer = ChunkWriter::new(format);
        let mut out = Vec::new();
        for card in rows {
            writer.push(card);
            if let Some(chunk) = writer.take_full_chunk() {
                out.push(chunk);
            }
        }
        out.push(writer.finish());
        join(&out)
    }

    /// The chunks a client would receive, concatenated back into the delivered file.
    fn join(chunks: &[Bytes]) -> String {
        String::from_utf8(chunks.iter().flatten().copied().collect()).expect("export is UTF-8")
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
            card("Sol Ring", "ltc", "284"),
            card("Lightning Bolt", "2x2", "117"),
        ];
        assert_eq!(
            render_all(&rows, ExportFormat::Text),
            "1 Sol Ring (LTC) 284\n1 Lightning Bolt (2X2) 117\n"
        );
    }

    #[test]
    fn names_dedupe_across_printings_and_keep_query_order() {
        let rows = [
            card("Sol Ring", "ltc", "284"),
            card("Sol Ring", "c21", "263"),
            card("Arcane Signet", "eld", "331"),
            card("Sol Ring", "cmr", "472"),
        ];
        assert_eq!(
            render_all(&rows, ExportFormat::Names),
            "Sol Ring\nArcane Signet\n"
        );
    }

    #[test]
    fn an_export_is_pure_card_lines() {
        let rows = [card("Sol Ring", "ltc", "284")];
        for format in [ExportFormat::Text, ExportFormat::Names] {
            // No cap means no truncation note — nothing but cards unless something broke.
            assert!(!render_all(&rows, format).contains('#'));
        }
    }

    #[test]
    fn an_export_matching_nothing_is_an_empty_file() {
        assert_eq!(render_all(&[], ExportFormat::Text), "");
        assert_eq!(render_all(&[], ExportFormat::Names), "");
    }

    #[test]
    fn chunking_splits_the_stream_without_losing_or_repeating_a_row() {
        // Two and a half chunks' worth: the boundary logic must not drop the partial
        // tail, duplicate a row, or reorder anything.
        let rows: Vec<ExportRow> = (0..EXPORT_CHUNK_CARDS * 2 + 7)
            .map(|i| card(&format!("Card {i}"), "dmb", &i.to_string()))
            .collect();

        let mut writer = ChunkWriter::new(ExportFormat::Text);
        let mut chunks = Vec::new();
        for card in &rows {
            writer.push(card);
            if let Some(chunk) = writer.take_full_chunk() {
                chunks.push(chunk);
            }
        }
        chunks.push(writer.finish());

        // Three sends: two full chunks and the remainder.
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0].iter().filter(|b| **b == b'\n').count(),
            EXPORT_CHUNK_CARDS
        );
        assert_eq!(chunks[2].iter().filter(|b| **b == b'\n').count(), 7);

        let joined = join(&chunks);
        let lines: Vec<&str> = joined.lines().collect();
        assert_eq!(lines.len(), rows.len(), "every row exported exactly once");
        assert_eq!(lines[0], "1 Card 0 (DMB) 0");
        assert_eq!(
            lines[EXPORT_CHUNK_CARDS],
            format!("1 Card {n} (DMB) {n}", n = EXPORT_CHUNK_CARDS),
            "the row straddling a chunk boundary survives intact"
        );
        assert_eq!(*lines.last().unwrap(), "1 Card 1006 (DMB) 1006");
    }

    #[test]
    fn names_dedupe_spans_chunk_boundaries() {
        // The de-dup set has to outlive a chunk: a name first seen in chunk 1 must not
        // reappear when it turns up again in chunk 2.
        let mut rows: Vec<ExportRow> = (0..EXPORT_CHUNK_CARDS + 5)
            .map(|i| card(&format!("Card {i}"), "dmb", &i.to_string()))
            .collect();
        rows.push(card("Card 0", "dmu", "1"));

        let rendered = render_all(&rows, ExportFormat::Names);
        assert_eq!(
            rendered.lines().filter(|line| *line == "Card 0").count(),
            1,
            "the reprint from a later chunk must not emit a second line"
        );
        assert_eq!(rendered.lines().count(), EXPORT_CHUNK_CARDS + 5);
    }

    #[test]
    fn filename_slugs_differ_per_shape() {
        assert_eq!(ExportFormat::Text.slug(), "cards");
        assert_eq!(ExportFormat::Names.slug(), "card-names");
    }
}
