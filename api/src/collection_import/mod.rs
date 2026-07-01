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
pub mod jobs;
pub mod rate_limit;

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};
use crate::error::AppError;

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

/// A collection provider we can import from. One variant per external service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Archidekt,
}

impl Provider {
    /// The provider's stable string id — as it appears in the API and in stored
    /// `collection_sources` rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Archidekt => "archidekt",
        }
    }

    /// Parse a provider id case-insensitively. `None` for an unknown provider.
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "archidekt" => Some(Provider::Archidekt),
            _ => None,
        }
    }

    /// Human-readable provider name for UI / error copy.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Archidekt => "Archidekt",
        }
    }

    /// Whether this provider can supply a collection for `game`. Archidekt is
    /// Magic-only (its card ids are Scryfall ids).
    pub fn supports_game(self, game: &str) -> bool {
        match self {
            Provider::Archidekt => game == crate::scryfall::GAME,
        }
    }

    /// A canonical, user-facing URL for a collection id on this provider (for linking
    /// back from the UI). `id` is a validated provider collection id.
    pub fn collection_url(self, id: &str) -> String {
        match self {
            Provider::Archidekt => format!("https://archidekt.com/collection/v2/{id}"),
        }
    }
}

/// How an import reconciles with the user's existing collection for the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReconcileMode {
    /// Set each imported card to the imported counts; leave cards not in the import
    /// untouched. Idempotent and non-destructive.
    Overwrite,
    /// Make the collection exactly mirror the import: set imported cards and delete
    /// owned cards that aren't in the import.
    Replace,
    /// Add the imported counts on top of the existing counts.
    Merge,
}

/// One card holding pulled from a provider, before aggregation. `external_card_id` is
/// the provider's card id in the form our catalog stores (`cards.external_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedHolding {
    pub external_card_id: String,
    pub foil: bool,
    pub quantity: i32,
}

/// Regular + foil copies owned of a single card. Held as `i64` during aggregation so a
/// pathological provider payload can't overflow before the final clamp.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    quantity: i64,
    foil_quantity: i64,
}

/// The outcome of an import, surfaced to the user.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub provider: &'static str,
    pub mode: ReconcileMode,
    /// Total holding rows fetched from the provider (before aggregation by card).
    pub total_rows: usize,
    /// Distinct cards in the provider collection (after aggregating rows by card).
    pub distinct_cards: usize,
    /// Distinct cards that matched a card in our catalog and were applied.
    pub matched_cards: usize,
    /// Distinct cards with no match in our catalog (skipped).
    pub unmatched_cards: usize,
    /// A capped sample of unmatched card ids, for user feedback / debugging.
    pub unmatched_sample: Vec<String>,
    /// Regular copies the provider reported across all matched cards.
    pub regular_copies: i64,
    /// Foil copies the provider reported across all matched cards.
    pub foil_copies: i64,
    /// Owned cards removed by the reconcile (non-zero only in `Replace` mode).
    pub removed_cards: usize,
}

/// A failure while importing a collection. Converts to the right `AppError` (and thus
/// HTTP status + JSON body) via the `From` impl below.
#[derive(Debug)]
pub enum ImportError {
    /// The source string couldn't be parsed into a collection id -> 422.
    InvalidSource(String),
    /// The provider has no public collection at that id -> 404.
    CollectionNotFound(String),
    /// The provider collection has no cards to import -> 422 (guards a `Replace` from
    /// silently wiping the user's collection against an empty/misresolved source).
    EmptyCollection,
    /// A `Replace` matched none of our catalog -> 422. Guards against wiping the whole
    /// collection when the source's cards simply aren't in our catalog (e.g. the
    /// catalog hasn't been synced), rather than deleting everything and importing nothing.
    NoMatchingCards,
    /// The collection is larger than we'll import in one request -> 422.
    TooLarge { count: usize, max: usize },
    /// The provider request or response parse failed -> 502.
    Upstream(String),
    /// A local database error -> 500.
    Db(sea_orm::DbErr),
}

