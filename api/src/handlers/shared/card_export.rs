//! The shared streaming engine behind every plain-text card export: the public
//! catalog's card-search exports (`handlers::catalog::export`) and the collection /
//! wish-list browse exports (`handlers::collection::export` /
//! `handlers::wishlist::export`). Each endpoint keeps building its query from its
//! listing's own builder and hands it here, so the file is provably the same search
//! the grid renders rather than a second implementation that can drift.
//!
//! Two shapes, both deliberately the dumbest thing that works:
//!
//! * `text` (default) — `N Name (SET) 123`, one line per printing (holdings add a
//!   second ` *F*`-tagged line for foil copies). That's the same line grammar the deck
//!   export's Moxfield text format emits and the one [`crate::deck_import::parser`] /
//!   [`crate::collection_import::text_list`] read back, so an exported search pastes
//!   straight into this app's own importers and into every deck site that accepts a
//!   text list. A catalog search pins every line to `1` — a quantity the format
//!   requires, not a claim about how many the visitor owns — while a holdings export
//!   carries the real held counts, so a collection round-trips through the text
//!   importer with quantities and finishes intact.
//! * `names` — bare card names, de-duplicated across printings. A search that matches
//!   nine printings of `Sol Ring` is, to a human building a list, one card; this shape
//!   is for pasting into a spreadsheet or another search box.
//!
//! **Complete, and streamed to stay that way.** There is no row cap: a search matching
//! the entire catalog exports the entire catalog. The response body is never assembled in
//! memory — rows are read in chunks and rendered out as they're produced (see [`drain`]).
//! Measured on a release build against a 300k-card catalog: the full 11 MB export runs in
//! ~4s for no measurable change in RSS. (The per-chunk re-acquire costs ~1.5s against a
//! single streamed query — the price of the availability property below, and worth it.)
//!
//! Two things the drain must keep doing, both learned the hard way:
//!
//! * **Never hold a database connection while awaiting the client.** SeaORM's row stream
//!   owns its `PoolConnection` for the stream's whole life, and sea-orm pins the SQLite
//!   backend — the default — to a *single* pooled connection. An earlier version of this
//!   module streamed one query straight to the client, which let one unauthenticated slow
//!   reader hold the process's only connection for as long as it liked; every other
//!   request, `/api/ready` included, then died on the pool's 30s acquire timeout. So the
//!   drain resolves its row list first and re-acquires per chunk, and every `await` on
//!   the client happens with nothing checked out.
//! * **Keep the channel bounded**, so a slow reader backpressures the drain instead of
//!   letting rendered chunks pile up. Bounded-but-connection-free is the combination that
//!   is both memory-safe and availability-safe; either alone is not enough.
//!
//! A consequence worth knowing: the status line is committed before the first row is
//! read, so a mid-export database failure can't become a `500`. It instead appends a
//! `# …incomplete` comment **and** fails the transfer, so neither a human reading the
//! file nor the browser downloading it mistakes a short file for the whole search.

use axum::body::{Body, Bytes};
use axum::response::Response;
use futures_util::stream;
use sea_orm::sea_query::SelectStatement;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect, Select,
    SelectGetableTuple, Selector,
};
use std::collections::{HashMap, HashSet};
use std::io;
use tokio::sync::mpsc;

use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::state::AppState;

use super::download::text_download_stream;

/// How many rows accumulate before a rendered chunk is pushed to the client.
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
pub(crate) enum CardExportFormat {
    /// `N Name (SET) 123` per printing (holdings add a ` *F*` line for foil copies) —
    /// importable by this app and by deck sites.
    Text,
    /// Bare card names, de-duplicated across printings.
    Names,
}

