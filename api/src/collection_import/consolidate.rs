//! Foil-variant consolidation (issue #209).
//!
//! Some sets — Secret Lair especially — print the **foil** of a card as a *separate*
//! Scryfall object whose collector number is the nonfoil's plus a star, e.g. `sld` `741`
//! (nonfoil, `finishes = "nonfoil"`) and `741★` (foil, `finishes = "foil"`). A provider
//! import that reports the `741★` printing would otherwise land as its own owned card,
//! sitting *alongside* the `741` you already track — two rows for one card.
//!
//! Our collection model already tracks regular **and** foil copies per card, so the foil
//! printing belongs on the base card as a foil copy, not as a separate holding. This module
//! resolves the star↔base pairs and folds a foil-★ holding onto its base as foil in two
//! places, so no path leaves a holding on a star card:
//! - [`apply_foil_remap`] / [`consolidate_local`] fold the **incoming** import onto the base
//!   before the reconcile engine sees it (aggregation, resolution, and reconcile then stay a
//!   straight 1:1 external-id→card mapping — see [`super::reconcile`]);
//! - [`fold_existing_star_holdings`] folds any **already-held** star row onto its base first
//!   (a legacy pre-#209 import, or a manual add of the `…★` catalog card), so a manual/legacy
//!   star holding never coexists with the consolidated base and double-counts.
//!
//! The rule is deliberately **conservative**. Only a `finishes = "foil"` star with a
//! `finishes = "nonfoil"` sibling (same game, set, oracle id, and collector number sans the
//! star) is folded — the case where the base card genuinely can't be foil on its own, so the
//! star is unambiguously *its* foil. A star whose base is itself foilable (`nonfoil,foil`) or
//! foil-only, an `etched` star, or a star with no base sibling (a standalone promo) is left as
//! its own card: those are distinct printings, not a nonfoil card's foil counterpart. The
//! foil price for these base cards is carried over from the star by
//! [`crate::scryfall::enrich_foil_variant_prices`] so the folded foil values correctly; the
//! legacy-data twin of this rule lives in the `m..023_consolidate_foil_star_holdings`
//! migration. Keep the three rules in step.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    Set, TransactionTrait,
};

use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};

use super::{FetchedHolding, ImportError, IN_CHUNK};

/// The Scryfall "foil variant" collector-number suffix (U+2605 BLACK STAR).
const FOIL_STAR: char = '★';

/// A resolved foil-★ ↔ nonfoil-base pairing (both the external and internal ids, so the same
/// resolution feeds both the external-id remap of incoming holdings and the internal-id fold
/// of existing holdings).
pub(super) struct FoilVariantPair {
    pub star_id: i32,
    pub star_ext: String,
    pub base_id: i32,
    pub base_ext: String,
}

/// Resolve every purely-foil `…★` card in `game` that has a purely-nonfoil sibling to that
/// pairing. Empty when the game has no such pairs (so a non-MTG game, or a catalog that hasn't
/// synced, costs one small query and nothing else). See the module docs for the (conservative)
/// matching rule.
pub(super) async fn load_foil_variant_pairs(
    db: &DatabaseConnection,
    game: &str,
) -> Result<Vec<FoilVariantPair>, ImportError> {
    // 1. Every purely-foil star card in the game.
    let stars = Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::Finishes.eq("foil"))
        .filter(card::Column::CollectorNumber.like(format!("%{FOIL_STAR}")))
        .all(db)
        .await
        .map_err(ImportError::Db)?;
    if stars.is_empty() {
        return Ok(Vec::new());
    }

    // 2. The distinct (set_code, base_collector_number) pairs to resolve.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut base_pairs: Vec<(String, String)> = Vec::new();
    for s in &stars {
        let key = (s.set_code.clone(), s.collector_number.replace(FOIL_STAR, ""));
        if seen.insert(key.clone()) {
            base_pairs.push(key);
        }
    }

    // 3. Resolve each pair to its purely-nonfoil base card (chunked tuple IN, two binds per
    //    pair, under SQLite's per-statement bind limit). `(set_code, collector_number)` is
    //    unique within a set, so a pair resolves to at most one base.
    let mut base_by_pair: HashMap<(String, String), BaseCard> = HashMap::new();
    for chunk in base_pairs.chunks(IN_CHUNK / 2) {
        let bases = Card::find()
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::Finishes.eq("nonfoil"))
            .filter(
                Expr::tuple([
                    Expr::col(card::Column::SetCode).into(),
                    Expr::col(card::Column::CollectorNumber).into(),
                ])
                .in_tuples(chunk.iter().cloned()),
            )
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        for b in bases {
            base_by_pair.insert(
                (b.set_code, b.collector_number),
                BaseCard {
                    id: b.id,
                    external_id: b.external_id,
                    oracle_id: b.oracle_id,
                },
            );
        }
    }

    // 4. Emit a pair where the oracle ids agree (a true foil/nonfoil sibling shares an oracle
    //    id; both must be present and equal — a NULL never matches, mirroring the migration's
    //    SQL `oracle_id = oracle_id`, so all three consolidation rules fold the same pairs).
    let mut pairs = Vec::with_capacity(stars.len());
    for s in stars {
        let base_cn = s.collector_number.replace(FOIL_STAR, "");
        if let Some(base) = base_by_pair.get(&(s.set_code.clone(), base_cn))
            && s.oracle_id.is_some()
            && base.oracle_id == s.oracle_id
        {
            pairs.push(FoilVariantPair {
                star_id: s.id,
                star_ext: s.external_id,
                base_id: base.id,
                base_ext: base.external_id.clone(),
            });
        }
    }
    Ok(pairs)
}

