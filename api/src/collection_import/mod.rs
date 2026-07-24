//! Provider-agnostic collection import / sync.
//!
//! Pulls a user's card collection from an external collection service and reconciles
//! it into the local per-user `collection_items`. The provider layer is a thin
//! fetch/parse boundary — one module per service (Archidekt and Moxfield today),
//! dispatched by the [`Provider`] enum (mirroring how `catalog` dispatches per game).
//! Everything downstream — aggregation, external-id resolution, reconcile, apply — is
//! provider-independent.
//!
//! A provider exposes each card by an id in the form our catalog stores as
//! `cards.external_id` (for both providers that is the Scryfall id), so a fetched
//! holding maps straight onto a local card by a single indexed lookup. Cards with no
//! match in our catalog are reported as "unmatched" and skipped.
//!
//! Not every source is a live fetch. A **file or pasted text** import ([`execute_file_import`])
//! covers the Moxfield CSV (no card id at all — rows resolve by set code + collector
//! number), the Mythic Tools CSV, and the plain-text card lists every app can copy out.

pub(crate) mod archidekt;
mod consolidate;
pub(crate) mod csv_import;
mod error;
pub mod jobs;
pub(crate) mod moxfield;
mod progress;
pub mod rate_limit;
pub(crate) mod reconcile;
mod smart;
pub(crate) mod text_list;
mod types;

use std::collections::HashMap;

use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

pub use error::ImportError;
pub use progress::{ProgressReporter, ProgressSnapshot};
use reconcile::{reconcile_holdings, reconcile_smart};
use smart::smart_absorb_page;
pub use types::*;

/// Hard cap on how many holding rows we'll pull from a provider in one import, so a
/// request can't make us fan out an unbounded number of upstream page fetches.
pub(crate) const MAX_IMPORT_ROWS: usize = 100_000;

/// How many unmatched card ids we surface in the summary (a debugging aid — the full
/// count is always reported).
pub(crate) const UNMATCHED_SAMPLE_CAP: usize = 20;

/// SQLite caps host parameters per statement (as few as 999 on old builds), so any
/// `IN (...)` lookup/delete over the imported cards is batched into chunks comfortably
/// under that limit — a large collection can carry far more distinct cards than that.
pub(crate) const IN_CHUNK: usize = 900;

/// Everything a provider fetch needs beyond the collection id: the shared HTTP client,
/// the per-provider rate limiters, and deployment-level provider settings.
/// Borrowed for the duration of one import.
pub struct ProviderContext<'a> {
    pub http: &'a reqwest::Client,
    pub limiters: &'a rate_limit::ProviderLimiters,
    pub settings: &'a ProviderSettings,
    /// Where the fetch loop publishes its live row-progress (rows fetched / total), for
    /// the job-status endpoint's progress bar.
    pub progress: &'a ProgressReporter,
}

/// Parse the collection id from a user-supplied source (a full provider URL or a bare
/// id). Pure and provider-specific.
pub fn parse_source(provider: Provider, input: &str) -> Result<String, ImportError> {
    let parsed = match provider {
        Provider::Archidekt => archidekt::parse_collection_id(input),
        Provider::Moxfield => moxfield::parse_collection_id(input),
        // No addressable collections, so nothing can parse to an id. Callers gate on
        // `network_import_enabled` first, so this is a defensive fallthrough.
        Provider::MythicTools => None,
    };
    parsed.ok_or_else(|| {
        ImportError::InvalidSource(format!(
            "couldn't read a {} collection id from '{}'",
            provider.label(),
            input.trim()
        ))
    })
}

/// Fetch every holding for a provider collection id, throttled by that provider's rate
/// limiter.
async fn fetch_holdings(
    provider: Provider,
    ctx: &ProviderContext<'_>,
    collection_id: &str,
) -> Result<Vec<FetchedHolding>, ImportError> {
    match provider {
        Provider::Archidekt => {
            archidekt::fetch(
                ctx.http,
                ctx.limiters.for_provider(provider),
                collection_id,
                ctx.progress,
            )
            .await
        }
        Provider::Moxfield => moxfield::fetch(ctx, collection_id).await,
        Provider::MythicTools => Err(no_live_fetch(provider)),
    }
}

