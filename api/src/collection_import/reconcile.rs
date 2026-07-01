//! Provider-independent reconcile engine: aggregate fetched holdings, resolve them to
//! local cards, plan the DB operations per [`ReconcileMode`], and apply them atomically.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

use super::{FetchedHolding, ImportError, ImportSummary, Provider, ReconcileMode};
use super::{IN_CHUNK, MAX_QUANTITY, UNMATCHED_SAMPLE_CAP};

/// Regular + foil copies owned of a single card. Held as `i64` during aggregation so a
/// pathological provider payload can't overflow before the final clamp.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) quantity: i64,
    pub(super) foil_quantity: i64,
}

/// Aggregate raw holdings into per-card regular/foil counts. The same printing can
/// appear across several provider rows (differing condition/language/tags), so counts
/// are summed; the row `foil` flag splits regular vs foil.
pub(super) fn aggregate(holdings: &[FetchedHolding]) -> HashMap<String, Counts> {
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
pub(super) struct ReconcilePlan {
    /// `(card_id, quantity, foil_quantity)` to set (both zero is treated as a delete).
    pub(super) upserts: Vec<(i32, i32, i32)>,
    /// Card ids whose holding row should be removed.
    pub(super) deletes: Vec<i32>,
}

pub(super) fn clamp_count(value: i64) -> i32 {
    value.clamp(0, MAX_QUANTITY) as i32
}

/// Decide the reconcile operations from the current holdings and the imported ones,
/// per `mode`. Pure: `existing`/`imported` are `card_id -> counts`, no DB access.
pub(super) fn plan_reconcile(
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
    // intent to wipe the collection — refuse before we delete everything.
    if imported.is_empty() && mode == ReconcileMode::Replace {
        return Err(ImportError::NoMatchingCards);
    }

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
