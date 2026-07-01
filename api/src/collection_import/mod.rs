//! Provider-agnostic collection import / sync.
//!
//! Pulls a user's card collection from an external collection service and reconciles
//! it into the local per-user `collection_items`. Archidekt is the first provider;
//! Moxfield is planned, so the provider layer is a thin fetch/parse boundary — one
//! module per service, dispatched by the [`Provider`] enum (mirroring how `catalog`
//! dispatches per game). Everything downstream — aggregation, external-id resolution,
//! reconcile, apply — is provider-independent.
//!
//! A provider exposes each card by an id in the form our catalog stores as
//! `cards.external_id` (for Archidekt that is the Scryfall id, `card.uid`), so a fetched
//! holding maps straight onto a local card by a single indexed lookup. Cards with no
//! match in our catalog are reported as "unmatched" and skipped.

mod archidekt;
mod csv_import;
mod error;
pub mod jobs;
pub mod rate_limit;
mod reconcile;
mod types;

use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

pub use error::ImportError;
pub use types::*;
use reconcile::{reconcile_holdings, reconcile_smart};

/// Hard cap on how many holding rows we'll pull from a provider in one import, so a
/// request can't make us fan out an unbounded number of upstream page fetches.
const MAX_IMPORT_ROWS: usize = 100_000;

/// How many unmatched card ids we surface in the summary (a debugging aid — the full
/// count is always reported).
const UNMATCHED_SAMPLE_CAP: usize = 20;

/// SQLite caps host parameters per statement (as few as 999 on old builds), so any
/// `IN (...)` lookup/delete over the imported cards is batched into chunks comfortably
/// under that limit — a large collection can carry far more distinct cards than that.
const IN_CHUNK: usize = 900;

/// Parse the collection id from a user-supplied source (a full provider URL or a bare
/// id). Pure and provider-specific.
pub fn parse_source(provider: Provider, input: &str) -> Result<String, ImportError> {
    let parsed = match provider {
        Provider::Archidekt => archidekt::parse_collection_id(input),
    };
    parsed.ok_or_else(|| {
        ImportError::InvalidSource(format!(
            "couldn't read an {} collection id from '{}'",
            provider.label(),
            input.trim()
        ))
    })
}

/// Fetch every holding for a provider collection id, throttled by the shared provider
/// rate limiter.
async fn fetch_holdings(
    provider: Provider,
    http: &reqwest::Client,
    limiter: &rate_limit::RateLimiter,
    collection_id: &str,
) -> Result<Vec<FetchedHolding>, ImportError> {
    match provider {
        Provider::Archidekt => archidekt::fetch(http, limiter, collection_id).await,
    }
}

/// The recently-updated prefix of a provider collection for a smart sync: fetch
/// most-recently-updated first and stop once a whole page already matches `local`
/// (`external_card_id -> (quantity, foil_quantity)`). Returns the fetched holdings plus
/// whether we stopped early (reached the already-synced tail) rather than scanning the
/// whole collection.
async fn fetch_holdings_smart(
    provider: Provider,
    http: &reqwest::Client,
    limiter: &rate_limit::RateLimiter,
    collection_id: &str,
    local: &HashMap<String, (i32, i32)>,
) -> Result<(Vec<FetchedHolding>, bool), ImportError> {
    match provider {
        Provider::Archidekt => archidekt::fetch_smart(http, limiter, collection_id, local).await,
    }
}

/// Fold one provider page's normalized holdings (`(external_card_id, foil, quantity)`)
/// into a smart fetch's running state: append each to `holdings`, accumulate the
/// per-card running aggregate into `running` (`uid -> (regular, foil)`), and report
/// whether **every** card touched on this page now already equals its `local` count.
///
/// That "all match" flag is the smart stop signal: because the provider returns rows
/// most-recently-updated first, once a whole page is already in sync the rest of the
/// collection (updated even longer ago) is too, so paging can stop. The match is judged
/// only **after** the whole page is folded in, so a card that owns both a regular and a
/// foil finish isn't seen mid-aggregate just because its two rows sit on the same page.
/// A card still mid-aggregate — one row here, another on a later page — stays a mismatch
/// until its last row lands, which only *defers* the stop, never falsely triggers it, so
/// the signal is conservative. Pure (no I/O) so the decision is unit-tested without the
/// network.
fn smart_absorb_page(
    running: &mut HashMap<String, (i64, i64)>,
    holdings: &mut Vec<FetchedHolding>,
    local: &HashMap<String, (i32, i32)>,
    page_rows: impl IntoIterator<Item = (String, bool, i32)>,
) -> bool {
    let mut touched: Vec<String> = Vec::new();
    for (uid, foil, quantity) in page_rows {
        let entry = running.entry(uid.clone()).or_insert((0, 0));
        let q = i64::from(quantity.max(0));
        if foil {
            entry.1 += q;
        } else {
            entry.0 += q;
        }
        touched.push(uid.clone());
        holdings.push(FetchedHolding {
            external_card_id: uid,
            foil,
            quantity,
        });
    }
    // A card matches only once its full running aggregate equals the local counts; an
    // unowned card (no local entry) never matches, so a new card keeps paging.
    touched.iter().all(|uid| {
        running.get(uid).copied()
            == local.get(uid).map(|&(r, f)| (i64::from(r), i64::from(f)))
    })
}