impl CardExportFormat {
    /// Parse the `?format=` param. Absent/blank is [`CardExportFormat::Text`]; anything
    /// unrecognised is a 422 (consistent with a malformed `q` or `sort`, and with the
    /// collection-CSV/deck exports) rather than a silent fall back to the default.
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, AppError> {
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

/// What an export drains — the two query shapes the engine accepts. Both arrive
/// filtered + sorted by the endpoint's own listing builder; only the row shape differs.
enum Source {
    /// A public catalog card search. Every row exports as a single regular copy
    /// (`1 …`, no foil line).
    Catalog(Select<card::Entity>),
    /// A holdings listing (collection / wish list), already narrowed to
    /// `(card_id, quantity, foil_quantity)` in the listing's own order (see
    /// [`super::holdings::narrow_export_statement`]). Rows render with the real held
    /// counts: one line per non-empty finish, the foil line tagged ` *F*`. A holdings
    /// row is never both-zero (that deletes it), so every row earns at least one line.
    Holdings(SelectStatement),
}

/// One resolved row of an export: which card, and how many copies of each finish its
/// line(s) should claim.
struct ExportItem {
    card_id: i32,
    quantity: i32,
    foil_quantity: i32,
}

/// Run an (already filtered + sorted) catalog card search as a streaming `.txt`
/// download. Every row renders as `1 Name (SET) 123` — the quantity the text grammar
/// requires, not a claim of ownership.
pub(crate) fn render_catalog_export(
    state: &AppState,
    query: Select<card::Entity>,
    format: CardExportFormat,
    filename_prefix: &str,
) -> Result<Response, AppError> {
    render_export(state, Source::Catalog(query), format, filename_prefix)
}

/// Run an (already filtered + sorted + narrowed) holdings listing as a streaming `.txt`
/// download, rendering the held counts: `N Name (SET) 123` per non-empty finish, foil
/// tagged ` *F*` — so the file re-imports through the text importer with quantities and
/// finishes intact.
pub(crate) fn render_holdings_export(
    state: &AppState,
    query: SelectStatement,
    format: CardExportFormat,
    filename_prefix: &str,
) -> Result<Response, AppError> {
    render_export(state, Source::Holdings(query), format, filename_prefix)
}

/// Run the query and hand back a streaming download.
///
/// The work happens in a detached task feeding a bounded channel that backs the response
/// body; see [`drain`] for the two-phase read and why it is shaped that way.
fn render_export(
    state: &AppState,
    source: Source,
    format: CardExportFormat,
    filename_prefix: &str,
) -> Result<Response, AppError> {
    // Bounded, so a client that reads slowly applies backpressure to the DB drain rather
    // than letting rendered chunks pile up in memory.
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(EXPORT_CHANNEL_CHUNKS);
    // The task outlives this call, so it owns its own handle (`DatabaseConnection` is a
    // cheap `Arc` clone). It borrows a *pooled connection* only per query, never across a
    // send — see `drain`.
    let db = state.db.clone();
    tokio::spawn(async move { drain(db, source, format, tx).await });

    text_download_stream(
        Body::from_stream(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })),
        &format!("{filename_prefix}-{}.txt", format.slug()),
    )
}

/// The three card columns an export line is made of.
///
/// Narrowing the select matters here in a way it doesn't for the paged listings: a `cards`
/// row carries ~70 columns including several JSON blobs (faces, legalities) and five image
/// URLs, and hydrating all of them for every row of a full-catalog drain — to render three
/// of them — dominates the export's runtime. The listings pay that cost for 60 rows; an
/// uncapped export would pay it for every printing in the game.
type ExportRow = (String, String, String);

/// Resolve the rows to export, in the listing's order, with one query.
///
/// The filters and ordering are untouched, so this stays the same search the grid ran —
/// `apply_card_sort` orders by columns that aren't selected, which is fine in a plain
/// `SELECT` on both backends (and the SQLite unique-mode `GROUP BY` tolerates it too).
async fn export_items(db: &DatabaseConnection, source: Source) -> Result<Vec<ExportItem>, DbErr> {
    match source {
        Source::Catalog(query) => Ok(query
            .select_only()
            .column(card::Column::Id)
            .into_tuple::<i32>()
            .all(db)
            .await?
            .into_iter()
            .map(|card_id| ExportItem {
                card_id,
                quantity: 1,
                foil_quantity: 0,
            })
            .collect()),
        Source::Holdings(query) => Ok(Selector::<SelectGetableTuple<(i32, i32, i32)>>::into_tuple(
            query,
        )
        .all(db)
        .await?
        .into_iter()
        .map(|(card_id, quantity, foil_quantity)| ExportItem {
            card_id,
            quantity,
            foil_quantity,
        })
        .collect()),
    }
}

