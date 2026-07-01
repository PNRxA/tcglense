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

/// Upper bound on how many copies a single card holding may carry after reconcile,
/// mirroring the collection handler's per-card clamp so an import can neither store a
/// pathological count nor overflow the valuation arithmetic.
const MAX_QUANTITY: i64 = 1_000_000;

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
    use super::reconcile::{Counts, aggregate, clamp_count, plan_reconcile};
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

    fn holding(id: &str, foil: bool, quantity: i32) -> FetchedHolding {
        FetchedHolding {
            external_card_id: id.to_string(),
            foil,
            quantity,
        }
    }

    fn counts(quantity: i64, foil_quantity: i64) -> Counts {
        Counts {
            quantity,
            foil_quantity,
        }
    }

    #[test]
    fn aggregate_sums_duplicate_rows_and_splits_foil() {
        // The same printing across three rows: two regular, one foil; plus another card.
        let holdings = vec![
            holding("a", false, 1),
            holding("a", false, 2),
            holding("a", true, 1),
            holding("b", true, 3),
        ];
        let agg = aggregate(&holdings);
        assert_eq!(agg[&"a".to_string()], counts(3, 1));
        assert_eq!(agg[&"b".to_string()], counts(0, 3));
        assert_eq!(agg.len(), 2);
    }

    #[test]
    fn aggregate_drops_cards_that_net_to_zero_copies() {
        // A negative/zero-only card contributes no real copies, so it's dropped entirely
        // (it must not later count as imported or trigger a delete).
        let agg = aggregate(&[holding("a", false, -5), holding("b", false, 2)]);
        assert!(!agg.contains_key("a"));
        assert_eq!(agg[&"b".to_string()], counts(2, 0));
    }

    #[test]
    fn overwrite_sets_matched_and_leaves_others() {
        let existing = HashMap::from([(1, (2, 0)), (9, (1, 1))]);
        let imported = HashMap::from([(1, counts(1, 0)), (2, counts(4, 0))]);
        let plan = plan_reconcile(&existing, &imported, ReconcileMode::Overwrite);
        let mut upserts = plan.upserts.clone();
        upserts.sort();
        assert_eq!(upserts, vec![(1, 1, 0), (2, 4, 0)]);
        assert!(plan.deletes.is_empty(), "overwrite never deletes");
    }

    #[test]
    fn merge_adds_onto_existing() {
        let existing = HashMap::from([(1, (2, 1))]);
        let imported = HashMap::from([(1, counts(1, 0)), (2, counts(3, 0))]);
        let plan = plan_reconcile(&existing, &imported, ReconcileMode::Merge);
        let mut upserts = plan.upserts.clone();
        upserts.sort();
        assert_eq!(upserts, vec![(1, 3, 1), (2, 3, 0)]);
        assert!(plan.deletes.is_empty(), "merge never deletes");
    }

    #[test]
    fn replace_sets_matched_and_deletes_unimported() {
        let existing = HashMap::from([(1, (2, 0)), (9, (1, 1))]);
        let imported = HashMap::from([(1, counts(1, 0)), (2, counts(4, 0))]);
        let plan = plan_reconcile(&existing, &imported, ReconcileMode::Replace);
        let mut upserts = plan.upserts.clone();
        upserts.sort();
        assert_eq!(upserts, vec![(1, 1, 0), (2, 4, 0)]);
        assert_eq!(plan.deletes, vec![9], "card 9 wasn't imported, so it's removed");
    }

    #[test]
    fn clamp_count_bounds_to_max() {
        assert_eq!(clamp_count(-1), 0);
        assert_eq!(clamp_count(5), 5);
        assert_eq!(clamp_count(MAX_QUANTITY + 10), MAX_QUANTITY as i32);
    }

    // ---- DB-backed reconcile tests (in-memory SQLite, no network) ----

    /// Insert a user (collection rows FK to `users`) and return its id.
    async fn insert_user(db: &DatabaseConnection) -> i32 {
        let now = Utc::now();
        crate::entities::user::ActiveModel {
            email: Set("importer@test.example".to_string()),
            password_hash: Set("x".to_string()),
            display_name: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert user")
        .id
    }

    /// Insert a minimal card and return its internal id.
    async fn insert_card(db: &DatabaseConnection, external_id: &str) -> i32 {
        let now = Utc::now();
        let card = card::ActiveModel {
            game: Set(crate::scryfall::GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Card {external_id}")),
            set_code: Set("tst".to_string()),
            set_name: Set("Test Set".to_string()),
            collector_number: Set("1".to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        card.insert(db).await.expect("insert card").id
    }

    async fn insert_holding(db: &DatabaseConnection, user_id: i32, card_id: i32, q: i32, f: i32) {
        let now = Utc::now();
        collection_item::ActiveModel {
            user_id: Set(user_id),
            game: Set(crate::scryfall::GAME.to_string()),
            card_id: Set(card_id),
            quantity: Set(q),
            foil_quantity: Set(f),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert holding");
    }

    async fn owned_counts(db: &DatabaseConnection, user_id: i32, card_id: i32) -> Option<(i32, i32)> {
        CollectionItem::find()
            .filter(collection_item::Column::UserId.eq(user_id))
            .filter(collection_item::Column::CardId.eq(card_id))
            .one(db)
            .await
            .expect("query holding")
            .map(|r| (r.quantity, r.foil_quantity))
    }

    #[tokio::test]
    async fn replace_mirrors_the_import_over_a_db() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await; // owned + imported -> updated
        let b = insert_card(&db, "ext-b").await; // not owned + imported -> inserted
        let c = insert_card(&db, "ext-c").await; // owned, not imported -> deleted (mirror)
        insert_holding(&db, user_id, a, 5, 0).await;
        insert_holding(&db, user_id, c, 2, 0).await;

        let holdings = vec![holding("ext-a", false, 1), holding("ext-b", true, 3), holding("ext-x", false, 9)];
        let summary = reconcile_holdings(&db, user_id, crate::scryfall::GAME, Provider::Archidekt, ReconcileMode::Replace, holdings)
            .await
            .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((1, 0)), "a overwritten to import");
        assert_eq!(owned_counts(&db, user_id, b).await, Some((0, 3)), "b inserted as foil");
        assert_eq!(owned_counts(&db, user_id, c).await, None, "c mirrored away");
        assert_eq!(summary.matched_cards, 2);
        assert_eq!(summary.unmatched_cards, 1, "ext-x isn't in the catalog");
        assert_eq!(summary.unmatched_sample, vec!["ext-x".to_string()]);
        assert_eq!(summary.removed_cards, 1);
        assert_eq!(summary.total_rows, 3);
        assert_eq!(summary.distinct_cards, 3);
    }

    #[tokio::test]
    async fn overwrite_leaves_unimported_owned_cards() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await;
        let c = insert_card(&db, "ext-c").await;
        insert_holding(&db, user_id, a, 5, 0).await;
        insert_holding(&db, user_id, c, 2, 1).await;

        let holdings = vec![holding("ext-a", false, 1)];
        let summary = reconcile_holdings(&db, user_id, crate::scryfall::GAME, Provider::Archidekt, ReconcileMode::Overwrite, holdings)
            .await
            .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((1, 0)));
        assert_eq!(owned_counts(&db, user_id, c).await, Some((2, 1)), "untouched by overwrite");
        assert_eq!(summary.removed_cards, 0);
    }

    #[tokio::test]
    async fn replace_with_no_matches_is_refused_and_keeps_collection() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 3, 0).await;

        // The provider returned cards, but none of them are in our catalog.
        let holdings = vec![holding("ext-unknown", false, 1)];
        let err = reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Replace,
            holdings,
        )
        .await
        .expect_err("replace with no matches must be refused");
        assert!(matches!(err, ImportError::NoMatchingCards));

        // The existing collection is untouched — nothing was wiped.
        assert_eq!(owned_counts(&db, user_id, a).await, Some((3, 0)));
    }

    #[tokio::test]
    async fn merge_adds_onto_owned_counts_over_a_db() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 2, 1).await;

        let holdings = vec![holding("ext-a", false, 3), holding("ext-a", true, 1)];
        reconcile_holdings(&db, user_id, crate::scryfall::GAME, Provider::Archidekt, ReconcileMode::Merge, holdings)
            .await
            .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((5, 2)), "2+3 regular, 1+1 foil");
    }

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
    async fn smart_preserves_unobserved_foil_and_never_deletes() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await; // owned reg+foil; only regular re-fetched
        let c = insert_card(&db, "ext-c").await; // owned, not fetched -> must remain
        insert_holding(&db, user_id, a, 2, 3).await;
        insert_holding(&db, user_id, c, 4, 0).await;

        // The smart fetch only paged a's regular row (its foil sat in the unscanned tail).
        let holdings = vec![holding("ext-a", false, 5)];
        let summary = reconcile_smart(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            holdings,
            true,
        )
        .await
        .expect("reconcile smart");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((5, 3)),
            "regular overwritten, unobserved foil preserved"
        );
        assert_eq!(
            owned_counts(&db, user_id, c).await,
            Some((4, 0)),
            "unfetched card kept (smart never deletes)"
        );
        assert_eq!(summary.matched_cards, 1);
        assert_eq!(summary.removed_cards, 0);
        assert!(summary.stopped_early);
    }

    #[tokio::test]
    async fn smart_inserts_a_newly_fetched_card() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let b = insert_card(&db, "ext-b").await; // not owned yet

        let holdings = vec![holding("ext-b", true, 2), holding("ext-b", false, 1)];
        reconcile_smart(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            holdings,
            false,
        )
        .await
        .expect("reconcile smart");

        assert_eq!(owned_counts(&db, user_id, b).await, Some((1, 2)));
    }

    #[tokio::test]
    async fn smart_preserve_reads_live_counts_not_a_stale_snapshot() {
        // Models a single-card edit landing while the (slow) smart fetch was running: the
        // regular count is now 2 in the DB. Smart re-fetched only this card's foil finish,
        // so it must overwrite foil but preserve the *current* regular (2), not revert it.
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 2, 5).await; // the post-edit live state

        let holdings = vec![holding("ext-a", true, 6)]; // foil observed, regular not
        reconcile_smart(&db, user_id, crate::scryfall::GAME, Provider::Archidekt, holdings, true)
            .await
            .expect("reconcile smart");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((2, 6)),
            "regular preserved from the live count, foil overwritten"
        );
    }

    #[tokio::test]
    async fn execute_csv_import_parses_then_reconciles_over_a_db() {
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
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
        let db = crate::test_support::migrated_memory_db().await;
        let user_id = insert_user(&db).await;
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
