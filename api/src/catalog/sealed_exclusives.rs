//! Booster **exclusivity derivation**: stamp `sealed_contents.exclusive` for every card a
//! product's own booster family can pull but no other-family booster in the same set can —
//! the flag the product page splits its "Exclusive to Collector Boosters" section on.
//!
//! Derived once per sync tick rather than at read time, for two reasons that pull in
//! opposite directions and meet here:
//!
//! * The answer is a **cross-product** fact — it is decided by every *sibling* booster's
//!   whole pull pool, not by the row it lands on — so a read-time derivation could not be
//!   narrowed to the page it serves. `handlers::catalog::products` paid for that on every
//!   `/cards` and `/cards/sections` request, page 2 and 3 included: two extra queries, the
//!   second of them a scan of the comparison families' entire pool (measured in production
//!   at 7.2s for one collector booster's 7,968 rows). The set it produces is also a *sort*
//!   key — family-exclusive printings lead the shared pool — so even a single-page read
//!   needed all of it.
//! * The inputs come from **two independently-gated syncs**. `sealed_contents` is rebuilt
//!   only when MTGJSON's ETag moves, but the family judgement reads
//!   `products.product_type` — TCGCSV's classification, refreshed on its own sweep — so a
//!   flag folded at rebuild time would go stale the moment a product was reclassified.
//!   Hence the `precon_decks.price_cents` model (`m..077`): the wholesale rebuild writes
//!   the column's `false` default and this pass recomputes it from the live rows each tick
//!   (and once at boot on the no-sync path), which is also why `m..085` needs no
//!   `DERIVATION_VERSION` bump — the derivation never runs inside the ETag-gated rebuild.
//!
//! The rule reproduced here is the one `booster_exclusive_card_ids` used to apply per
//! request, guard for guard, because the read still renders what this writes:
//!
//! 1. Only a product whose **own** `product_type` is a booster family is judged. An
//!    "Exclusive to Collector Boosters" heading belongs on the collector boosters' own
//!    pages, never on a bundle that merely wraps one (issue #646).
//! 2. Judged over the **plain view** only — a row is in it when its `component` is `NULL`
//!    (the product's own contents) or names a **listed** component (one resolving to its
//!    own catalog product). A card reaching the product through an *unlisted* component
//!    renders in that component's own named section, which keeps its own certainty split
//!    and never shows an exclusive bucket.
//! 3. A card counts as one of this product's booster cards only when its **collapsed**
//!    plain membership is `booster`: a printing the product also *guarantees* is a
//!    `contains` card, and a guarantee outranks a pull (`Membership::rank`).
//! 4. The comparison pool is every `booster` row of the same set's booster products of a
//!    **different** family — unfolded and unscoped by component, exactly as the read's
//!    query was. A set with no other-family booster, or whose other families carry no
//!    ingested booster row, yields nothing rather than marking its whole pool exclusive,
//!    which would be vacuously true.
//!
//! Cost: one read of the booster products' membership rows per tick, batched under
//! [`PRODUCT_BATCH`]. Sets holding a single booster family are skipped without reading
//! their pools at all (nothing in them can be exclusive) and only have stale flags
//! cleared. Writes are diffed against the stored column and grouped by target, so the
//! steady state — no rebuild, no reclassification — issues no `UPDATE` at all.

use std::collections::{HashMap, HashSet};

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect, sea_query::Expr,
};

use crate::entities::prelude::{Product, SealedComponent, SealedContent};
use crate::entities::sealed_component::ComponentKind;
use crate::entities::sealed_content::Membership;
use crate::entities::{product, sealed_component, sealed_content};
use crate::tcgcsv::classify::{BOOSTER_PRODUCT_TYPES, BoosterFamily, booster_family};

/// SQLite caps host parameters per statement (as few as 999 on old builds), so every
/// `IN (…)` list is chunked well under the bind limit.
const IN_CHUNK: usize = 900;

/// How many products' membership rows to hold in memory at once. Sets are never split
/// across batches — a product's comparison pool is its set's other-family boosters, so they
/// must be resident together — which makes this a floor, not a cap: a single set larger
/// than the batch is still processed whole.
const PRODUCT_BATCH: usize = 200;

/// One set's booster products, paired with the family each belongs to.
type SetGroup = Vec<(i32, BoosterFamily)>;