/// Execute an import against an already-parsed provider collection id: fetch (throttled),
/// aggregate, resolve to local cards, reconcile per `mode`, and apply in one transaction.
/// Runs in the background worker (see [`jobs`]); the handler does the synchronous
/// validation (provider/game/source) before enqueuing, so this trusts its inputs.
#[allow(clippy::too_many_arguments)]
pub async fn execute_import(
    db: &DatabaseConnection,
    http: &reqwest::Client,
    limiter: &rate_limit::RateLimiter,
    user_id: i32,
    game: &str,
    provider: Provider,
    collection_id: &str,
    mode: ReconcileMode,
) -> Result<ImportSummary, ImportError> {
    if mode == ReconcileMode::Smart {
        // Smart needs the current collection up front to drive the early-stop (fetch
        // until a page already matches what we hold). Note this snapshot is only for the
        // stop decision — the reconcile re-reads current counts after the (minutes-long)
        // fetch, so a concurrent edit during the fetch isn't clobbered.
        let local = load_local_by_external(db, user_id, game).await?;
        let (holdings, stopped_early) =
            fetch_holdings_smart(provider, http, limiter, collection_id, &local).await?;
        if holdings.is_empty() {
            return Err(ImportError::EmptyCollection);
        }
        return reconcile_smart(db, user_id, game, provider, holdings, stopped_early).await;
    }

    let holdings = fetch_holdings(provider, http, limiter, collection_id).await?;
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
        let rows = Card::find()
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        for c in rows {
            external_by_id.insert(c.id, c.external_id);
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

/// Import a collection from an uploaded Archidekt CSV export.
///
/// Parses the (untrusted, already size-bounded) CSV bytes into normalized holdings, then
/// runs the exact same aggregate / resolve / reconcile / apply path as a network import —
/// but with no upstream fetch, so no rate limiter or background job is involved and this
/// runs inline in the request. A CSV has no persistent location, so it's always a
/// one-off; the caller picks the `mode`. An empty parse (no usable rows) is refused so a
/// `Replace` can't silently wipe the collection against a blank upload.
pub async fn execute_csv_import(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    mode: ReconcileMode,
    csv_bytes: &[u8],
) -> Result<ImportSummary, ImportError> {
    let holdings = csv_import::parse_archidekt_csv(csv_bytes)?;
    if holdings.is_empty() {
        return Err(ImportError::EmptyCollection);
    }
    // A CSV export is Archidekt data, so it reconciles as the Archidekt provider (the
    // summary's `provider` field, and the finish semantics, match the URL import).
    reconcile_holdings(db, user_id, game, Provider::Archidekt, mode, holdings).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        insert_card, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };

    // ---- Smart sync ----

    #[test]
    fn smart_absorb_page_reports_all_match_only_when_page_is_in_sync() {
        let local = HashMap::from([("a".to_string(), (2, 0)), ("b".to_string(), (1, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        // Every card on the page equals local (b spans a regular + a foil row).
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            vec![
                ("a".to_string(), false, 2),
                ("b".to_string(), false, 1),
                ("b".to_string(), true, 1),
            ],
        );
        assert!(all, "the page is fully in sync -> stop signal");
        assert_eq!(holdings.len(), 3, "every fetched row is still captured");
    }

    #[test]
    fn smart_absorb_page_flags_a_changed_or_new_card() {
        let local = HashMap::from([("a".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        // 'a' changed (3 != 2) and 'x' is unowned locally -> keep paging.
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            vec![("a".to_string(), false, 3), ("x".to_string(), false, 1)],
        );
        assert!(!all);
    }

    #[test]
    fn smart_absorb_page_defers_match_until_a_split_finish_settles() {
        // A card owned as regular + foil whose rows land on different pages: the first
        // page (regular only) reads as a mismatch because the running foil is still 0;
        // the second page (its foil row) settles the aggregate and matches.
        let local = HashMap::from([("a".to_string(), (2, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let page1 =
            smart_absorb_page(&mut running, &mut holdings, &local, vec![("a".to_string(), false, 2)]);
        assert!(!page1, "regular-only aggregate (2,0) != local (2,1)");
        let page2 =
            smart_absorb_page(&mut running, &mut holdings, &local, vec![("a".to_string(), true, 1)]);
        assert!(page2, "now (2,1) == local -> stop signal");
    }

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