impl From<ImportError> for AppError {
    fn from(err: ImportError) -> Self {
        match err {
            ImportError::InvalidSource(msg) => AppError::Validation(msg),
            ImportError::CollectionNotFound(id) => {
                AppError::NotFound(format!("no public collection found for '{id}'"))
            }
            ImportError::EmptyCollection => {
                AppError::Validation("the collection has no cards to import".to_string())
            }
            ImportError::NoMatchingCards => AppError::Validation(
                "none of the collection's cards are in our catalog, so there was nothing to \
                 import (your collection was left unchanged)"
                    .to_string(),
            ),
            ImportError::TooLarge { count, max } => AppError::Validation(format!(
                "collection is too large to import ({count} cards; the limit is {max})"
            )),
            ImportError::Upstream(detail) => {
                // Log the upstream detail server-side; return a generic gateway error.
                tracing::warn!(error = %detail, "collection provider request failed");
                AppError::BadGateway("the collection provider could not be reached".to_string())
            }
            ImportError::Db(err) => AppError::from(err),
        }
    }
}

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
    let holdings = fetch_holdings(provider, http, limiter, collection_id).await?;
    if holdings.is_empty() {
        return Err(ImportError::EmptyCollection);
    }
    reconcile_holdings(db, user_id, game, provider, mode, holdings).await
}

/// Aggregate raw holdings into per-card regular/foil counts. The same printing can
/// appear across several provider rows (differing condition/language/tags), so counts
/// are summed; the row `foil` flag splits regular vs foil.
fn aggregate(holdings: &[FetchedHolding]) -> HashMap<String, Counts> {
    let mut map: HashMap<String, Counts> = HashMap::new();
    for h in holdings {
        let entry = map.entry(h.external_card_id.clone()).or_default();
        let q = i64::from(h.quantity.max(0));
        if h.foil {
            entry.foil_quantity += q;
        } else {
            entry.quantity += q;
        }
    }
    // Drop phantom cards that net to zero copies (e.g. a stray zero-quantity row) so
    // they neither count as "imported" nor cause a spurious delete.
    map.retain(|_, c| c.quantity > 0 || c.foil_quantity > 0);
    map
}

/// The DB operations a reconcile resolves to, keyed by internal `cards.id`.
#[derive(Debug, Default, PartialEq, Eq)]
struct ReconcilePlan {
    /// `(card_id, quantity, foil_quantity)` to set (both zero is treated as a delete).
    upserts: Vec<(i32, i32, i32)>,
    /// Card ids whose holding row should be removed.
    deletes: Vec<i32>,
}

fn clamp_count(value: i64) -> i32 {
    value.clamp(0, MAX_QUANTITY) as i32
}

/// Decide the reconcile operations from the current holdings and the imported ones,
/// per `mode`. Pure: `existing`/`imported` are `card_id -> counts`, no DB access.
fn plan_reconcile(
    existing: &HashMap<i32, (i32, i32)>,
    imported: &HashMap<i32, Counts>,
    mode: ReconcileMode,
) -> ReconcilePlan {
    let mut plan = ReconcilePlan::default();
    match mode {
        ReconcileMode::Overwrite => {
            for (&card_id, counts) in imported {
                plan.upserts.push((
                    card_id,
                    clamp_count(counts.quantity),
                    clamp_count(counts.foil_quantity),
                ));
            }
        }
        ReconcileMode::Merge => {
            for (&card_id, counts) in imported {
                let (eq, ef) = existing.get(&card_id).copied().unwrap_or((0, 0));
                plan.upserts.push((
                    card_id,
                    clamp_count(counts.quantity + i64::from(eq)),
                    clamp_count(counts.foil_quantity + i64::from(ef)),
                ));
            }
        }
        ReconcileMode::Replace => {
            for (&card_id, counts) in imported {
                plan.upserts.push((
                    card_id,
                    clamp_count(counts.quantity),
                    clamp_count(counts.foil_quantity),
                ));
            }
            // Mirror: drop owned cards that aren't in the imported set.
            for &card_id in existing.keys() {
                if !imported.contains_key(&card_id) {
                    plan.deletes.push(card_id);
                }
            }
        }
    }
    plan
}