/// The error for asking a file/paste-only provider to fetch. Unreachable in practice —
/// every caller gates on [`Provider::network_import_enabled`] first — but it keeps the
/// dispatch total without an `unreachable!` on a request path.
fn no_live_fetch(provider: Provider) -> ImportError {
    ImportError::InvalidSource(format!(
        "{} collections can't be fetched — import an export file or paste the list instead",
        provider.label()
    ))
}

/// The recently-updated prefix of a provider collection for a smart sync: fetch
/// most-recently-updated first and stop once a whole page already matches `local`
/// (`external_card_id -> (quantity, foil_quantity)`). Returns the fetched holdings plus
/// whether we stopped early (reached the already-synced tail) rather than scanning the
/// whole collection.
async fn fetch_holdings_smart(
    provider: Provider,
    ctx: &ProviderContext<'_>,
    collection_id: &str,
    local: &HashMap<String, (i32, i32)>,
    remap: &HashMap<String, String>,
) -> Result<(Vec<FetchedHolding>, bool), ImportError> {
    match provider {
        Provider::Archidekt => {
            let limiter = ctx.limiters.for_provider(provider);
            archidekt::fetch_smart(ctx.http, limiter, collection_id, local, remap, ctx.progress)
                .await
        }
        Provider::Moxfield => moxfield::fetch_smart(ctx, collection_id, local, remap).await,
        Provider::MythicTools => Err(no_live_fetch(provider)),
    }
}

/// Execute an import against an already-parsed provider collection id: fetch (throttled),
/// aggregate, resolve to local cards, reconcile per `mode`, and apply in one transaction.
/// Runs in the background worker (see [`jobs`]); the handler does the synchronous
/// validation (provider/game/source) before enqueuing, so this trusts its inputs.
pub async fn execute_import(
    db: &DatabaseConnection,
    ctx: &ProviderContext<'_>,
    user_id: i32,
    game: &str,
    provider: Provider,
    collection_id: &str,
    mode: ReconcileMode,
) -> Result<ImportSummary, ImportError> {
    if mode == ReconcileMode::Smart {
        // Fold separately-modelled foil printings (`…★`) onto their base card as foil
        // copies (issue #209). Resolve the pairs once (drives both the fetch's early-stop
        // and the reconcile); the non-smart path does the same inside `reconcile_holdings`.
        let pairs = consolidate::load_foil_variant_pairs(db, game).await?;
        // Fold any star row the user already holds onto its base first, so a manual/legacy
        // star holding can't coexist with the base and double-count. Must run before the
        // `local` snapshot below so it reflects the folded state.
        consolidate::fold_existing_star_holdings(db, user_id, game, &pairs).await?;
        let remap = consolidate::ext_remap(&pairs);
        // Smart needs the current collection up front to drive the early-stop (fetch
        // until a page already matches what we hold). Note this snapshot is only for the
        // stop decision — the reconcile re-reads current counts after the (minutes-long)
        // fetch, so a concurrent edit during the fetch isn't clobbered. Fold it through
        // the remap so a held foil-★ matches its re-fetched, remapped page.
        let local = consolidate::consolidate_local(
            load_local_by_external(db, user_id, game).await?,
            &remap,
        );
        let (holdings, stopped_early) =
            fetch_holdings_smart(provider, ctx, collection_id, &local, &remap).await?;
        if holdings.is_empty() {
            return Err(ImportError::EmptyCollection);
        }
        // `holdings` already carry base external ids (the smart fetch remapped each page),
        // so the reconcile sees a straight 1:1 external-id→card mapping.
        return reconcile_smart(db, user_id, game, provider, holdings, stopped_early).await;
    }

    let holdings = fetch_holdings(provider, ctx, collection_id).await?;
    if holdings.is_empty() {
        return Err(ImportError::EmptyCollection);
    }
    reconcile_holdings(db, user_id, game, provider, mode, holdings).await
}

