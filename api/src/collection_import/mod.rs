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
//! match in our catalog are reported as "unmatched" and skipped. (A Moxfield **CSV**
//! carries no card id at all; its rows resolve by set code + collector number instead —
//! see [`execute_csv_import`].)

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
mod types;

use std::collections::HashMap;

use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

pub use error::ImportError;
pub use progress::{ProgressReporter, ProgressSnapshot};
pub use types::*;
use reconcile::{reconcile_holdings, reconcile_smart};
use smart::smart_absorb_page;

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
    }
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

/// Import a collection from an uploaded CSV export (Archidekt or Moxfield — the shape is
/// sniffed from the header row, see [`csv_import::parse_csv`]).
///
/// Parses the (untrusted, already size-bounded) CSV bytes into normalized holdings, then
/// runs the exact same aggregate / resolve / reconcile / apply path as a network import —
/// but with no upstream fetch, so no rate limiter or background job is involved and this
/// runs inline in the request. A CSV has no persistent location, so it's always a
/// one-off; the caller picks the `mode`. An empty parse (no usable rows) is refused so a
/// `Replace` can't silently wipe the collection against a blank upload.
///
/// An Archidekt export carries Scryfall ids, so its rows are already keyed the way the
/// engine expects. A Moxfield export carries **no card id** — its rows are first resolved
/// from `(set code, collector number)` to the catalog's external ids
/// ([`moxfield_rows_to_holdings`]); the summary's `provider` reflects the detected shape.
pub async fn execute_csv_import(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    mode: ReconcileMode,
    csv_bytes: &[u8],
) -> Result<ImportSummary, ImportError> {
    let (provider, holdings) = match csv_import::parse_csv(csv_bytes)? {
        csv_import::ParsedCsv::Archidekt(holdings) => (Provider::Archidekt, holdings),
        csv_import::ParsedCsv::Moxfield(rows) => (
            Provider::Moxfield,
            moxfield_rows_to_holdings(db, game, rows).await?,
        ),
    };
    if holdings.is_empty() {
        return Err(ImportError::EmptyCollection);
    }
    reconcile_holdings(db, user_id, game, provider, mode, holdings).await
}

/// Resolve Moxfield CSV rows — identified by `(set code, collector number)` rather than
/// a card id — into the engine's normalized holdings.
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
pub(crate) async fn moxfield_rows_to_holdings(
    db: &DatabaseConnection,
    game: &str,
    rows: Vec<csv_import::MoxfieldCsvRow>,
) -> Result<Vec<FetchedHolding>, ImportError> {
    // Distinct (set, number) pairs to look up, and each pair's unmatched-placeholder
    // label (from its first-seen row, so the key is deterministic).
    let mut placeholder_by_pair: HashMap<(&str, &str), String> = HashMap::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let pair = (row.set_code.as_str(), row.collector_number.as_str());
        if placeholder_by_pair.contains_key(&pair) {
            continue;
        }
        let label = if row.name.is_empty() {
            format!("{} #{}", row.set_code, row.collector_number)
        } else {
            format!("{} ({} #{})", row.name, row.set_code, row.collector_number)
        };
        placeholder_by_pair.insert(pair, label);
        pairs.push((row.set_code.clone(), row.collector_number.clone()));
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
            let pair = (row.set_code.as_str(), row.collector_number.as_str());
            let external_card_id = external_by_pair
                .get(&(row.set_code.clone(), row.collector_number.clone()))
                .cloned()
                // Unmatched: keep the readable placeholder so the summary's
                // unmatched sample names the card, not an opaque key.
                .unwrap_or_else(|| placeholder_by_pair[&pair].clone());
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
    async fn execute_csv_import_parses_then_reconciles_over_a_db() {
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

        let summary = execute_csv_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Replace,
            csv.as_bytes(),
        )
        .await
        .expect("csv import");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((0, 2)), "a imported as foil");
        assert_eq!(owned_counts(&db, user_id, b).await, Some((3, 0)), "b imported as regular");
        assert_eq!(owned_counts(&db, user_id, stale).await, None, "stale mirrored away");
        assert_eq!(summary.provider, "archidekt");
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(summary.unmatched_cards, 1, "the ghost card isn't in the catalog");
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
    async fn execute_csv_import_resolves_a_moxfield_export_by_set_and_number() {
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

        let summary = execute_csv_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Merge,
            csv.as_bytes(),
        )
        .await
        .expect("csv import");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((0, 1)), "foil row applied");
        assert_eq!(owned_counts(&db, user_id, b).await, Some((2, 0)), "regular row applied");
        assert_eq!(summary.provider, "moxfield", "the shape was sniffed as Moxfield");
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(summary.unmatched_cards, 1);
        assert_eq!(
            summary.unmatched_sample,
            vec!["Ghost Card (zzz #999)".to_string()],
            "unmatched rows are labelled by name + set + number, not an opaque key"
        );
    }

    #[tokio::test]
    async fn execute_csv_import_aggregates_duplicate_moxfield_rows_onto_one_card() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "moxfield-dupes@test.example").await;
        let a = insert_card_at(&db, "ext-tle-146", "tle", "146").await;

        // The same printing across three rows (differing condition/tags upstream): two
        // regular rows and a foil row must land on one holding.
        let csv = "Count,Name,Edition,Foil,Collector Number\n\
                   2,Aang,tle,,146\n\
                   1,Aang,tle,,146\n\
                   1,Aang,tle,foil,146\n";

        let summary = execute_csv_import(
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
    async fn execute_csv_import_refuses_an_empty_upload() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 3, 0).await;

        // A header-only CSV yields no holdings; a Replace against it must be refused so an
        // empty upload can't wipe the collection.
        let err = execute_csv_import(
            &db,
            user_id,
            crate::scryfall::GAME,
            ReconcileMode::Replace,
            b"Scryfall ID,Finish,Quantity\n",
        )
        .await
        .expect_err("empty upload must be refused");
        assert!(matches!(err, ImportError::EmptyCollection));
        assert_eq!(owned_counts(&db, user_id, a).await, Some((3, 0)), "collection untouched");
    }
}
