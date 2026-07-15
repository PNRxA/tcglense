//! Multi-source merge logic layered on top of the MTGJSON pass: curated fallback
//! memberships/components, Secret Lair drop→cards derivation, and the contained/sibling
//! booster-pool synthesis (issue #290). Most rules are gated on "the product has no rows of
//! its own", so MTGJSON / the fallback stay authoritative and each source fills only genuine
//! gaps. The deliberate exceptions attach an axis MTGJSON doesn't surface, and so must apply
//! even to a "covered" product: [`merge_sld_bonus_cards`] (a
//! superdrop's *shared bonus pool*) and a fallback entry flagged `supplement` (see
//! [`merge_fallback`] — e.g. cards upstream models through an incomplete deck reference;
//! issue #352). Supplements are additive unless they explicitly override contradictory
//! memberships for their curated cards.

use std::collections::{HashMap, HashSet};

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use super::resolve::{distinct, resolve_cards_by_setnum, resolve_products};
use super::super::fallback::{FallbackData, FallbackProduct};
use super::super::sld;
use super::super::{GAME, MtgjsonError};
use super::{ComponentRow, IN_CHUNK, Row};
use crate::entities::prelude::{Card, Product};
use crate::entities::{card, product, sealed_component, sealed_content};

/// Merge curated [`fallback`](super::super::fallback) memberships into `rows`, returning the
/// number of new rows added. A fallback product is applied **only when MTGJSON emitted no
/// rows for it** (`mtgjson_products`), so upstream stays authoritative and the fallback fills
/// genuine gaps (e.g. Avatar's originally-`contents: null` Commander's Bundle) — except an
/// entry flagged `supplement`, whose rows merge **even when** upstream describes the product:
/// they carry an axis upstream is missing (that same bundle again, once its contents gained an
/// incomplete deck reference plus textual-only land packs; issue #352).
/// A supplement is additive by default — upstream's rows are untouched and a row upstream
/// also emits dedups away, so it self-retires per card like [`merge_sld_bonus_cards`]. An
/// `override_memberships` supplement instead removes contradictory memberships for only the
/// curated product/card pairs before inserting the fallback rows. This corrects sources that
/// resolve the right cards under the wrong certainty (the Avatar Commander's Bundle's random
/// pool currently arrives as guaranteed) without replacing unrelated upstream cards. A
/// fallback product or card absent from our catalog, or carrying an unknown membership, is
/// skipped and logged.
pub(super) async fn merge_fallback(
    db: &DatabaseConnection,
    data: &FallbackData,
    mtgjson_products: &HashSet<i32>,
    rows: &mut HashSet<Row>,
) -> Result<usize, MtgjsonError> {
    if data.products.is_empty() {
        return Ok(0);
    }
    // Resolve fallback products (TCGplayer id) and cards ((set, number)) to internal ids.
    let product_ext: Vec<String> = distinct(data.products.iter().map(|p| &p.tcgplayer_product_id));
    let products = resolve_products(db, &product_ext).await?;
    let card_keys: Vec<(String, String)> = data
        .products
        .iter()
        .flat_map(|p| &p.contents)
        .map(|c| (c.set.to_lowercase(), c.number.clone()))
        .collect();
    let cards = resolve_cards_by_setnum(db, &card_keys).await?;

    let mut added = 0;
    let mut overridden = 0;
    for product in &data.products {
        let Some(&product_id) = products.get(&product.tcgplayer_product_id) else {
            tracing::debug!(
                product = %product.name,
                tcgplayer_id = %product.tcgplayer_product_id,
                "mtgjson fallback: product not in catalog, skipping"
            );
            continue;
        };
        // MTGJSON already describes this product — it wins, skip the whole fallback entry.
        // A `supplement` entry is the exception: its rows carry an axis upstream is
        // missing, so they merge alongside (additive unless it opts into the narrow
        // membership override below; duplicates dedup via the row set).
        if mtgjson_products.contains(&product_id) && !product.supplement {
            continue;
        }
        for card in &product.contents {
            let Some(&card_id) = cards.get(&(card.set.to_lowercase(), card.number.clone())) else {
                tracing::debug!(
                    card = %card.name, set = %card.set, number = %card.number,
                    "mtgjson fallback: card not in catalog, skipping"
                );
                continue;
            };
            let Some(membership) = card.parsed_membership() else {
                tracing::warn!(
                    membership = %card.membership, card = %card.name,
                    "mtgjson fallback: unknown membership, skipping"
                );
                continue;
            };
            if product.supplement && product.override_memberships {
                let before = rows.len();
                rows.retain(|&(row_product, row_card, row_membership, _)| {
                    row_product != product_id
                        || row_card != card_id
                        || row_membership == membership.as_str()
                });
                overridden += before - rows.len();
            }
            if rows.insert((product_id, card_id, membership.as_str(), card.foil)) {
                added += 1;
            }
        }
    }
    if added > 0 || overridden > 0 {
        tracing::info!(
            rows = added,
            overridden,
            "mtgjson: merged curated fallback memberships"
        );
    }
    Ok(added)
}

