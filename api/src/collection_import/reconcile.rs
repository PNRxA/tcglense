//! Provider-independent reconcile engine: aggregate fetched holdings, resolve them to
//! local cards, plan the DB operations per [`ReconcileMode`], and apply them atomically.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

use super::consolidate;
use super::{FetchedHolding, ImportError, ImportSummary, Provider, ReconcileMode};
use super::{IN_CHUNK, UNMATCHED_SAMPLE_CAP};

/// Regular + foil copies owned of a single card. Held as `i64` during aggregation so a
/// pathological provider payload can't overflow before the final clamp.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    quantity: i64,
    foil_quantity: i64,
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
    value.clamp(0, i64::from(collection_item::MAX_CARD_QUANTITY)) as i32
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
        // Smart is reconciled by `reconcile_smart` (preserve unobserved finishes, never
        // delete), not here — but at the plan level it's the same overwrite-without-delete
        // as `Overwrite`, so share the arm rather than leave it unreachable.
        ReconcileMode::Overwrite | ReconcileMode::Smart => {
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

/// Resolve external card ids -> internal `cards.id` for one game, chunked so a very large
/// collection can't blow past SQLite's per-statement bind-variable limit. Ids with no
/// catalog match are simply absent from the returned map. Shared by both reconcile paths.
async fn resolve_card_ids(
    db: &DatabaseConnection,
    game: &str,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, ImportError> {
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
    Ok(matched)
}

/// Current owned counts for (user, game), keyed by internal `cards.id` (regular, foil).
/// Feeds reconcile planning; the apply step upserts/deletes by key, so it doesn't need the
/// row ids. Read after the fetch so a concurrent single-card edit isn't reverted. Shared by
/// both reconcile paths. (Distinct from `load_local_by_external`, which keys by external id
/// and also loads the Card row.)
async fn existing_counts_by_card(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<HashMap<i32, (i32, i32)>, ImportError> {
    Ok(CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .all(db)
        .await
        .map_err(ImportError::Db)?
        .into_iter()
        .map(|r| (r.card_id, (r.quantity, r.foil_quantity)))
        .collect())
}

/// Resolve the aggregated holdings to local cards, reconcile against the user's current
/// collection, and apply. Split from [`execute_import`] so it can be unit-tested against
/// an in-memory DB without any provider network calls.
pub(super) async fn reconcile_holdings(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    provider: Provider,
    mode: ReconcileMode,
    holdings: Vec<FetchedHolding>,
) -> Result<ImportSummary, ImportError> {
    // Fold separately-modelled foil printings (`…★`) onto their base card as foil copies
    // (issue #209), so aggregation, id resolution, and reconcile all see a straight 1:1
    // external-id→card mapping. Remap the incoming holdings up front (pure, no DB); the
    // existing-star fold is a DB mutation, so it's deferred until *after* the zero-match
    // guard below so a refused import leaves the collection untouched. Covers the full
    // network import and the CSV upload (both land here); the smart path does the same
    // during its fetch.
    let pairs = consolidate::load_foil_variant_pairs(db, game).await?;
    let holdings = consolidate::apply_foil_remap(holdings, &consolidate::ext_remap(&pairs));

    let total_rows = holdings.len();
    let aggregated = aggregate(&holdings);
    let distinct_cards = aggregated.len();

    // Resolve external ids -> internal card ids (indexed on (game, external_id)).
    let external_ids: Vec<String> = aggregated.keys().cloned().collect();
    let matched = resolve_card_ids(db, game, &external_ids).await?;

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
    // intent to wipe the collection — refuse before we delete (or fold) anything.
    if imported.is_empty() && mode == ReconcileMode::Replace {
        return Err(ImportError::NoMatchingCards);
    }

    // Fold any star row the user already holds (a manual add of the `…★` card, or a legacy
    // pre-#209 import) onto its base first, so it can't coexist with — and double-count
    // against — the base holding this import writes. Runs after the guard (so a refused
    // import doesn't mutate) and before the current-counts read below (so `merge`/`replace`
    // plan against the folded state).
    consolidate::fold_existing_star_holdings(db, user_id, game, &pairs).await?;

    // Current holdings for (user, game): the counts feed `merge` planning.
    let existing_counts = existing_counts_by_card(db, user_id, game).await?;

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
        // A full fetch/reconcile always scans the whole collection.
        stopped_early: false,
    })
}

/// Reconcile a **smart** sync's fetched prefix: overwrite each fetched card's *observed*
/// finishes (regular and/or foil) to the fetched counts, but preserve any finish we
/// didn't fetch (its rows sit in the unscanned tail, so its stored count still stands),
/// and never delete. This is what makes the early-stop safe: stopping before the whole
/// collection is seen can't zero a foil we simply didn't page to, nor drop an untouched
/// card. Resolving unfetched-upstream deletions needs a full [`ReconcileMode::Replace`].
///
/// The unobserved-finish preserve reads the collection's **current** counts here (after
/// the fetch), not the pre-fetch snapshot, so a single-card edit made while the
/// minutes-long fetch was running isn't reverted (same read-then-apply window as the
/// full modes).
pub(super) async fn reconcile_smart(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    provider: Provider,
    holdings: Vec<FetchedHolding>,
    stopped_early: bool,
) -> Result<ImportSummary, ImportError> {
    let total_rows = holdings.len();

    // Which finishes we actually saw a row for, per external id — a finish we never saw
    // must be preserved from the current holding rather than overwritten to zero.
    let mut observed: HashMap<String, (bool, bool)> = HashMap::new();
    for h in &holdings {
        let seen = observed.entry(h.external_card_id.clone()).or_insert((false, false));
        if h.foil {
            seen.1 = true;
        } else {
            seen.0 = true;
        }
    }

    let aggregated = aggregate(&holdings);
    let distinct_cards = aggregated.len();

    // Resolve external ids -> internal card ids (chunked under SQLite's bind limit).
    let external_ids: Vec<String> = aggregated.keys().cloned().collect();
    let matched = resolve_card_ids(db, game, &external_ids).await?;

    // Current counts (read now, after the fetch) so preserving an unobserved finish uses
    // the live value — a concurrent single-card edit during the fetch isn't reverted.
    let existing_counts = existing_counts_by_card(db, user_id, game).await?;

    let mut upserts: Vec<(i32, i32, i32)> = Vec::new();
    let mut unmatched_sample: Vec<String> = Vec::new();
    let mut unmatched_cards = 0usize;
    let mut regular_copies = 0i64;
    let mut foil_copies = 0i64;
    for (ext_id, counts) in &aggregated {
        let Some(&card_id) = matched.get(ext_id) else {
            unmatched_cards += 1;
            if unmatched_sample.len() < UNMATCHED_SAMPLE_CAP {
                unmatched_sample.push(ext_id.clone());
            }
            continue;
        };
        // Preserve any finish we didn't fetch (its rows are in the unscanned tail).
        let (seen_reg, seen_foil) = observed.get(ext_id).copied().unwrap_or((false, false));
        let (cur_reg, cur_foil) = existing_counts.get(&card_id).copied().unwrap_or((0, 0));
        let new_reg = if seen_reg {
            clamp_count(counts.quantity)
        } else {
            cur_reg
        };
        let new_foil = if seen_foil {
            clamp_count(counts.foil_quantity)
        } else {
            cur_foil
        };
        regular_copies += i64::from(new_reg);
        foil_copies += i64::from(new_foil);
        upserts.push((card_id, new_reg, new_foil));
    }
    let matched_cards = upserts.len();

    apply_plan(
        db,
        user_id,
        game,
        ReconcilePlan {
            upserts,
            deletes: Vec::new(),
        },
    )
    .await
    .map_err(ImportError::Db)?;

    Ok(ImportSummary {
        provider: provider.as_str(),
        mode: ReconcileMode::Smart,
        total_rows,
        distinct_cards,
        matched_cards,
        unmatched_cards,
        unmatched_sample,
        regular_copies,
        foil_copies,
        // Smart never mirrors deletions.
        removed_cards: 0,
        stopped_early,
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
    use crate::test_support::{
        insert_card, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };

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
        assert_eq!(
            clamp_count(i64::from(collection_item::MAX_CARD_QUANTITY) + 10),
            collection_item::MAX_CARD_QUANTITY
        );
    }

    // ---- DB-backed reconcile tests (in-memory SQLite, no network) ----

    #[tokio::test]
    async fn replace_mirrors_the_import_over_a_db() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        insert_holding(&db, user_id, a, 2, 1).await;

        let holdings = vec![holding("ext-a", false, 3), holding("ext-a", true, 1)];
        reconcile_holdings(&db, user_id, crate::scryfall::GAME, Provider::Archidekt, ReconcileMode::Merge, holdings)
            .await
            .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, a).await, Some((5, 2)), "2+3 regular, 1+1 foil");
    }

    // ---- Smart sync ----

    #[tokio::test]
    async fn smart_preserves_unobserved_foil_and_never_deletes() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
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

    // ---- Foil-variant consolidation (issue #209) ----

    /// Insert a card with explicit set/number/finishes/oracle id — the fields the
    /// foil-★ consolidation keys off — reusing the canonical all-defaults row.
    async fn insert_variant(
        db: &DatabaseConnection,
        id: i32,
        external_id: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
    ) -> i32 {
        use crate::test_support::card_model;
        use sea_orm::{ActiveModelTrait, IntoActiveModel};
        card::Model {
            external_id: external_id.into(),
            set_code: "sld".into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle_id.into()),
            ..card_model(id)
        }
        .into_active_model()
        .insert(db)
        .await
        .expect("insert card");
        id
    }

    #[tokio::test]
    async fn reconcile_folds_a_foil_star_holding_onto_its_base_as_foil() {
        // The issue's exact case: importing the foil `741★` printing (even reported as a
        // non-foil finish, as Archidekt does) lands on the base `741` as a foil copy — no
        // separate `741★` holding is created.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let base = insert_variant(&db, 1, "ext-741", "741", "nonfoil", "ora-chaos").await;
        let star = insert_variant(&db, 2, "ext-741-star", "741★", "foil", "ora-chaos").await;

        let holdings = vec![holding("ext-741-star", false, 3)];
        let summary = reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Overwrite,
            holdings,
        )
        .await
        .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, base).await, Some((0, 3)), "base owns 3 foil");
        assert_eq!(owned_counts(&db, user_id, star).await, None, "no separate star holding");
        assert_eq!(summary.matched_cards, 1);
        assert_eq!(summary.foil_copies, 3);
        assert_eq!(summary.regular_copies, 0);
    }

    #[tokio::test]
    async fn reconcile_merges_base_and_star_rows_onto_one_card() {
        // Owning both the nonfoil `741` and the foil `741★` collapses to a single base
        // holding carrying both counts.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let base = insert_variant(&db, 1, "ext-741", "741", "nonfoil", "ora-chaos").await;
        insert_variant(&db, 2, "ext-741-star", "741★", "foil", "ora-chaos").await;

        // The base reported as two regular rows, plus the foil star.
        let holdings = vec![
            holding("ext-741", false, 1),
            holding("ext-741", false, 1),
            holding("ext-741-star", true, 2),
        ];
        let summary = reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Overwrite,
            holdings,
        )
        .await
        .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, base).await, Some((2, 2)), "2 regular + 2 foil");
        assert_eq!(summary.distinct_cards, 1, "star + base count as one card");
        assert_eq!(summary.matched_cards, 1);
    }

    #[tokio::test]
    async fn reconcile_leaves_an_ambiguous_star_as_its_own_card() {
        // A foil star whose base is itself foilable (`nonfoil,foil`) is left separate — it
        // is a genuinely distinct printing, not a nonfoil card's foil counterpart.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        insert_variant(&db, 1, "ext-33", "33", "nonfoil,foil", "ora-proctor").await;
        let star = insert_variant(&db, 2, "ext-33-star", "33★", "foil", "ora-proctor").await;

        let holdings = vec![holding("ext-33-star", true, 1)];
        reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Overwrite,
            holdings,
        )
        .await
        .expect("reconcile");

        assert_eq!(
            owned_counts(&db, user_id, star).await,
            Some((0, 1)),
            "the ambiguous star keeps its own holding"
        );
    }

    #[tokio::test]
    async fn reconcile_folds_a_pre_existing_star_holding_so_the_import_never_double_counts() {
        // The review's core case: the user already holds the `741★` card directly (a manual
        // add, or a legacy pre-#209 import), AND an import brings `741★`. Both must land on
        // the single base `741` foil — no second row, no doubled count.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let base = insert_variant(&db, 1, "ext-741", "741", "nonfoil", "ora-chaos").await;
        let star = insert_variant(&db, 2, "ext-741-star", "741★", "foil", "ora-chaos").await;
        // Pre-existing holding on the star card itself (stored as regular, pre-fix).
        insert_holding(&db, user_id, star, 1, 0).await;

        // Overwrite is the non-mirroring mode most exposed to the double-count.
        let holdings = vec![holding("ext-741-star", false, 1)];
        reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Overwrite,
            holdings,
        )
        .await
        .expect("reconcile");

        assert_eq!(owned_counts(&db, user_id, base).await, Some((0, 1)), "one foil on the base");
        assert_eq!(owned_counts(&db, user_id, star).await, None, "no leftover star holding");
    }

    #[tokio::test]
    async fn overwrite_of_only_the_nonfoil_sets_the_consolidated_foil_per_the_import() {
        // Once a foil-★ folds onto its base, the base is one dual-finish card, so an
        // Overwrite import that lists the base's nonfoil but not the foil-★ sets the base to
        // the import's *absolute* counts (foil = 0) — the same authoritative-overwrite
        // behavior Overwrite already applies to any card owned in both finishes. (Merge
        // adds; Smart preserves the unobserved foil — a user who tracks a foil their import
        // omits should use those modes.) Pinned so this stays intentional, not accidental.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "importer@test.example").await;
        let base = insert_variant(&db, 1, "ext-741", "741", "nonfoil", "ora-chaos").await;
        insert_variant(&db, 2, "ext-741-star", "741★", "foil", "ora-chaos").await;
        // Already consolidated: the base holds 2 foil, no separate star holding.
        insert_holding(&db, user_id, base, 0, 2).await;

        let holdings = vec![holding("ext-741", false, 3)]; // the import lists only the nonfoil
        reconcile_holdings(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            ReconcileMode::Overwrite,
            holdings,
        )
        .await
        .expect("reconcile");

        assert_eq!(
            owned_counts(&db, user_id, base).await,
            Some((3, 0)),
            "Overwrite sets the base's absolute counts from the import"
        );
    }
}