/// Hydrate one chunk of items into [`ExportRow`]s, back in the order `items` gives.
///
/// `IN (...)` returns rows in whatever order the backend likes, so the chunk's own order
/// is re-imposed here — that's what preserves the listing's sort across chunks. An item
/// whose card no longer resolves (deleted between the two phases, or a holding whose card
/// row a catalog re-import removed) is simply skipped, exactly as the listings skip it.
async fn export_chunk(
    db: &DatabaseConnection,
    items: &[ExportItem],
) -> Result<Vec<(ExportRow, i32, i32)>, DbErr> {
    let fetched: Vec<(i32, String, String, String)> = Card::find()
        .filter(card::Column::Id.is_in(items.iter().map(|item| item.card_id)))
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::Name)
        .column(card::Column::SetCode)
        .column(card::Column::CollectorNumber)
        .into_tuple()
        .all(db)
        .await?;
    let mut by_id: HashMap<i32, ExportRow> = fetched
        .into_iter()
        .map(|(id, name, set_code, number)| (id, (name, set_code, number)))
        .collect();
    Ok(items
        .iter()
        .filter_map(|item| {
            by_id
                .remove(&item.card_id)
                .map(|row| (row, item.quantity, item.foil_quantity))
        })
        .collect())
}

/// Drain the source into rendered chunks on `tx`. Runs detached: a send failure means the
/// client hung up, which is a normal end, not an error.
///
/// **Never holds a database connection while awaiting the client.** That is the whole
/// shape of this function, and it is not an optimisation: SeaORM's row stream owns the
/// `PoolConnection` it was created from for the stream's entire life, and the SQLite
/// backend — the default — is pinned by sea-orm to a *single* pooled connection. Draining
/// one stream across a client-paced transfer therefore let one unauthenticated slow reader
/// hold the process's only connection indefinitely, and every other request (including
/// `/api/ready`) then failed on the pool's acquire timeout. So instead: resolve the rows in
/// one query, then re-acquire per chunk. Each `await` on `tx` happens with no connection
/// checked out.
///
/// The cost of that is a snapshot: the row list and each chunk are separate reads, so a card
/// edited mid-export can render with its newer values, and one deleted mid-export drops out.
/// For a catalog that changes on a daily sync that is a fair trade for not being trivially
/// DoS-able.
async fn drain(
    db: DatabaseConnection,
    source: Source,
    format: CardExportFormat,
    tx: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    // Phase 1: which rows, and in what order. One query, released as soon as it returns —
    // it is never awaited against the client. A handful of integers a row, so even a
    // whole-catalog export is a couple of megabytes here rather than a result set.
    let items = match export_items(&db, source).await {
        Ok(items) => items,
        Err(error) => {
            // Nothing has been written yet, but the 200 is already on the wire, so this
            // can't become a 500 — say so in the body and fail the transfer.
            tracing::error!(%error, "card export query failed to start");
            fail(&tx, None, "card export query failed").await;
            return;
        }
    };

    // Phase 2: render a chunk at a time, re-acquiring a connection per chunk and giving it
    // back before the (client-paced) send.
    let mut writer = ChunkWriter::new(format);
    for chunk in items.chunks(EXPORT_CHUNK_CARDS) {
        let rows = match export_chunk(&db, chunk).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "card export failed part-way");
                fail(&tx, Some(writer.finish()), "card export failed part-way").await;
                return;
            }
        };
        for (row, quantity, foil_quantity) in &rows {
            writer.push(row, *quantity, *foil_quantity);
        }
        // `names` folds duplicates away, so a chunk of rows can render fewer lines than
        // it has rows; flush whatever is ready rather than only full chunks.
        if tx.send(Ok(writer.take())).await.is_err() {
            return; // client hung up
        }
    }
    let _ = tx.send(Ok(writer.finish())).await;
}

/// End a broken export: flush anything already rendered, mark the file incomplete, then
/// error the transfer — so neither a human reading the file nor the browser downloading it
/// mistakes a short file for the whole result set.
async fn fail(
    tx: &mpsc::Sender<Result<Bytes, io::Error>>,
    pending: Option<Bytes>,
    message: &'static str,
) {
    if let Some(pending) = pending {
        let _ = tx.send(Ok(pending)).await;
    }
    let _ = tx.send(Ok(Bytes::from_static(FAILED_NOTE))).await;
    let _ = tx.send(Err(io::Error::other(message))).await;
}