/// A resolved nonfoil base card (the fields the pairing needs).
struct BaseCard {
    id: i32,
    external_id: String,
    oracle_id: Option<String>,
}

/// The star→base **external-id** map derived from the pairs, for folding incoming holdings.
pub(super) fn ext_remap(pairs: &[FoilVariantPair]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|p| (p.star_ext.clone(), p.base_ext.clone()))
        .collect()
}

/// Fold each foil-★ holding onto its base card as a foil copy: rewrite its external id to the
/// base and force its finish to foil. A holding not in `remap` passes through untouched. Pure
/// — the DB work is all in [`load_foil_variant_pairs`].
pub(super) fn apply_foil_remap(
    holdings: Vec<FetchedHolding>,
    remap: &HashMap<String, String>,
) -> Vec<FetchedHolding> {
    if remap.is_empty() {
        return holdings;
    }
    holdings
        .into_iter()
        .map(|mut h| {
            if let Some(base) = remap.get(&h.external_card_id) {
                h.external_card_id = base.clone();
                h.foil = true;
            }
            h
        })
        .collect()
}

/// Fold a local holdings snapshot (`external_id -> (regular, foil)`) through the remap so it
/// speaks the same base external ids the fetched holdings will after [`apply_foil_remap`]. A
/// folded star's regular **and** foil copies become foil on the base. Used only by the smart
/// fetch's early-stop comparison, so a collection already holding a foil-★ still matches its
/// re-fetched, remapped page.
pub(super) fn consolidate_local(
    local: HashMap<String, (i32, i32)>,
    remap: &HashMap<String, String>,
) -> HashMap<String, (i32, i32)> {
    if remap.is_empty() {
        return local;
    }
    let mut out: HashMap<String, (i32, i32)> = HashMap::with_capacity(local.len());
    for (ext, (reg, foil)) in local {
        match remap.get(&ext) {
            // A star's copies (whichever finish they were stored under) are foils of the base.
            Some(base) => {
                let e = out.entry(base.clone()).or_insert((0, 0));
                e.1 = e.1.saturating_add(reg).saturating_add(foil);
            }
            None => {
                let e = out.entry(ext).or_insert((0, 0));
                e.0 = e.0.saturating_add(reg);
                e.1 = e.1.saturating_add(foil);
            }
        }
    }
    out
}