/// Merge curated [`fallback`](super::super::fallback) composition components into `rows`,
/// returning how many were added. Applied per product **only when MTGJSON gave that product
/// no composition** (`mtgjson_component_products`), mirroring [`merge_fallback`] — so upstream
/// stays authoritative and this fills genuine gaps (e.g. Avatar's `contents: null` Commander's
/// Bundle: "9× Play Booster, 1× Collector Booster, …"). A component's optional child
/// product / card links resolve to internal ids the same way MTGJSON's do; an unresolved
/// link keeps the line item as text. A fallback product absent from our catalog, or a
/// component with an unknown kind, is skipped and logged.
pub(super) async fn merge_fallback_components(
    db: &DatabaseConnection,
    data: &FallbackData,
    mtgjson_component_products: &HashSet<i32>,
    rows: &mut Vec<ComponentRow>,
) -> Result<usize, MtgjsonError> {
    let authored: Vec<&FallbackProduct> = data
        .products
        .iter()
        .filter(|p| !p.components.is_empty())
        .collect();
    if authored.is_empty() {
        return Ok(0);
    }

    // Resolve the fallback products (parents) + any child products they link, plus the
    // (set, number) keys of any card components, to internal ids.
    let product_ext: Vec<String> = distinct(
        authored.iter().map(|p| &p.tcgplayer_product_id).chain(
            authored
                .iter()
                .flat_map(|p| &p.components)
                .filter_map(|c| c.child_tcgplayer_product_id.as_ref()),
        ),
    );
    let products = resolve_products(db, &product_ext).await?;
    let card_keys: Vec<(String, String)> = authored
        .iter()
        .flat_map(|p| &p.components)
        .filter_map(|c| Some((c.child_set.as_ref()?.to_lowercase(), c.child_number.clone()?)))
        .collect();
    let cards = resolve_cards_by_setnum(db, &card_keys).await?;

    let mut added = 0;
    for product in &authored {
        let Some(&product_id) = products.get(&product.tcgplayer_product_id) else {
            tracing::debug!(
                product = %product.name,
                tcgplayer_id = %product.tcgplayer_product_id,
                "mtgjson fallback: composition product not in catalog, skipping"
            );
            continue;
        };
        // MTGJSON already describes this product's composition — it wins, skip the entry.
        if mtgjson_component_products.contains(&product_id) {
            continue;
        }
        let mut position = 0i32;
        for component in &product.components {
            let Some(kind) = component.parsed_kind() else {
                tracing::warn!(
                    kind = %component.kind, name = %component.name,
                    "mtgjson fallback: unknown component kind, skipping"
                );
                continue;
            };
            let child_product_id = component
                .child_tcgplayer_product_id
                .as_ref()
                .and_then(|id| products.get(id).copied());
            let child_card_id = match (&component.child_set, &component.child_number) {
                (Some(set), Some(number)) => {
                    cards.get(&(set.to_lowercase(), number.clone())).copied()
                }
                _ => None,
            };
            rows.push(ComponentRow {
                product_id,
                position,
                kind: kind.as_str().to_string(),
                name: component.name.clone(),
                quantity: component.quantity.max(1),
                child_product_id,
                child_card_id,
            });
            position += 1;
            added += 1;
        }
    }
    if added > 0 {
        tracing::info!(
            rows = added,
            "mtgjson: merged curated fallback composition components"
        );
    }
    Ok(added)
}