/// The user's current collection for `(user, game)` keyed by the provider's external
/// card id (`cards.external_id`) — the shape a smart fetch compares fetched holdings
/// against. Empty when nothing is owned. Card ids are resolved in chunks to stay under
/// SQLite's per-statement bind-variable limit.
async fn load_local_by_external(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<HashMap<String, (i32, i32)>, ImportError> {
    let items = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .all(db)
        .await
        .map_err(ImportError::Db)?;
    if items.is_empty() {
        return Ok(HashMap::new());
    }

    let card_ids: Vec<i32> = items.iter().map(|i| i.card_id).collect();
    let mut external_by_id: HashMap<i32, String> = HashMap::new();
    for chunk in card_ids.chunks(IN_CHUNK) {
        // Only the two id columns; skip the ~66 heavy card columns we never read here.
        let rows: Vec<(i32, String)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::ExternalId)
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        for (id, external_id) in rows {
            external_by_id.insert(id, external_id);
        }
    }

    let mut local = HashMap::with_capacity(items.len());
    for i in items {
        if let Some(ext) = external_by_id.get(&i.card_id) {
            local.insert(ext.clone(), (i.quantity, i.foil_quantity));
        }
    }
    Ok(local)
}

/// Import a collection from an **uploaded or pasted** export, sniffing the format from the
/// content itself.
///
/// Three shapes are recognised, in order (see [`csv_import::parse_csv`] for the CSV
/// sniffing and [`text_list`] for the text grammar):
///
/// 1. an **Archidekt** CSV — rows carry Scryfall ids, already the engine's shape;
/// 2. a **Moxfield** or **Mythic Tools** CSV — rows identify a printing by set code +
///    collector number (Mythic Tools rows may carry an id too, and take it when present);
/// 3. a **plain-text card list** (`1 Sol Ring (C21) 263 *F*`) — the copy/paste and TXT
///    export format shared by Mythic Tools, Moxfield, Archidekt and MTGA.
///
/// The text list is the fallback: it runs only when the content matches no CSV header we
/// know, so a genuine CSV never silently degrades into it. When neither reads, the 422
/// names every supported format.
///
/// Everything downstream is the same aggregate / resolve / reconcile / apply path as a
/// network import — but with no upstream fetch, so no rate limiter or background job is
/// involved and this runs inline in the request. An upload has no persistent location, so
/// it's always a one-off; the caller picks the `mode`. An empty parse (no usable rows) is
/// refused so a `Replace` can't silently wipe the collection against a blank upload.
pub async fn execute_file_import(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    mode: ReconcileMode,
    bytes: &[u8],
) -> Result<ImportSummary, ImportError> {
    let parsed = match csv_import::parse_csv(bytes)? {
        Some(parsed) => parsed,
        None => parse_text_list(bytes)?,
    };
    let csv_import::ParsedCsv {
        provider,
        mut holdings,
        printings,
    } = parsed;
    holdings.extend(printing_rows_to_holdings(db, game, printings).await?);
    if holdings.is_empty() {
        return Err(ImportError::EmptyCollection);
    }
    reconcile_holdings(db, user_id, game, provider, mode, holdings).await
}

/// Read the content as a plain-text card list (one `1 Sol Ring (C21) 263 *F*` line per
/// holding). Every line resolves by printing key or by name, so the rows all land in the
/// `printings` bucket.
///
/// Reported as [`Provider::MythicTools`] because that's the source this format was added
/// for (issue #572) and there is nothing in a bare text list that could identify which app
/// wrote it — the summary line names where we *understood* it from, not a claim about the
/// user's app.
fn parse_text_list(bytes: &[u8]) -> Result<csv_import::ParsedCsv, ImportError> {
    let text = std::str::from_utf8(csv_import::strip_bom(bytes))
        .map_err(|_| ImportError::InvalidSource(UNRECOGNISED_FORMAT.to_string()))?;

    let mut printings = Vec::new();
    let mut rows_seen = 0usize;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // A non-card line is a section/board header or a stray note. A collection has no
        // sections, so it's simply skipped (and never counted against the row cap).
        let text_list::TextListLine::Card(row) = text_list::parse_line(line) else {
            continue;
        };
        rows_seen += 1;
        if rows_seen > MAX_IMPORT_ROWS {
            return Err(ImportError::TooLarge {
                count: rows_seen,
                max: MAX_IMPORT_ROWS,
            });
        }
        // A card-shaped line with no usable name still counted against the cap above.
        let Some(row) = row else { continue };
        printings.push(csv_import::PrintingRow {
            set_code: row.set_code,
            collector_number: row.collector_number,
            name: row.name,
            foil: row.foil,
            quantity: row.quantity,
        });
    }

    if printings.is_empty() {
        return Err(ImportError::InvalidSource(UNRECOGNISED_FORMAT.to_string()));
    }
    Ok(csv_import::ParsedCsv {
        provider: Provider::MythicTools,
        holdings: Vec::new(),
        printings,
    })
}