/// Fold this user's **existing** holdings on star cards onto their base as foil, then delete
/// the star holdings — so a manual add of the `…★` catalog card, or a legacy pre-#209 row,
/// never coexists with the base holding the import writes (which would double-count the foil).
/// Runs before the reconcile reads the current collection, in its own transaction; a folded
/// star's regular + foil copies both become foil on the base (never lossy — the copies move,
/// they aren't dropped). A no-op when the user holds no star cards.
///
/// The read-then-relocate is one transaction, but not globally serialized: two imports for
/// the *same* user racing (a double-submitted CSV) can both fold the same star and over-count
/// its foil. It's best-effort like the rest of reconcile (the fold and the apply are separate
/// transactions anyway); Overwrite/Replace's absolute set masks it, and a full Replace heals
/// it. Never lossy in any interleaving.
pub(super) async fn fold_existing_star_holdings(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    pairs: &[FoilVariantPair],
) -> Result<(), ImportError> {
    if pairs.is_empty() {
        return Ok(());
    }
    let base_by_star: HashMap<i32, i32> = pairs.iter().map(|p| (p.star_id, p.base_id)).collect();
    let star_ids: Vec<i32> = base_by_star.keys().copied().collect();

    // Do the whole fold in one transaction — read the user's star holdings, then relocate
    // each onto its base and delete it — so it's all-or-nothing and doesn't read the star
    // rows before the write window opens (which would let two concurrent imports of the same
    // user both fold the same star). Each star maps to a distinct base, so no two folds
    // target the same base row within a run.
    let txn = db.begin().await.map_err(ImportError::Db)?;
    let now = Utc::now();

    let mut star_holdings: Vec<collection_item::Model> = Vec::new();
    for chunk in star_ids.chunks(IN_CHUNK) {
        let rows = CollectionItem::find()
            .filter(collection_item::Column::UserId.eq(user_id))
            .filter(collection_item::Column::Game.eq(game))
            .filter(collection_item::Column::CardId.is_in(chunk.iter().copied()))
            .all(&txn)
            .await
            .map_err(ImportError::Db)?;
        star_holdings.extend(rows);
    }
    if star_holdings.is_empty() {
        return Ok(());
    }

    for star in &star_holdings {
        let base_id = base_by_star[&star.card_id];
        let foil_add = i64::from(star.quantity) + i64::from(star.foil_quantity);
        let existing_base = CollectionItem::find()
            .filter(collection_item::Column::UserId.eq(user_id))
            .filter(collection_item::Column::Game.eq(game))
            .filter(collection_item::Column::CardId.eq(base_id))
            .one(&txn)
            .await
            .map_err(ImportError::Db)?;
        match existing_base {
            Some(base) => {
                let new_foil = clamp_quantity(i64::from(base.foil_quantity) + foil_add);
                let mut am = base.into_active_model();
                am.foil_quantity = Set(new_foil);
                am.updated_at = Set(now);
                am.update(&txn).await.map_err(ImportError::Db)?;
            }
            None => {
                collection_item::ActiveModel {
                    user_id: Set(user_id),
                    game: Set(game.to_string()),
                    card_id: Set(base_id),
                    quantity: Set(0),
                    foil_quantity: Set(clamp_quantity(foil_add)),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .insert(&txn)
                .await
                .map_err(ImportError::Db)?;
            }
        }
    }

    // Drop the folded star holdings (chunked delete by key).
    let held_star_ids: Vec<i32> = star_holdings.iter().map(|s| s.card_id).collect();
    for chunk in held_star_ids.chunks(IN_CHUNK) {
        CollectionItem::delete_many()
            .filter(collection_item::Column::UserId.eq(user_id))
            .filter(collection_item::Column::Game.eq(game))
            .filter(collection_item::Column::CardId.is_in(chunk.iter().copied()))
            .exec(&txn)
            .await
            .map_err(ImportError::Db)?;
    }

    txn.commit().await.map_err(ImportError::Db)
}

fn clamp_quantity(value: i64) -> i32 {
    value.clamp(0, i64::from(collection_item::MAX_CARD_QUANTITY)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        card_model, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };

    /// Insert a card with an explicit set/number/finishes/oracle id (the fields the pairing
    /// keys off), reusing the canonical all-defaults row. Returns its internal id.
    async fn insert(
        db: &DatabaseConnection,
        id: i32,
        external_id: &str,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: Option<&str>,
    ) -> i32 {
        card::Model {
            external_id: external_id.into(),
            set_code: set_code.into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: oracle_id.map(str::to_string),
            ..card_model(id)
        }
        .into_active_model()
        .insert(db)
        .await
        .expect("insert card")
        .id
    }

    fn holding(id: &str, foil: bool, quantity: i32) -> FetchedHolding {
        FetchedHolding {
            external_card_id: id.into(),
            foil,
            quantity,
        }
    }

    #[tokio::test]
    async fn pairs_cover_only_a_foil_star_with_a_nonfoil_base() {
        let db = migrated_memory_db().await;
        // The issue's case: nonfoil 741 + foil 741★ (share an oracle id) -> a pair.
        insert(&db, 1, "ext-741", "sld", "741", "nonfoil", Some("ora-chaos")).await;
        insert(&db, 2, "ext-741-star", "sld", "741★", "foil", Some("ora-chaos")).await;
        // A foil star whose base is itself foilable -> NOT a pair (ambiguous).
        insert(&db, 3, "ext-33", "stx", "33", "nonfoil,foil", Some("ora-proctor")).await;
        insert(&db, 4, "ext-33-star", "stx", "33★", "foil", Some("ora-proctor")).await;
        // An etched star -> NOT a pair (a distinct premium finish).
        insert(&db, 5, "ext-159", "sld", "159", "nonfoil,foil", Some("ora-belz")).await;
        insert(&db, 6, "ext-159-star", "sld", "159★", "etched", Some("ora-belz")).await;
        // An orphan foil star (no base sibling) -> NOT a pair.
        insert(&db, 7, "ext-orphan", "pxln", "1★", "foil", Some("ora-orphan")).await;

        let pairs = load_foil_variant_pairs(&db, "mtg").await.expect("pairs");
        assert_eq!(pairs.len(), 1, "only the clean nonfoil-base case pairs");
        let remap = ext_remap(&pairs);
        assert_eq!(remap.get("ext-741-star").map(String::as_str), Some("ext-741"));
    }

    #[tokio::test]
    async fn pairs_skip_a_sibling_with_a_mismatched_oracle_id() {
        let db = migrated_memory_db().await;
        insert(&db, 1, "ext-b", "sld", "500", "nonfoil", Some("ora-a")).await;
        insert(&db, 2, "ext-s", "sld", "500★", "foil", Some("ora-different")).await;
        let pairs = load_foil_variant_pairs(&db, "mtg").await.expect("pairs");
        assert!(pairs.is_empty(), "oracle-id mismatch is not paired");
    }

    #[test]
    fn apply_foil_remap_rewrites_id_and_forces_foil() {
        let remap = HashMap::from([("star".to_string(), "base".to_string())]);
        // A star reported as a non-foil row still becomes a foil of the base.
        let out = apply_foil_remap(
            vec![holding("star", false, 2), holding("other", false, 1)],
            &remap,
        );
        assert_eq!(out[0], holding("base", true, 2), "remapped to base + forced foil");
        assert_eq!(out[1], holding("other", false, 1), "untouched");
    }

    #[test]
    fn consolidate_local_folds_star_copies_into_base_foil() {
        let remap = HashMap::from([("star".to_string(), "base".to_string())]);
        // A base owned as 2 regular, plus a legacy star row of 1 (stored as regular).
        let local = HashMap::from([("base".to_string(), (2, 0)), ("star".to_string(), (1, 0))]);
        let out = consolidate_local(local, &remap);
        assert_eq!(out.len(), 1);
        assert_eq!(out["base"], (2, 1), "star's copy becomes the base's foil");
    }

    #[tokio::test]
    async fn fold_existing_moves_a_manual_star_holding_onto_the_base_and_deletes_it() {
        let db = migrated_memory_db().await;
        let user = insert_user(&db, "folder@test.example").await;
        let base = insert(&db, 1, "ext-741", "sld", "741", "nonfoil", Some("ora-chaos")).await;
        let star = insert(&db, 2, "ext-741-star", "sld", "741★", "foil", Some("ora-chaos")).await;
        // Base owned 1 regular; a manual star holding of 2 (stored as regular).
        insert_holding(&db, user, base, 1, 0).await;
        insert_holding(&db, user, star, 2, 0).await;

        let pairs = load_foil_variant_pairs(&db, "mtg").await.expect("pairs");
        fold_existing_star_holdings(&db, user, "mtg", &pairs).await.expect("fold");

        assert_eq!(owned_counts(&db, user, base).await, Some((1, 2)), "star folded into base foil");
        assert_eq!(owned_counts(&db, user, star).await, None, "star holding removed");
    }

    #[tokio::test]
    async fn fold_existing_inserts_a_base_when_only_the_star_is_held() {
        let db = migrated_memory_db().await;
        let user = insert_user(&db, "folder@test.example").await;
        let base = insert(&db, 1, "ext-741", "sld", "741", "nonfoil", Some("ora-chaos")).await;
        let star = insert(&db, 2, "ext-741-star", "sld", "741★", "foil", Some("ora-chaos")).await;
        insert_holding(&db, user, star, 0, 3).await; // 3 foil, no base holding

        let pairs = load_foil_variant_pairs(&db, "mtg").await.expect("pairs");
        fold_existing_star_holdings(&db, user, "mtg", &pairs).await.expect("fold");

        assert_eq!(owned_counts(&db, user, base).await, Some((0, 3)), "base created as foil");
        assert_eq!(owned_counts(&db, user, star).await, None);
    }

    #[tokio::test]
    async fn fold_existing_is_a_noop_when_no_star_is_held() {
        let db = migrated_memory_db().await;
        let user = insert_user(&db, "folder@test.example").await;
        let base = insert(&db, 1, "ext-741", "sld", "741", "nonfoil", Some("ora-chaos")).await;
        insert(&db, 2, "ext-741-star", "sld", "741★", "foil", Some("ora-chaos")).await;
        insert_holding(&db, user, base, 2, 1).await;

        let pairs = load_foil_variant_pairs(&db, "mtg").await.expect("pairs");
        fold_existing_star_holdings(&db, user, "mtg", &pairs).await.expect("fold");

        assert_eq!(owned_counts(&db, user, base).await, Some((2, 1)), "untouched");
    }
}