/// Recompute `sealed_contents.exclusive` for `game`, writing only the rows whose flag
/// actually changes. Returns how many rows changed.
pub async fn refresh_sealed_exclusives(db: &DatabaseConnection, game: &str) -> Result<u64, DbErr> {
    // Every booster product of the game, with the two columns the judgement reads. A
    // non-booster is neither judged nor a comparison pool, so its rows never need loading —
    // and they can never carry the flag, since this pass is the only thing that sets it.
    let products: Vec<(i32, String, String)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::SetCode)
        .column(product::Column::ProductType)
        .filter(product::Column::Game.eq(game))
        .filter(product::Column::ProductType.is_in(BOOSTER_PRODUCT_TYPES.iter().copied()))
        .into_tuple()
        .all(db)
        .await?;
    if products.is_empty() {
        return Ok(0);
    }

    // Group by set: exclusivity is judged only against the same set's boosters.
    let mut by_set: HashMap<String, SetGroup> = HashMap::new();
    for (id, set_code, product_type) in products {
        // `BOOSTER_PRODUCT_TYPES` is the union of the families, so this always resolves;
        // skipping rather than unwrapping leaves the two lists free to drift.
        if let Some(family) = booster_family(&product_type) {
            by_set.entry(set_code).or_default().push((id, family));
        }
    }

    // A set holding one family has nothing to compare against (guard 4), so none of its
    // cards can be exclusive — and clearing those flags needs no read of their pools.
    let mut single_family: Vec<i32> = Vec::new();
    let mut comparable: Vec<SetGroup> = Vec::new();
    for members in by_set.into_values() {
        let first = members[0].1;
        if members.iter().all(|(_, family)| *family == first) {
            single_family.extend(members.into_iter().map(|(id, _)| id));
        } else {
            comparable.push(members);
        }
    }

    let mut changed = clear_flags(db, game, &single_family).await?;

    // Batch whole sets together so every product's comparison pool is resident with it.
    let mut batch: Vec<SetGroup> = Vec::new();
    let mut batched_products = 0;
    for members in comparable {
        batched_products += members.len();
        batch.push(members);
        if batched_products >= PRODUCT_BATCH {
            changed += refresh_batch(db, game, &batch).await?;
            batch.clear();
            batched_products = 0;
        }
    }
    if !batch.is_empty() {
        changed += refresh_batch(db, game, &batch).await?;
    }

    Ok(changed)
}

/// Clear any flag left on products that can no longer hold one — their set lost its second
/// booster family, or they were reclassified. A no-op in the steady state: the `= true`
/// predicate matches nothing once the flags are already down.
async fn clear_flags(
    db: &DatabaseConnection,
    game: &str,
    product_ids: &[i32],
) -> Result<u64, DbErr> {
    let mut changed = 0;
    for chunk in product_ids.chunks(IN_CHUNK) {
        let result = SealedContent::update_many()
            .col_expr(sealed_content::Column::Exclusive, Expr::value(false))
            .filter(sealed_content::Column::Game.eq(game))
            .filter(sealed_content::Column::ProductId.is_in(chunk.iter().copied()))
            .filter(sealed_content::Column::Exclusive.eq(true))
            .exec(db)
            .await?;
        changed += result.rows_affected;
    }
    Ok(changed)
}

/// Judge one batch of whole sets and write back only the rows whose flag moved.
async fn refresh_batch(
    db: &DatabaseConnection,
    game: &str,
    batch: &[SetGroup],
) -> Result<u64, DbErr> {
    let product_ids: Vec<i32> = batch
        .iter()
        .flat_map(|members| members.iter().map(|(id, _)| *id))
        .collect();

    let listed = listed_component_names(db, game, &product_ids).await?;
    let rows = membership_rows(db, game, &product_ids).await?;
    if rows.is_empty() {
        return Ok(0);
    }

    let booster_rank = Membership::rank(Membership::Booster.as_str());
    let booster = Membership::Booster.as_str();

    // Per product: the plain view's collapsed rank per card (the read's `best_memberships`
    // keeps the strongest, i.e. lowest, rank), and the product's whole unfolded booster
    // pool (what a *sibling* is compared against — component-blind, as the read's query
    // was).
    let mut plain_rank: HashMap<i32, HashMap<i32, u8>> = HashMap::new();
    let mut pool_by_product: HashMap<i32, HashSet<i32>> = HashMap::new();
    for (_, product_id, card_id, membership, component, _) in &rows {
        if membership == booster {
            pool_by_product
                .entry(*product_id)
                .or_default()
                .insert(*card_id);
        }
        if !in_plain_view(component.as_deref(), listed.get(product_id)) {
            continue;
        }
        let rank = Membership::rank(membership);
        plain_rank
            .entry(*product_id)
            .or_default()
            .entry(*card_id)
            .and_modify(|best| *best = (*best).min(rank))
            .or_insert(rank);
    }

    let comparison = comparison_pools(batch, &pool_by_product);

    let mut raise: Vec<i32> = Vec::new();
    let mut clear: Vec<i32> = Vec::new();
    for (row_id, product_id, card_id, membership, component, current) in &rows {
        let target = membership == booster
            && in_plain_view(component.as_deref(), listed.get(product_id))
            && plain_rank
                .get(product_id)
                .and_then(|ranks| ranks.get(card_id))
                .is_some_and(|rank| *rank == booster_rank)
            && comparison
                .get(product_id)
                .is_some_and(|pool| !pool.contains(card_id));
        if target == *current {
            continue;
        }
        if target { &mut raise } else { &mut clear }.push(*row_id);
    }

    let mut changed = write_flags(db, &raise, true).await?;
    changed += write_flags(db, &clear, false).await?;
    Ok(changed)
}