/// Accumulates rendered card lines and hands them out a chunk at a time.
///
/// Owns the `names` de-duplication, which is why it spans the whole export rather than
/// living per-chunk: a name first seen in chunk 1 must not reappear in chunk 40. The set
/// is bounded by *distinct names* (tens of thousands for a full catalog), not printings.
struct ChunkWriter {
    format: CardExportFormat,
    buffer: String,
    seen: HashSet<String>,
}

impl ChunkWriter {
    fn new(format: CardExportFormat) -> Self {
        Self {
            format,
            buffer: String::new(),
            seen: HashSet::new(),
        }
    }

    /// Render one row, if it earns lines in this format: in `text`, one line per
    /// non-empty finish count (the catalog source always passes `(1, 0)` — exactly one
    /// regular line); in `names`, one line the first time the name is seen, however
    /// many copies the row carries.
    fn push(&mut self, row: &ExportRow, quantity: i32, foil_quantity: i32) {
        let (name, set_code, collector_number) = row;
        match self.format {
            CardExportFormat::Text => {
                if quantity > 0 {
                    self.buffer.push_str(&text_row(
                        name,
                        set_code,
                        collector_number,
                        quantity,
                        false,
                    ));
                }
                if foil_quantity > 0 {
                    self.buffer.push_str(&text_row(
                        name,
                        set_code,
                        collector_number,
                        foil_quantity,
                        true,
                    ));
                }
            }
            CardExportFormat::Names => {
                // First appearance wins its place in the query's order.
                if self.seen.insert(name.clone()) {
                    self.buffer.push_str(name);
                    self.buffer.push('\n');
                }
            }
        }
    }

    /// Whatever is left (possibly empty — an export matching nothing is an empty file).
    fn finish(&mut self) -> Bytes {
        self.take()
    }

    pub(self) fn take(&mut self) -> Bytes {
        Bytes::from(std::mem::take(&mut self.buffer))
    }
}