/// Derive card contents for Secret Lair Drop products MTGJSON **and** the fallback both
/// left empty. Each such product's name identifies its drop (see [`sld`](super::super::sld));
/// the drop's cards (by collector number in the `sld` set) become `contains` memberships,
/// foil per the product's edition. `covered` is the set of products that already have a
/// membership row, so this only fills genuine gaps — mirroring the fallback gate. Returns
/// rows added.
pub(super) async fn merge_sld_derived(
    db: &DatabaseConnection,
    covered: &HashSet<i32>,
    rows: &mut HashSet<Row>,
) -> Result<usize, MtgjsonError> {
    let Some(table) = sld::table() else {
        return Ok(0);
    };
    let products = load_sld_products(db).await?;

    // Resolve each still-empty SLD product to its drop, keeping the collector numbers it
    // contains — the drop's cards plus any shared superdrop bonus cards (a `'static` drop
    // table, so the strs are `'static`) — and the product's foilness.
    let mut resolved: Vec<(i32, bool, Vec<&'static str>)> = Vec::new();
    for (product_id, external_id, name) in &products {
        if covered.contains(product_id) {
            continue;
        }
        if let Some(pd) = sld::resolve_product_drop(table, external_id, name) {
            resolved.push((*product_id, pd.foil, pd.collector_numbers().collect()));
        }
    }
    if resolved.is_empty() {
        return Ok(0);
    }

    // Resolve just the needed collector numbers (all in the `sld` set) to internal card ids
    // — bounded to the drops in play, not the whole set.
    let mut numbers: Vec<&str> =
        resolved.iter().flat_map(|(_, _, cns)| cns.iter().copied()).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let cards = resolve_sld_cards(db, &numbers).await?;

    let membership = sealed_content::Membership::Contains.as_str();
    let mut added = 0;
    for (product_id, foil, cns) in &resolved {
        for cn in cns {
            let Some(&card_id) = cards.get(*cn) else {
                continue;
            };
            if rows.insert((*product_id, card_id, membership, *foil)) {
                added += 1;
            }
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: derived Secret Lair drop card contents");
    }
    Ok(added)
}

/// Attach each Secret Lair drop's curated **random bonus-card pool** to every product of the drop
/// as `variable` ("may be included") memberships — the shared bonus cards MTGJSON's `AllPrintings`
/// doesn't surface (e.g. Avatar's Command Tower / Fellwar Stone, one drawn at random per drop).
/// Unlike [`merge_sld_derived`], which fills a product's *own* drop cards, this runs even for
/// products whose drop cards MTGJSON already describes: the bonus pool is a distinct axis, so it is
/// **not** gated on `covered` (the cards stay linked even after MTGJSON authors the deck).
///
/// It is **add-only and self-retires per card**: `rows` is a set, so a `(product, card, variable,
/// foil)` MTGJSON already authored is deduplicated away and not re-counted — upstream wins for every
/// card it names, while a pool card it omits (e.g. FINAL FANTASY's shared Evoke rares, where MTGJSON
/// authors only the per-drop card) still surfaces. Never rewrites a product's own `contains` cards
/// (a shadowing number is excluded from [`sld::random_bonus_pool`] by curation). Returns rows added.
pub(super) async fn merge_sld_bonus_cards(
    db: &DatabaseConnection,
    rows: &mut HashSet<Row>,
) -> Result<usize, MtgjsonError> {
    let Some(table) = sld::table() else {
        return Ok(0);
    };
    let products = load_sld_products(db).await?;

    // (product_id, foil, collector number) for every random bonus card a product may carry.
    let mut resolved: Vec<(i32, bool, &'static str)> = Vec::new();
    for (product_id, external_id, name) in &products {
        let Some(pd) = sld::resolve_product_drop(table, external_id, name) else {
            continue;
        };
        for cn in sld::random_bonus_pool(&pd.drop.slug) {
            resolved.push((*product_id, pd.foil, cn));
        }
    }
    if resolved.is_empty() {
        return Ok(0);
    }

    let mut numbers: Vec<&str> = resolved.iter().map(|(_, _, cn)| *cn).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let cards = resolve_sld_cards(db, &numbers).await?;

    let variable = sealed_content::Membership::Variable.as_str();
    let mut added = 0;
    for (product_id, foil, cn) in &resolved {
        let Some(&card_id) = cards.get(*cn) else {
            continue;
        };
        if rows.insert((*product_id, card_id, variable, *foil)) {
            added += 1;
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: derived Secret Lair random bonus-card pools");
    }
    Ok(added)
}

/// Resolve `sld`-set cards by collector number -> internal `cards.id`, for just the numbers
/// requested (chunked under the bind limit). The set is always `sld`, so the returned map is
/// keyed by collector number alone.
async fn resolve_sld_cards(
    db: &DatabaseConnection,
    numbers: &[&str],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in numbers.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Card::find()
            .select_only()
            .column(card::Column::CollectorNumber)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::SetCode.eq(sld::SET_CODE))
            .filter(card::Column::CollectorNumber.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Load every Secret Lair sealed product `(id, external_id, name)` for the game — the
/// candidates the drop→cards derivation matches by name.
async fn load_sld_products(
    db: &DatabaseConnection,
) -> Result<Vec<(i32, String, String)>, MtgjsonError> {
    Ok(Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::ExternalId)
        .column(product::Column::Name)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::SetCode.eq(sld::SET_CODE))
        .into_tuple::<(i32, String, String)>()
        .all(db)
        .await?)
}

/// The booster-membership cards `(card_id, foil)` each product currently carries, indexed by
/// product id — the base both booster-pool synthesizers read. Built once over the resolved
/// rows; a product with no booster row simply isn't a key.
fn booster_pool_by_product(rows: &HashSet<Row>) -> HashMap<i32, Vec<(i32, bool)>> {
    let booster = sealed_content::Membership::Booster.as_str();
    let mut pools: HashMap<i32, Vec<(i32, bool)>> = HashMap::new();
    for &(product_id, card_id, membership, foil) in rows {
        if membership == booster {
            pools.entry(product_id).or_default().push((card_id, foil));
        }
    }
    pools
}

/// Give a product the booster pull-pools of the boosters it **contains** — a bundle / gift box
/// wrapping a play + collector booster inherits both boosters' `booster`-membership cards as
/// its own, so its page shows "Can be pulled from boosters" (issue #290). Applied only to a
/// parent with **no** booster rows of its own: MTGJSON's native `sealed` recursion already
/// gives most bundles their pool, so this fills only the ones whose contents MTGJSON shipped
/// as `null` (composed just by the fallback, e.g. Avatar's Commander's Bundle). One level deep
/// — direct `sealed` component children with a resolved child-product link. Pure + in-memory
/// over the resolved membership + component rows; returns the number of new rows added.
pub(super) fn merge_contained_booster_pools(
    rows: &mut HashSet<Row>,
    components: &[ComponentRow],
) -> usize {
    // Snapshot the pools (and thus which parents already have one) *before* adding anything,
    // so every eligible parent inherits from ALL its booster children — adding the play pool
    // mustn't make the parent look "covered" and skip its collector pool.
    let pools = booster_pool_by_product(rows);
    let sealed_kind = sealed_component::ComponentKind::Sealed.as_str();
    let booster = sealed_content::Membership::Booster.as_str();
    let mut added = 0;
    for component in components {
        // Only a `sealed` child with a resolved product link is a contained sub-product.
        if component.kind != sealed_kind {
            continue;
        }
        let Some(child_id) = component.child_product_id else {
            continue;
        };
        // The parent already carries a booster pool (its own, from MTGJSON) — leave it be.
        if pools.contains_key(&component.product_id) {
            continue;
        }
        let Some(child_pool) = pools.get(&child_id) else {
            continue; // the child isn't a resolved booster / has no pool to inherit
        };
        for &(card_id, foil) in child_pool {
            if rows.insert((component.product_id, card_id, booster, foil)) {
                added += 1;
            }
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: synthesized contained-booster pull pools");
    }
    added
}

/// Give a booster **variant** with no pool of its own the pull-pool of its canonical sibling —
/// a Sleeved Play Booster Pack, a language variant, or any booster product MTGJSON never
/// modelled inherits the fullest same-set / same-family booster's cards, so it stops rendering
/// an empty "cards in this product" (issue #290). Grouped by set code + booster family (a
/// family's pack and box forms share one pool); the canonical is the sibling with the largest
/// pool. Applied only to a product with **no** booster rows, so a variant carrying its own
/// (smaller, curated) pool — a Sample / Jumpstart pack — keeps it. Returns the rows added.
pub(super) async fn merge_sibling_booster_pools(
    db: &DatabaseConnection,
    rows: &mut HashSet<Row>,
) -> Result<usize, MtgjsonError> {
    use crate::tcgcsv::classify::booster_family;

    // Every product's (id, set_code, product_type) for the game — the grouping keys.
    let meta: Vec<(i32, String, String)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::SetCode)
        .column(product::Column::ProductType)
        .filter(product::Column::Game.eq(GAME))
        .into_tuple()
        .all(db)
        .await?;

    // Group only the booster products by (set_code, family) — a family's representative slug
    // folds its pack + box forms together. Non-boosters (bundles, decks, …) are handled by
    // contained inheritance and never seed a sibling group.
    let mut groups: HashMap<(String, &'static str), Vec<i32>> = HashMap::new();
    for (id, set_code, product_type) in &meta {
        if let Some(family) = booster_family(product_type) {
            groups
                .entry((set_code.clone(), family.representative_type()))
                .or_default()
                .push(*id);
        }
    }

    let pools = booster_pool_by_product(rows);
    let booster = sealed_content::Membership::Booster.as_str();
    let mut added = 0;
    for members in groups.values() {
        // The canonical sibling = the member with the largest pool. Skip the group when every
        // member is empty (an entirely unmodelled set — nothing to inherit from).
        let Some(&canonical) = members.iter().max_by_key(|&&id| pools.get(&id).map_or(0, |p| p.len()))
        else {
            continue;
        };
        let Some(canonical_pool) = pools.get(&canonical).filter(|p| !p.is_empty()) else {
            continue;
        };
        for &member in members {
            // Fill only genuinely-empty variants; a sibling with its own pool keeps it.
            if pools.contains_key(&member) {
                continue;
            }
            for &(card_id, foil) in canonical_pool {
                if rows.insert((member, card_id, booster, foil)) {
                    added += 1;
                }
            }
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: synthesized sibling booster pull pools");
    }
    Ok(added)
}