/// Which of each product's component names resolve to a catalog product of their own. Only
/// `sealed` line items link children, and the membership attribution stores the same `name`
/// the composition row does, so the name is the join key — the read path's rule.
async fn listed_component_names(
    db: &DatabaseConnection,
    game: &str,
    product_ids: &[i32],
) -> Result<HashMap<i32, HashSet<String>>, DbErr> {
    let mut listed: HashMap<i32, HashSet<String>> = HashMap::new();
    for chunk in product_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, String)> = SealedComponent::find()
            .select_only()
            .column(sealed_component::Column::ProductId)
            .column(sealed_component::Column::Name)
            .filter(sealed_component::Column::Game.eq(game))
            .filter(sealed_component::Column::Kind.eq(ComponentKind::Sealed.as_str()))
            .filter(sealed_component::Column::ChildProductId.is_not_null())
            .filter(sealed_component::Column::ProductId.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await?;
        for (product_id, name) in rows {
            listed.entry(product_id).or_default().insert(name);
        }
    }
    Ok(listed)
}

/// Every membership row of the given products. `id` rides along so the write-back can
/// address rows directly, and `exclusive` so an unchanged flag costs no `UPDATE`.
#[allow(clippy::type_complexity)]
async fn membership_rows(
    db: &DatabaseConnection,
    game: &str,
    product_ids: &[i32],
) -> Result<Vec<(i32, i32, i32, String, Option<String>, bool)>, DbErr> {
    let mut rows: Vec<(i32, i32, i32, String, Option<String>, bool)> = Vec::new();
    for chunk in product_ids.chunks(IN_CHUNK) {
        let mut page: Vec<(i32, i32, i32, String, Option<String>, bool)> = SealedContent::find()
            .select_only()
            .column(sealed_content::Column::Id)
            .column(sealed_content::Column::ProductId)
            .column(sealed_content::Column::CardId)
            .column(sealed_content::Column::Membership)
            .column(sealed_content::Column::Component)
            .column(sealed_content::Column::Exclusive)
            .filter(sealed_content::Column::Game.eq(game))
            .filter(sealed_content::Column::ProductId.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await?;
        rows.append(&mut page);
    }
    Ok(rows)
}

/// Whether a membership row is in the product's **plain** view: its own direct contents, or
/// a component the reader can click through to. Rows attributed to an unlisted component
/// render in that component's own section instead.
fn in_plain_view(component: Option<&str>, listed: Option<&HashSet<String>>) -> bool {
    match component {
        None => true,
        Some(name) => listed.is_some_and(|names| names.contains(name)),
    }
}

/// For each product, the union of its set-mates' booster pools — the other-family cards it
/// is judged against. Folded **per family** rather than per product, so a set costs one
/// union per family present rather than one per product (a 20-product set with 5 families
/// would otherwise re-union ~8,000-card pools 20 times).
///
/// A product whose set holds no other-family booster, or whose other families carry no
/// ingested booster row, is **absent** from the map — the read's "nothing to compare
/// against" guards, under which nothing is exclusive rather than everything.
fn comparison_pools(
    batch: &[SetGroup],
    pool_by_product: &HashMap<i32, HashSet<i32>>,
) -> HashMap<i32, HashSet<i32>> {
    let mut pools: HashMap<i32, HashSet<i32>> = HashMap::new();
    for members in batch {
        // One pool per family present in this set.
        let mut by_family: HashMap<BoosterFamily, HashSet<i32>> = HashMap::new();
        for (id, family) in members {
            let entry = by_family.entry(*family).or_default();
            if let Some(cards) = pool_by_product.get(id) {
                entry.extend(cards.iter().copied());
            }
        }
        for (id, family) in members {
            let mut pool: HashSet<i32> = HashSet::new();
            let mut has_comparison = false;
            for (other_family, cards) in &by_family {
                if other_family == family {
                    continue;
                }
                has_comparison = true;
                pool.extend(cards.iter().copied());
            }
            if has_comparison && !pool.is_empty() {
                pools.insert(*id, pool);
            }
        }
    }
    pools
}

/// Set `exclusive` on the given rows, chunked under the bind limit.
async fn write_flags(db: &DatabaseConnection, row_ids: &[i32], value: bool) -> Result<u64, DbErr> {
    let mut changed = 0;
    for chunk in row_ids.chunks(IN_CHUNK) {
        let result = SealedContent::update_many()
            .col_expr(sealed_content::Column::Exclusive, Expr::value(value))
            .filter(sealed_content::Column::Id.is_in(chunk.iter().copied()))
            .exec(db)
            .await?;
        changed += result.rows_affected;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pools(entries: &[(i32, &[i32])]) -> HashMap<i32, HashSet<i32>> {
        entries
            .iter()
            .map(|(id, cards)| (*id, cards.iter().copied().collect()))
            .collect()
    }

    #[test]
    fn plain_view_takes_direct_and_listed_rows_only() {
        let listed: HashSet<String> = ["Play Booster".to_string()].into_iter().collect();
        // The product's own contents.
        assert!(in_plain_view(None, Some(&listed)));
        assert!(in_plain_view(None, None));
        // Inherited through a component the reader can click through to.
        assert!(in_plain_view(Some("Play Booster"), Some(&listed)));
        // Inherited through an unlisted component: it renders in that component's own
        // section, which never carries the exclusive split.
        assert!(!in_plain_view(Some("Land Pack"), Some(&listed)));
        assert!(!in_plain_view(Some("Land Pack"), None));
    }

    #[test]
    fn comparison_pool_is_the_other_families_union() {
        // One set: a collector pack + box (one family) and a play pack (another).
        let batch = vec![vec![
            (1, BoosterFamily::Collector),
            (2, BoosterFamily::Collector),
            (3, BoosterFamily::Play),
        ]];
        let pools = pools(&[(1, &[10, 11]), (2, &[11, 12]), (3, &[10, 20])]);
        let comparison = comparison_pools(&batch, &pools);

        // The collector products are judged against the play pool alone — a same-family
        // sibling is never a comparison pool, or a collector box would cancel out its own
        // pack's exclusives.
        assert_eq!(comparison[&1], [10, 20].into_iter().collect());
        assert_eq!(comparison[&2], [10, 20].into_iter().collect());
        // The play product is judged against the union of both collector products.
        assert_eq!(comparison[&3], [10, 11, 12].into_iter().collect());
    }

    #[test]
    fn a_single_family_set_has_no_comparison_pool() {
        // A collector-only release: "exclusive" would be vacuously true of every card, so
        // the product is absent from the map and nothing is flagged.
        let batch = vec![vec![
            (1, BoosterFamily::Collector),
            (2, BoosterFamily::Collector),
        ]];
        let comparison = comparison_pools(&batch, &pools(&[(1, &[10]), (2, &[11])]));
        assert!(comparison.is_empty());
    }

    #[test]
    fn an_empty_other_family_pool_is_no_comparison() {
        // The other family's product exists but MTGJSON gave it no booster rows. The read
        // treated that as nothing to compare against — otherwise every card of the only
        // populated booster would be "exclusive", which is no signal — so the collector
        // product must be absent from the map.
        let batch = vec![vec![
            (1, BoosterFamily::Collector),
            (2, BoosterFamily::Play),
        ]];
        let comparison = comparison_pools(&batch, &pools(&[(1, &[10, 11])]));
        assert!(
            !comparison.contains_key(&1),
            "an empty comparison pool flags nothing, rather than flagging everything"
        );
        // The empty product itself may carry a pool entry; it has no booster rows of its
        // own to flag, so it is moot either way.
        assert_eq!(comparison[&2], [10, 11].into_iter().collect());
    }

    #[test]
    fn sets_are_judged_independently() {
        // Two sets in one batch: a card shared across sets must not cancel exclusivity,
        // because exclusivity is a within-set question.
        let batch = vec![
            vec![(1, BoosterFamily::Collector), (2, BoosterFamily::Play)],
            vec![(3, BoosterFamily::Collector), (4, BoosterFamily::Play)],
        ];
        let pools = pools(&[(1, &[10]), (2, &[99]), (3, &[10]), (4, &[98])]);
        let comparison = comparison_pools(&batch, &pools);
        assert_eq!(comparison[&1], [99].into_iter().collect());
        assert_eq!(comparison[&3], [98].into_iter().collect());
    }
}
