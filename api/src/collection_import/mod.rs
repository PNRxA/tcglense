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

mod archidekt;
mod consolidate;
mod csv_import;
mod error;
pub mod jobs;
mod moxfield;
mod progress;
pub mod rate_limit;
mod reconcile;
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
///
/// The stop needs more than the current page, though: the provider can split one printing
/// across rows (differing condition/language/tags) whose `updatedAt`s put them on
/// non-adjacent pages, so a card can be mid-aggregate from an earlier page yet absent from
/// the page that looks in sync. Stopping there would strand its remaining rows and let the
/// reconcile overwrite it *down* to the partial count (silently dropping copies). So the
/// stop also requires that **no** card in the running aggregate is still below its local
/// count in either finish — a straddling (or genuinely decreased) card keeps paging until
/// its rows all land. Pure (no I/O) so the decision is unit-tested without the network.
///
/// `remap` folds a separately-modelled foil printing (`…★`) onto its base card as a foil
/// copy (issue #209) **before** aggregating, so the running aggregate, the accumulated
/// holdings, and the early-stop comparison all speak the base external id — and so the
/// holdings this returns are already consolidated for the reconcile.
fn smart_absorb_page(
    running: &mut HashMap<String, (i64, i64)>,
    holdings: &mut Vec<FetchedHolding>,
    local: &HashMap<String, (i32, i32)>,
    remap: &HashMap<String, String>,
    page_rows: impl IntoIterator<Item = (String, bool, i32)>,
) -> bool {
    let mut touched: Vec<String> = Vec::new();
    for (uid, foil, quantity) in page_rows {
        // Fold a foil-★ variant onto its base as foil before it enters the aggregate.
        let (uid, foil) = match remap.get(&uid) {
            Some(base) => (base.clone(), true),
            None => (uid, foil),
        };
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
    // unowned card (no local entry) never matches, so a new card keeps paging. A page
    // that contributed NO rows (e.g. Moxfield's fetch skips proxies / id-less custom
    // cards, so a bulk-edited block of proxies can fill a whole page) proves nothing
    // about sync state — vacuous truth here would falsely stop the fetch, so an empty
    // contribution reads as "keep paging".
    let page_in_sync = !touched.is_empty()
        && touched.iter().all(|uid| {
            running.get(uid).copied()
                == local.get(uid).map(|&(r, f)| (i64::from(r), i64::from(f)))
        });
    // ...but the per-page check above can't see a card whose rows straddle a page boundary:
    // seen (mid-aggregate, under-counted) on an earlier page, absent from this one. Stopping
    // while any card in the whole running aggregate is still *below* its local count would
    // strand that card's remaining rows and let reconcile_smart overwrite its finish down to
    // the partial count. Gate the stop on that too (checked only once the cheap per-page test
    // passes). A new card (local read as (0,0)) can't be below, so it never holds paging open.
    page_in_sync
        && running.iter().all(|(uid, &(reg, foil))| {
            let (lr, lf) = local
                .get(uid)
                .map_or((0, 0), |&(r, f)| (i64::from(r), i64::from(f)));
            reg >= lr && foil >= lf
        })
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
async fn moxfield_rows_to_holdings(
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
            &HashMap::new(),
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
            &HashMap::new(),
            vec![("a".to_string(), false, 3), ("x".to_string(), false, 1)],
        );
        assert!(!all);
    }

    #[test]
    fn smart_absorb_page_treats_an_empty_contribution_as_keep_paging() {
        // A page whose rows were ALL filtered out upstream (e.g. Moxfield proxies /
        // id-less custom cards) contributes nothing — it must not read as "in sync"
        // (vacuous truth), or a bulk-edited block of proxies at the front of a smart
        // fetch would falsely stop it (or empty the whole fetch into an
        // EmptyCollection error for a non-empty collection).
        let local = HashMap::from([("a".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let all = smart_absorb_page(&mut running, &mut holdings, &local, &HashMap::new(), Vec::new());
        assert!(!all, "an empty page contribution must keep paging");
        assert!(holdings.is_empty());
    }

    #[test]
    fn smart_absorb_page_defers_match_until_a_split_finish_settles() {
        // A card owned as regular + foil whose rows land on different pages: the first
        // page (regular only) reads as a mismatch because the running foil is still 0;
        // the second page (its foil row) settles the aggregate and matches.
        let local = HashMap::from([("a".to_string(), (2, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 2)],
        );
        assert!(!page1, "regular-only aggregate (2,0) != local (2,1)");
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), true, 1)],
        );
        assert!(page2, "now (2,1) == local -> stop signal");
    }

    #[test]
    fn smart_absorb_page_folds_a_foil_star_onto_its_base_and_matches_local() {
        // A held foil-★ (issue #209): the local snapshot has been consolidated to the base
        // as foil, and the re-fetched star row — even reported as a non-foil finish — folds
        // onto the same base, so the page reads as in sync.
        let remap = HashMap::from([("star".to_string(), "base".to_string())]);
        let local = HashMap::from([("base".to_string(), (0, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &remap,
            vec![("star".to_string(), false, 1)],
        );
        assert!(all, "the folded star matches the consolidated local -> stop signal");
        assert_eq!(
            holdings,
            vec![FetchedHolding {
                external_card_id: "base".to_string(),
                foil: true,
                quantity: 1,
            }],
            "the captured holding is already remapped to the base as foil"
        );
    }

    #[test]
    fn smart_absorb_page_keeps_paging_while_an_earlier_card_is_still_below_local() {
        // Regression: a card whose provider rows straddle a page boundary must not be
        // stranded. `a` is owned as 2 regular but only one of its rows is on page 1; page 2's
        // own card (`b`) is fully in sync but `a` is absent from it. The old per-page-only
        // check stopped on page 2 and abandoned `a`'s second row, so reconcile then overwrote
        // `a` down to 1 (losing a copy). The stop must now be withheld while `a` is still
        // below its local count.
        let local = HashMap::from([("a".to_string(), (2, 0)), ("b".to_string(), (1, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();

        // Page 1: one of a's two regular rows -> a is mid-aggregate (1 of 2).
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 1)],
        );
        assert!(!page1, "a under-counted (1 != 2) -> keep paging");

        // Page 2: b is fully in sync and a is ABSENT — but a is still below local, so the
        // fetch must keep paging rather than strand a's remaining row.
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("b".to_string(), false, 1)],
        );
        assert!(!page2, "a still below its local count -> must not stop and strand its tail row");

        // Page 3: a's second row lands; now the whole aggregate matches local -> stop.
        let page3 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 1)],
        );
        assert!(page3, "a now fully aggregated (2,0) and everything matches -> stop");
    }

    #[test]
    fn smart_absorb_page_still_stops_when_a_card_grew_above_local() {
        // The below-local gate uses `>=`, not `==`: a card whose upstream count GREW (running
        // above its old local) is fully observed on the front pages, so it must NOT hold the
        // fetch open — otherwise any sync with a pending increase would degrade to a full scan.
        let local = HashMap::from([("grew".to_string(), (1, 0)), ("same".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();

        // Page 1: the grown card (now 3 regular). Above local, so this page isn't "in sync".
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("grew".to_string(), false, 3)],
        );
        assert!(!page1, "grew (3,0) != local (1,0) on its own page -> keep paging");

        // Page 2: an unchanged card, in sync. `grew` is above (not below) local, so it must
        // not block the stop.
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("same".to_string(), false, 2)],
        );
        assert!(page2, "no card is below local (grew is above) -> stop signal fires");
    }

    #[tokio::test]
    async fn smart_sync_does_not_drop_a_copy_when_a_card_straddles_the_stop_page() {
        // End-to-end regression for the reported bug (an import totalling N copies dropped to
        // N-1 on the next smart sync). Card `a` is owned as 2 regular copies the provider
        // reports as two SEPARATE regular rows (e.g. two conditions) whose `updatedAt`s put
        // them on non-adjacent pages, with a fully-in-sync `b` page in between. Driving the
        // same page loop `fetch_smart` runs, the middle page must not stop the fetch and
        // strand a's second row — which reconcile_smart would then overwrite a down to 1.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "straddle@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        let b = insert_card(&db, "ext-b").await;
        insert_holding(&db, user_id, a, 2, 0).await; // owned: 2 regular
        insert_holding(&db, user_id, b, 1, 0).await; // owned: 1 regular

        let local = HashMap::from([("ext-a".to_string(), (2, 0)), ("ext-b".to_string(), (1, 0))]);
        let remap: HashMap<String, String> = HashMap::new();

        let pages: Vec<Vec<(String, bool, i32)>> = vec![
            vec![("ext-a".to_string(), false, 1)], // page 1: one of a's two rows
            vec![("ext-b".to_string(), false, 1)], // page 2: b in sync, a absent
            vec![("ext-a".to_string(), false, 1)], // page 3: a's second row
        ];
        let mut running: HashMap<String, (i64, i64)> = HashMap::new();
        let mut holdings: Vec<FetchedHolding> = Vec::new();
        let mut stopped_early = false;
        for page in pages {
            if smart_absorb_page(&mut running, &mut holdings, &local, &remap, page) {
                stopped_early = true;
                break;
            }
        }

        // The middle page must not have stopped the fetch, so both of a's rows were absorbed.
        assert!(stopped_early, "still stops early once a is fully aggregated on page 3");
        assert_eq!(running["ext-a"], (2, 0), "a fully aggregated, not stranded at 1");

        reconcile_smart(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            holdings,
            stopped_early,
        )
        .await
        .expect("reconcile smart");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((2, 0)),
            "no copy lost from the straddling card"
        );
        assert_eq!(
            owned_counts(&db, user_id, b).await,
            Some((1, 0)),
            "the in-sync card is unchanged"
        );
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