/// Resolve the aggregated holdings to local cards, reconcile against the user's current
/// collection, and apply. Split from [`run_import`] so it can be unit-tested against an
/// in-memory DB without any provider network calls.
async fn reconcile_holdings(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    provider: Provider,
    mode: ReconcileMode,
    holdings: Vec<FetchedHolding>,
) -> Result<ImportSummary, ImportError> {
    let total_rows = holdings.len();
    let aggregated = aggregate(&holdings);
    let distinct_cards = aggregated.len();

    // Resolve external ids -> internal card ids (indexed on (game, external_id)),
    // chunked so a very large collection can't blow past SQLite's per-statement
    // bind-variable limit.
    let external_ids: Vec<String> = aggregated.keys().cloned().collect();
    let mut matched: HashMap<String, i32> = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows = Card::find()
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::ExternalId.is_in(chunk.iter().cloned()))
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        for c in rows {
            matched.insert(c.external_id, c.id);
        }
    }

    // Partition into matched (keyed by internal id) and unmatched.
    let mut imported: HashMap<i32, Counts> = HashMap::new();
    let mut unmatched_sample: Vec<String> = Vec::new();
    let mut unmatched_cards = 0usize;
    let mut regular_copies = 0i64;
    let mut foil_copies = 0i64;
    for (ext_id, counts) in &aggregated {
        match matched.get(ext_id) {
            Some(&card_id) => {
                imported.insert(card_id, *counts);
                regular_copies += counts.quantity;
                foil_copies += counts.foil_quantity;
            }
            None => {
                unmatched_cards += 1;
                if unmatched_sample.len() < UNMATCHED_SAMPLE_CAP {
                    unmatched_sample.push(ext_id.clone());
                }
            }
        }
    }
    let matched_cards = imported.len();

    // A mirror/replace that matched nothing is almost always a catalog mismatch, not an
    // intent to wipe the collection — refuse before we delete everything.
    if imported.is_empty() && mode == ReconcileMode::Replace {
        return Err(ImportError::NoMatchingCards);
    }

    // Current holdings for (user, game): the counts feed `merge` planning (the apply
    // step upserts/deletes by key, so it doesn't need the row ids).
    let existing_counts: HashMap<i32, (i32, i32)> = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .all(db)
        .await
        .map_err(ImportError::Db)?
        .into_iter()
        .map(|r| (r.card_id, (r.quantity, r.foil_quantity)))
        .collect();

    let plan = plan_reconcile(&existing_counts, &imported, mode);
    let removed_cards = plan.deletes.len();

    apply_plan(db, user_id, game, plan)
        .await
        .map_err(ImportError::Db)?;

    Ok(ImportSummary {
        provider: provider.as_str(),
        mode,
        total_rows,
        distinct_cards,
        matched_cards,
        unmatched_cards,
        unmatched_sample,
        regular_copies,
        foil_copies,
        removed_cards,
    })
}

/// Apply a reconcile plan in a single transaction: upsert each holding (both-zero
/// deletes) and remove the planned deletions. All-or-nothing so a mid-import failure
/// can't leave a half-synced collection.
///
/// Upserts go through `INSERT ... ON CONFLICT DO UPDATE` on the unique
/// `(user, game, card)` index, so a concurrent writer (another sync, or a single-card
/// `PUT`) inserting the same holding can't abort the whole import on a unique violation
/// — the row is created or updated atomically either way.
async fn apply_plan(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    plan: ReconcilePlan,
) -> Result<(), sea_orm::DbErr> {
    let txn = db.begin().await?;
    let now = Utc::now();

    for (card_id, quantity, foil_quantity) in plan.upserts {
        // Owning zero of both is "not in the collection": drop the row if present.
        // (Aggregation drops all-zero cards, so this is defensive.)
        if quantity == 0 && foil_quantity == 0 {
            delete_holding(&txn, user_id, game, &[card_id]).await?;
            continue;
        }
        let active = collection_item::ActiveModel {
            user_id: Set(user_id),
            game: Set(game.to_string()),
            card_id: Set(card_id),
            quantity: Set(quantity),
            foil_quantity: Set(foil_quantity),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        CollectionItem::insert(active)
            .on_conflict(
                OnConflict::columns([
                    collection_item::Column::UserId,
                    collection_item::Column::Game,
                    collection_item::Column::CardId,
                ])
                .update_columns([
                    collection_item::Column::Quantity,
                    collection_item::Column::FoilQuantity,
                    collection_item::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec(&txn)
            .await?;
    }

    // Mirror deletions, chunked to stay under SQLite's bind-variable limit.
    for chunk in plan.deletes.chunks(IN_CHUNK) {
        delete_holding(&txn, user_id, game, chunk).await?;
    }

    txn.commit().await
}

/// Delete the given cards' holdings for a user in a game (by key, no row-id lookup).
async fn delete_holding<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: i32,
    game: &str,
    card_ids: &[i32],
) -> Result<(), sea_orm::DbErr> {
    if card_ids.is_empty() {
        return Ok(());
    }
    CollectionItem::delete_many()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(collection_item::Column::CardId.is_in(card_ids.iter().copied()))
        .exec(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