/// One printing-finish as `N Name (SET) 123`, foil tagged ` *F*` — the deck export's
/// text row (and the marker [`crate::collection_import::text_list`] /
/// [`crate::deck_import::parser`] read back).
fn text_row(
    name: &str,
    set_code: &str,
    collector_number: &str,
    quantity: i32,
    foil: bool,
) -> String {
    format!(
        "{quantity} {name} ({}) {collector_number}{}\n",
        set_code.to_ascii_uppercase(),
        if foil { " *F*" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, set_code: &str, number: &str) -> ExportRow {
        (name.to_string(), set_code.to_string(), number.to_string())
    }

    /// A catalog-shaped row: one regular copy, no foils.
    fn catalog_row(name: &str, set_code: &str, number: &str) -> (ExportRow, i32, i32) {
        (card(name, set_code, number), 1, 0)
    }

    /// Render every row through the same chunked path `drain` uses, minus the database:
    /// push a chunk's worth, flush, repeat.
    fn render_all(rows: &[(ExportRow, i32, i32)], format: CardExportFormat) -> String {
        join(&chunks_of(rows, format))
    }

    /// The chunks a client would receive for `rows`, in order.
    fn chunks_of(rows: &[(ExportRow, i32, i32)], format: CardExportFormat) -> Vec<Bytes> {
        let mut writer = ChunkWriter::new(format);
        let mut out = Vec::new();
        for chunk in rows.chunks(EXPORT_CHUNK_CARDS) {
            for (row, quantity, foil_quantity) in chunk {
                writer.push(row, *quantity, *foil_quantity);
            }
            out.push(writer.take());
        }
        out.push(writer.finish());
        out
    }

    /// The chunks a client would receive, concatenated back into the delivered file.
    fn join(chunks: &[Bytes]) -> String {
        String::from_utf8(chunks.iter().flatten().copied().collect()).expect("export is UTF-8")
    }

    #[test]
    fn format_parses_case_insensitively_and_defaults_to_text() {
        assert_eq!(
            CardExportFormat::parse(None).unwrap(),
            CardExportFormat::Text
        );
        assert_eq!(
            CardExportFormat::parse(Some("")).unwrap(),
            CardExportFormat::Text
        );
        assert_eq!(
            CardExportFormat::parse(Some("  TEXT ")).unwrap(),
            CardExportFormat::Text
        );
        assert_eq!(
            CardExportFormat::parse(Some("Names")).unwrap(),
            CardExportFormat::Names
        );
        // An unrecognised shape is a 422, not a silent default.
        assert!(matches!(
            CardExportFormat::parse(Some("csv")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn catalog_text_rows_are_quantity_name_set_number() {
        let rows = [
            catalog_row("Sol Ring", "ltc", "284"),
            catalog_row("Lightning Bolt", "2x2", "117"),
        ];
        assert_eq!(
            render_all(&rows, CardExportFormat::Text),
            "1 Sol Ring (LTC) 284\n1 Lightning Bolt (2X2) 117\n"
        );
    }

    #[test]
    fn holdings_text_rows_carry_real_counts_and_a_foil_line() {
        // A held card renders one line per non-empty finish: the regular count bare, the
        // foil count tagged with the ` *F*` marker the text importer reads back.
        let rows = [
            (card("Sol Ring", "ltc", "284"), 4, 1),
            (card("Arcane Signet", "eld", "331"), 0, 2),
            (card("Lightning Bolt", "2x2", "117"), 3, 0),
        ];
        assert_eq!(
            render_all(&rows, CardExportFormat::Text),
            "4 Sol Ring (LTC) 284\n\
             1 Sol Ring (LTC) 284 *F*\n\
             2 Arcane Signet (ELD) 331 *F*\n\
             3 Lightning Bolt (2X2) 117\n"
        );
    }

    #[test]
    fn names_dedupe_across_printings_and_keep_query_order() {
        let rows = [
            catalog_row("Sol Ring", "ltc", "284"),
            catalog_row("Sol Ring", "c21", "263"),
            catalog_row("Arcane Signet", "eld", "331"),
            catalog_row("Sol Ring", "cmr", "472"),
        ];
        assert_eq!(
            render_all(&rows, CardExportFormat::Names),
            "Sol Ring\nArcane Signet\n"
        );
    }

    #[test]
    fn names_ignore_counts_and_finishes() {
        // A foil-only holding is still one name; counts never multiply a name line.
        let rows = [
            (card("Sol Ring", "ltc", "284"), 0, 3),
            (card("Sol Ring", "c21", "263"), 9, 9),
        ];
        assert_eq!(render_all(&rows, CardExportFormat::Names), "Sol Ring\n");
    }

    #[test]
    fn an_export_is_pure_card_lines() {
        let rows = [(card("Sol Ring", "ltc", "284"), 2, 1)];
        for format in [CardExportFormat::Text, CardExportFormat::Names] {
            // No cap means no truncation note — nothing but cards unless something broke.
            assert!(!render_all(&rows, format).contains('#'));
        }
    }

    #[test]
    fn an_export_matching_nothing_is_an_empty_file() {
        assert_eq!(render_all(&[], CardExportFormat::Text), "");
        assert_eq!(render_all(&[], CardExportFormat::Names), "");
    }

    #[test]
    fn chunking_splits_the_stream_without_losing_or_repeating_a_row() {
        // Two and a half chunks' worth: the boundary logic must not drop the partial
        // tail, duplicate a row, or reorder anything.
        let rows: Vec<(ExportRow, i32, i32)> = (0..EXPORT_CHUNK_CARDS * 2 + 7)
            .map(|i| catalog_row(&format!("Card {i}"), "dmb", &i.to_string()))
            .collect();

        let chunks: Vec<Bytes> = chunks_of(&rows, CardExportFormat::Text)
            .into_iter()
            .filter(|c| !c.is_empty())
            .collect();

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
        let mut rows: Vec<(ExportRow, i32, i32)> = (0..EXPORT_CHUNK_CARDS + 5)
            .map(|i| catalog_row(&format!("Card {i}"), "dmb", &i.to_string()))
            .collect();
        rows.push(catalog_row("Card 0", "dmu", "1"));

        let rendered = render_all(&rows, CardExportFormat::Names);
        assert_eq!(
            rendered.lines().filter(|line| *line == "Card 0").count(),
            1,
            "the reprint from a later chunk must not emit a second line"
        );
        assert_eq!(rendered.lines().count(), EXPORT_CHUNK_CARDS + 5);
    }

    #[test]
    fn filename_slugs_differ_per_shape() {
        assert_eq!(CardExportFormat::Text.slug(), "cards");
        assert_eq!(CardExportFormat::Names.slug(), "card-names");
    }
}