/// The 422 for content that matched no CSV header and held no readable card lines. It
/// names every format we accept, since at this point we have no idea what the user meant.
const UNRECOGNISED_FORMAT: &str = "that doesn't look like a collection we can read. Paste a card list (one card per \
     line, e.g. \"2 Sol Ring (C21) 263\"), or upload a CSV export from Mythic Tools, \
     Archidekt (Scryfall ID, Finish, Quantity), or Moxfield (Count, Edition, Collector \
     Number, Foil).";

/// Resolve rows identified by `(set code, collector number)` — or, for a plain-text list,
/// by card name alone — into the engine's normalized holdings.
///
/// Each distinct `(set, number)` pair is looked up against the catalog (per set, chunked
/// under SQLite's bind-variable limit; set codes are already lowercased to match the
/// catalog, numbers compare exactly). A matched row's holding carries the card's
/// `external_id`, so downstream aggregation merges it with any other spelling of the same
/// printing. An unmatched row keeps a **readable placeholder key** (`"Name (set #num)"`)
/// instead: placeholders can never collide with a real external id (Scryfall UUIDs don't
/// contain `#` or spaces), so the engine counts them as unmatched and surfaces them in
/// the summary's sample verbatim — far more useful to a user than a bare UUID. The
/// placeholder is keyed off the pair's first-seen row so duplicate rows still aggregate.
///
/// A row that names **no** printing (only possible from a text list — every CSV shape
/// requires the pair) falls back to the newest catalog printing of that exact name, the
/// same rule the deck importer uses. A row that *did* name a printing never takes that
/// fallback: a supplied-but-unmatched pair stays unmatched rather than silently importing
/// a different art at a different price.
pub(crate) async fn printing_rows_to_holdings(
    db: &DatabaseConnection,
    game: &str,
    rows: Vec<csv_import::PrintingRow>,
) -> Result<Vec<FetchedHolding>, ImportError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    // Name-only rows resolve against the catalog by exact name (newest printing wins).
    let names: Vec<String> = rows
        .iter()
        .filter(|row| row.pair().is_none() && !row.name.is_empty())
        .map(|row| row.name.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let by_name = reconcile::resolve_newest_printing_by_name(db, game, &names).await?;

    // Distinct (set, number) pairs to look up, and each pair's unmatched-placeholder
    // label (from its first-seen row, so the key is deterministic).
    let mut placeholder_by_pair: HashMap<(&str, &str), String> = HashMap::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let Some(pair) = row.pair() else { continue };
        if placeholder_by_pair.contains_key(&pair) {
            continue;
        }
        let label = if row.name.is_empty() {
            format!("{} #{}", pair.0, pair.1)
        } else {
            format!("{} ({} #{})", row.name, pair.0, pair.1)
        };
        placeholder_by_pair.insert(pair, label);
        pairs.push((pair.0.to_string(), pair.1.to_string()));
    }

    // (set, number) -> external id, via row-value `(set_code, collector_number) IN
    // ((?,?),…)` chunks. Chunking by PAIRS (two binds each, under SQLite's per-statement
    // bind limit) means the query count is bounded by distinct pairs alone — a crafted
    // upload naming 100k *distinct set codes* costs the same handful of queries as a
    // genuine export, rather than one query per set (an easy DoS amplification since
    // this runs synchronously in the request).
    let mut external_by_pair: HashMap<(String, String), String> = HashMap::new();
    for chunk in pairs.chunks(IN_CHUNK / 2) {
        // Project only (set_code, collector_number, external_id) of the card's ~65 columns — up to
        // 450 rows a chunk. The seek is served by `idx_cards_game_set_code_collector_number` (m..024);
        // with only `(game, set_code)` this row-value IN scans ~every card of each named set.
        let cards: Vec<(String, String, String)> = Card::find()
            .select_only()
            .column(card::Column::SetCode)
            .column(card::Column::CollectorNumber)
            .column(card::Column::ExternalId)
            .filter(card::Column::Game.eq(game))
            .filter(
                Expr::tuple([
                    Expr::col(card::Column::SetCode).into(),
                    Expr::col(card::Column::CollectorNumber).into(),
                ])
                .in_tuples(chunk.iter().cloned()),
            )
            .into_tuple()
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        for (set_code, collector_number, external_id) in cards {
            external_by_pair.insert((set_code, collector_number), external_id);
        }
    }

    Ok(rows
        .iter()
        .map(|row| {
            let external_card_id = match row.pair() {
                Some(pair) => external_by_pair
                    .get(&(pair.0.to_string(), pair.1.to_string()))
                    .cloned()
                    // Unmatched: keep the readable placeholder so the summary's
                    // unmatched sample names the card, not an opaque key.
                    .unwrap_or_else(|| {
                        placeholder_by_pair
                            .get(&pair)
                            .cloned()
                            .unwrap_or_else(|| format!("{} #{}", pair.0, pair.1))
                    }),
                // Name-only: the newest printing of that name, else the name itself as
                // the placeholder (same "readable unmatched key" contract as above).
                None => by_name
                    .get(&row.name)
                    .map(|(_, external_id)| external_id.clone())
                    .unwrap_or_else(|| row.name.clone()),
            };
            FetchedHolding {
                external_card_id,
                foil: row.foil,
                quantity: row.quantity,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        insert_card, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };

    #[tokio::test]
    async fn execute_file_import_parses_then_reconciles_over_a_db() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        // Two catalog cards keyed by the Scryfall ids the CSV references, plus one owned
        // card absent from the CSV (to prove Replace mirrors it away).
        let uid_a = "f369827d-e4cd-4bc7-8c5e-72882eff0908";
        let uid_b = "50a22ad6-d2a4-48a6-91c9-147c946a60a5";
        let a = insert_card(&db, uid_a).await;
        let b = insert_card(&db, uid_b).await;
        let stale = insert_card(&db, "not-in-the-csv").await;
        insert_holding(&db, user_id, stale, 4, 0).await;

        // A real-shaped export: extra columns, a quoted comma in a name, a foil + a regular,
        // and an unknown Scryfall id that should be reported as unmatched, not applied.
        let csv = "Quantity,Name,Finish,Scryfall ID\r\n\
                   2,\"Aang, Air Nomad\",Foil,f369827d-e4cd-4bc7-8c5e-72882eff0908\r\n\
                   3,Sol Ring,Normal,50a22ad6-d2a4-48a6-91c9-147c946a60a5\r\n\
                   1,Ghost Card,Normal,ffffffff-ffff-ffff-ffff-ffffffffffff\r\n";

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Replace,
            csv.as_bytes(),
        )
        .await
        .expect("csv import");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((0, 2)),
            "a imported as foil"
        );
        assert_eq!(
            owned_counts(&db, user_id, b).await,
            Some((3, 0)),
            "b imported as regular"
        );
        assert_eq!(
            owned_counts(&db, user_id, stale).await,
            None,
            "stale mirrored away"
        );
        assert_eq!(summary.provider, "archidekt");
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(
            summary.unmatched_cards, 1,
            "the ghost card isn't in the catalog"
        );
        assert_eq!(summary.removed_cards, 1);
    }

    /// Insert a minimal card with a specific set code + collector number (the key a
    /// Moxfield CSV identifies printings by).
    async fn insert_card_at(
        db: &DatabaseConnection,
        external_id: &str,
        set_code: &str,
        collector_number: &str,
    ) -> i32 {
        use sea_orm::{ActiveModelTrait, Set};
        let now = chrono::Utc::now();
        card::ActiveModel {
            game: Set(crate::scryfall::GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Card {external_id}")),
            set_code: Set(set_code.to_string()),
            set_name: Set("Test Set".to_string()),
            collector_number: Set(collector_number.to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert card")
        .id
    }

    #[tokio::test]
    async fn execute_file_import_resolves_a_moxfield_export_by_set_and_number() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "moxfield-csv@test.example").await;
        // Two printings the CSV references by (set, number) — including an uppercase
        // Edition cell that must match the catalog's lowercase set code — plus one row
        // pointing nowhere (unmatched, surfaced by name).
        let a = insert_card_at(&db, "ext-tle-146", "tle", "146").await;
        let b = insert_card_at(&db, "ext-tla-203", "tla", "203").await;

        let csv = "Count,Tradelist Count,Name,Edition,Foil,Collector Number,Proxy\n\
                   1,1,\"Aang, A Lot to Learn\",TLE,foil,146,False\n\
                   2,0,\"Aang, at the Crossroads // Aang, Destined Savior\",tla,,203,False\n\
                   1,0,Ghost Card,zzz,,999,False\n";

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Merge,
            csv.as_bytes(),
        )
        .await
        .expect("csv import");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((0, 1)),
            "foil row applied"
        );
        assert_eq!(
            owned_counts(&db, user_id, b).await,
            Some((2, 0)),
            "regular row applied"
        );
        assert_eq!(
            summary.provider, "moxfield",
            "the shape was sniffed as Moxfield"
        );
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(summary.unmatched_cards, 1);
        assert_eq!(
            summary.unmatched_sample,
            vec!["Ghost Card (zzz #999)".to_string()],
            "unmatched rows are labelled by name + set + number, not an opaque key"
        );
    }

    #[tokio::test]
    async fn execute_file_import_aggregates_duplicate_moxfield_rows_onto_one_card() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "moxfield-dupes@test.example").await;
        let a = insert_card_at(&db, "ext-tle-146", "tle", "146").await;

        // The same printing across three rows (differing condition/tags upstream): two
        // regular rows and a foil row must land on one holding.
        let csv = "Count,Name,Edition,Foil,Collector Number\n\
                   2,Aang,tle,,146\n\
                   1,Aang,tle,,146\n\
                   1,Aang,tle,foil,146\n";

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Overwrite,
            csv.as_bytes(),
        )
        .await
        .expect("csv import");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((3, 1)));
        assert_eq!(summary.distinct_cards, 1);
        assert_eq!(summary.total_rows, 3);
    }

    #[tokio::test]
    async fn execute_file_import_refuses_an_empty_upload() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 3, 0).await;

        // A header-only CSV yields no holdings; a Replace against it must be refused so an
        // empty upload can't wipe the collection.
        let err = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Replace,
            b"Scryfall ID,Finish,Quantity\n",
        )
        .await
        .expect_err("empty upload must be refused");
        assert!(matches!(err, ImportError::EmptyCollection));
        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((3, 0)),
            "collection untouched"
        );
    }

    // ---- Pasted / uploaded plain-text card lists (issue #572) ----

    #[tokio::test]
    async fn execute_file_import_reads_a_pasted_card_list() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "paste@test.example").await;
        let a = insert_card_at(&db, "ext-tle-146", "tle", "146").await;
        let b = insert_card_at(&db, "ext-c21-263", "c21", "263").await;

        // A Mythic Tools-shaped TXT export: quantities with and without the `x` suffix,
        // a foil marker, a blank line and a comment, plus a section header (a collection
        // has no sections, so it's simply ignored) and a line naming nothing we hold.
        let text = "# My binder\n\
                    \n\
                    Mainboard\n\
                    2 Aang, Air Nomad (TLE) 146 *F*\n\
                    3x Sol Ring (C21) 263\n\
                    1 Ghost Card (ZZZ) 999\n";

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Overwrite,
            text.as_bytes(),
        )
        .await
        .expect("text import");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((0, 2)),
            "foil line"
        );
        assert_eq!(
            owned_counts(&db, user_id, b).await,
            Some((3, 0)),
            "the `3x` spelling parses like `3`"
        );
        assert_eq!(
            summary.provider, "mythictools",
            "a bare text list is reported as the source it was added for"
        );
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(
            summary.unmatched_sample,
            vec!["Ghost Card (zzz #999)".to_string()],
            "an unmatched line is named, not shown as an opaque key"
        );
    }

    #[tokio::test]
    async fn a_pasted_line_without_a_set_matches_the_newest_printing() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "paste-name@test.example").await;
        // Two printings of one card. A bare name must resolve to the newer one — and
        // deterministically, so the same paste always lands on the same printing.
        let old = insert_card_at(&db, "ext-old", "lea", "232").await;
        let new = insert_card_at(&db, "ext-new", "2xm", "129").await;
        for (id, released) in [(old, "1993-08-05"), (new, "2020-08-07")] {
            Card::update_many()
                .col_expr(card::Column::Name, Expr::value("Counterspell".to_string()))
                .col_expr(
                    card::Column::ReleasedAt,
                    Expr::value(chrono::NaiveDate::parse_from_str(released, "%Y-%m-%d").unwrap()),
                )
                .filter(card::Column::Id.eq(id))
                .exec(&db)
                .await
                .expect("stamp printing");
        }

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Overwrite,
            b"4 Counterspell\n",
        )
        .await
        .expect("text import");

        assert_eq!(owned_counts(&db, user_id, new).await, Some((4, 0)));
        assert_eq!(owned_counts(&db, user_id, old).await, None);
        assert_eq!(summary.matched_cards, 1);
    }

    #[tokio::test]
    async fn a_named_but_unmatched_printing_never_falls_back_to_another_art() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "paste-strict@test.example").await;
        // The catalog holds this name — but at a different printing than the line names.
        // Importing the one we have would silently change the art (and the value), so the
        // line must stay unmatched instead.
        let held = insert_card_at(&db, "ext-2xm-129", "2xm", "129").await;
        Card::update_many()
            .col_expr(card::Column::Name, Expr::value("Counterspell".to_string()))
            .filter(card::Column::Id.eq(held))
            .exec(&db)
            .await
            .expect("name the card");

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Overwrite,
            b"4 Counterspell (LEA) 232\n",
        )
        .await
        .expect("import runs, it just matches nothing");

        assert_eq!(summary.matched_cards, 0);
        assert_eq!(
            summary.unmatched_sample,
            vec!["Counterspell (lea #232)".to_string()],
            "reported as unmatched, naming the printing the user asked for"
        );
        assert_eq!(
            owned_counts(&db, user_id, held).await,
            None,
            "the printing we do hold was left untouched"
        );
    }

    #[tokio::test]
    async fn unreadable_content_names_every_supported_format() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "paste-bad@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 3, 0).await;

        // Neither a CSV header we know nor anything with card-shaped lines.
        let err = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Replace,
            b"just some prose about magic cards\n",
        )
        .await
        .expect_err("unreadable content must be refused");
        let ImportError::InvalidSource(msg) = err else {
            panic!("expected InvalidSource, got {err:?}");
        };
        for format in ["Mythic Tools", "Archidekt", "Moxfield"] {
            assert!(msg.contains(format), "names {format}: {msg}");
        }
        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((3, 0)),
            "a refused Replace leaves the collection alone"
        );
    }

    #[tokio::test]
    async fn a_mythic_tools_csv_resolves_by_id_and_by_set_number_together() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "mythic-csv@test.example").await;
        let by_id = insert_card_at(&db, "f369827d-e4cd-4bc7-8c5e-72882eff0908", "tle", "146").await;
        let by_pair = insert_card_at(&db, "ext-c21-263", "c21", "263").await;

        // One row carries a Scryfall ID, the other only Set Code + Collector Number — the
        // app allows both, and a single export can mix them.
        let csv = "Amount,Name,Set Code,Set Name,Collector Number,Condition,Finish,Language,\
                   Extra Info,Assigned Price,Notes,Scryfall ID\n\
                   2,\"Aang, Air Nomad\",tle,Avatar Eternal,146,NM,foil,en,,,,\
                   f369827d-e4cd-4bc7-8c5e-72882eff0908\n\
                   3,Sol Ring,C21,Commander 2021,263,NM,Nonfoil,en,,,,\n";

        let summary = execute_file_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Overwrite,
            csv.as_bytes(),
        )
        .await
        .expect("mythic tools import");

        assert_eq!(owned_counts(&db, user_id, by_id).await, Some((0, 2)));
        assert_eq!(
            owned_counts(&db, user_id, by_pair).await,
            Some((3, 0)),
            "\"Nonfoil\" is a regular copy, not an unrecognised finish"
        );
        assert_eq!(summary.provider, "mythictools");
        assert_eq!(summary.matched_cards, 2);
    }
}
