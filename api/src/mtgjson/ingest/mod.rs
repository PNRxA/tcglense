//! Rebuild of the `sealed_contents` table from MTGJSON's `AllPrintings.json`.
//!
//! The flow mirrors [`crate::tcgcsv::ingest`]'s shape (version gate, `ingest_state`
//! bookkeeping, batched writes): fetch the file conditionally (skip on a `304`), resolve
//! every sealed product's contents into per-card membership rows ([`model::build_memberships`]),
//! map the external ids (TCGplayer product id -> `products.id`, Scryfall id ->
//! `cards.id`) onto our catalog, and **replace** the game's rows in one transaction so
//! stale membership never lingers. Version-gated on the file's HTTP `ETag` (stored in the
//! `(mtg, mtgjson_sealed_contents)` `ingest_state` row), so an unchanged file costs one
//! conditional request.
//!
//! Only products that resolve to our `products` table (by `tcgplayerProductId`) and cards
//! that resolve to our `cards` table (by Scryfall id) get rows; the rest are skipped and
//! tallied. Cross-set references whose card isn't in our catalog, and any product not on
//! TCGplayer, simply don't appear.
//!
//! After the MTGJSON pass, curated [`fallback`](super::fallback) memberships are merged in
//! for any product MTGJSON left empty (its cards would otherwise show no sealed product),
//! and the stored version couples the file's `ETag` with the fallback snapshot's content
//! hash (see [`compose_version`]) so editing the fallback data rebuilds on the next sync
//! even when `AllPrintings.json` is byte-identical.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, TransactionTrait,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use super::client::{FetchOutcome, fetch_all_printings};
use super::fallback;
use super::model::{self, RawComponent, RawMembership};
use super::progress::SyncProgress;
use super::sld;
use super::{DATASET, GAME, MtgjsonError};
use crate::catalog::ingest_state::{self, StateFields};
use crate::entities::prelude::{SealedComponent, SealedContent};
use crate::entities::{sealed_component, sealed_content};

mod merge;
mod resolve;

use merge::*;
use resolve::*;

/// Rows per external-id `IN` lookup — under SQLite's 32 766 bound-parameter limit.
pub(super) const IN_CHUNK: usize = 900;

/// Rows per membership insert. Eight columns, so ~2000 rows ≈ 16k binds — under the limit.
pub(super) const INSERT_BATCH: usize = 2000;

/// A resolved-to-internal-ids membership row: `(product_id, card_id, membership, foil)`.
/// Both the MTGJSON pass and the fallback merge accumulate into a `HashSet<Row>`, which
/// deduplicates across the two sources for free.
pub(super) type Row = (i32, i32, &'static str, bool);

/// A resolved-to-internal-ids composition row, ready to insert into `sealed_components`.
/// The MTGJSON pass and the fallback merge both accumulate into a `Vec<ComponentRow>`
/// (ordered by `position` within each product; not deduplicated — position is identity).
pub(super) struct ComponentRow {
    pub(super) product_id: i32,
    pub(super) position: i32,
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) quantity: i32,
    pub(super) child_product_id: Option<i32>,
    pub(super) child_card_id: Option<i32>,
}

/// Separator between MTGJSON's `ETag` and the fallback content hash in the stored
/// `ingest_state.source_updated_at`. A US control byte can't occur in an HTTP `ETag`
/// (RFC 9110 `etagc` excludes control chars), so splitting on it is unambiguous.
const VERSION_SEP: char = '\u{1f}';

/// Version tag for the **derived** booster-pool synthesis ([`merge_contained_booster_pools`]
/// + [`merge_sibling_booster_pools`]). Folded into the stored version (see
/// [`compose_version`]) so that changing this derivation forces a one-off rebuild even when
/// `AllPrintings.json`, the fallback data, and the SLD inputs are all byte-identical — the
/// only way a pure code change takes effect, since the sync is otherwise ETag-gated. Bump it
/// whenever the synthesis logic changes.
const DERIVATION_VERSION: &str = "booster-pool-1";

/// Sync MTG sealed-product memberships from MTGJSON, recording status in `ingest_state`.
/// On error the state row is best-effort marked `"error"` (so the next tick retries) and
/// the error is returned for the caller to log.
pub async fn refresh(
    db: &DatabaseConnection,
    http: &Client,
    source: &crate::datasets::SyncSource,
) -> Result<(), MtgjsonError> {
    match refresh_inner(db, http, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest_state::mark_error(db, GAME, DATASET, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    http: &Client,
    source: &crate::datasets::SyncSource,
) -> Result<(), MtgjsonError> {
    let existing = ingest_state::load(db, GAME, DATASET).await?;
    // The stored version couples MTGJSON's ETag with the committed fallback file's hash
    // (see `compose_version`), so a fallback-data edit forces a rebuild even when
    // AllPrintings is byte-identical.
    let stored = existing
        .as_ref()
        .filter(|s| s.status == "complete")
        .and_then(|s| s.source_updated_at.clone());
    let (prior_etag, prior_fallback, prior_sld, prior_derivation) =
        stored.as_deref().map(split_version).unwrap_or((None, None, None, None));
    let fallback_version = fallback::version();
    let fallback_changed = prior_fallback != Some(fallback_version);
    // The Secret Lair drop→cards derivation reads `sld_drops.json` + curated overrides;
    // hash them into the gate so regenerating that data rebuilds even if MTGJSON didn't.
    let sld_version = sld::derivation_version();
    let sld_changed = prior_sld != Some(sld_version);
    // The derived booster-pool synthesis is pure code (no data file), so its version is a
    // bumped constant; a change forces one rebuild the same way a fallback/SLD edit does.
    let derivation_changed = prior_derivation != Some(DERIVATION_VERSION);

    let progress = SyncProgress::start("checking for updates");

    // Conditional fetch: a 304 (unchanged file) short-circuits the whole rebuild — but
    // only when the fallback data and the SLD derivation inputs are also unchanged. If
    // either local source changed we must re-fetch AllPrintings to rebuild the merged
    // table, so skip the conditional request. In mirror mode the file streams from the
    // mirror; upstream mode hits MTGJSON directly.
    let base_url = source.mtgjson_base_url();
    let conditional = if fallback_changed || sld_changed || derivation_changed {
        None
    } else {
        prior_etag
    };
    let (etag, all) = match fetch_all_printings(http, &base_url, conditional).await? {
        FetchOutcome::Unchanged => {
            drop(progress);
            tracing::info!("mtgjson sealed contents already up to date");
            return Ok(());
        }
        FetchOutcome::Fetched { etag, all } => (etag, all),
    };

    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET,
            status: "running",
            source_updated_at: None,
            detail: "resolving contents",
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: None,
        },
    )
    .await?;

    // Resolve contents -> per-card memberships + the structural composition off the async
    // runtime (CPU-bound over a big document). `all` is dropped when the closure returns,
    // freeing the parse tree; both passes run in the one blocking task so it's moved once.
    progress.set_stage("resolving contents");
    let all = *all;
    let (memberships, components): (Vec<RawMembership>, Vec<RawComponent>) =
        tokio::task::spawn_blocking(move || {
            (model::build_memberships(&all), model::build_compositions(&all))
        })
        .await
        .map_err(|err| MtgjsonError::Join(err.to_string()))?;
    tracing::info!(
        memberships = memberships.len(),
        components = components.len(),
        "mtgjson: resolved membership + composition rows"
    );

    // Map external ids onto our catalog: products for membership targets, component parents,
    // and the sub-products components link to; cards for membership targets + linked promos.
    let product_ext: Vec<String> = distinct(
        memberships
            .iter()
            .map(|m| &m.tcgplayer_product_id)
            .chain(components.iter().map(|c| &c.tcgplayer_product_id))
            .chain(components.iter().filter_map(|c| c.child_tcgplayer_product_id.as_ref())),
    );
    let card_ext: Vec<String> = distinct(
        memberships
            .iter()
            .map(|m| &m.scryfall_id)
            .chain(components.iter().filter_map(|c| c.child_scryfall_id.as_ref())),
    );
    progress.set_stage("matching to catalog");
    let products = resolve_products(db, &product_ext).await?;
    let cards = resolve_cards(db, &card_ext).await?;

    // Resolve MTGJSON memberships whose product AND card are both in our catalog into a
    // deduplicated row set, tracking which products MTGJSON actually described.
    let mut rows: HashSet<Row> = HashSet::new();
    let mut mtgjson_products: HashSet<i32> = HashSet::new();
    // Products MTGJSON gave a `variable` ("may be in") row of its own — the curated Secret Lair
    // bonus-pool derivation steps aside for these, so an upstream-authored pool always wins.
    let mut mtgjson_variable_products: HashSet<i32> = HashSet::new();
    for m in &memberships {
        if let (Some(&product_id), Some(&card_id)) =
            (products.get(&m.tcgplayer_product_id), cards.get(&m.scryfall_id))
        {
            rows.insert((product_id, card_id, m.membership, m.foil));
            mtgjson_products.insert(product_id);
            if m.membership == sealed_content::Membership::Variable.as_str() {
                mtgjson_variable_products.insert(product_id);
            }
        }
    }
    let from_mtgjson = rows.len();

    // Fill products MTGJSON left empty with curated fallback memberships.
    let from_fallback = merge_fallback(db, fallback::data(), &mtgjson_products, &mut rows).await?;

    // Derive card contents for Secret Lair drop products still empty after MTGJSON +
    // fallback (a drop's cards come from `sld_drops.json`, matched by product name), under
    // the same "only when nothing described it" gate — so upstream stays authoritative.
    let covered: HashSet<i32> = rows.iter().map(|&(pid, ..)| pid).collect();
    let from_sld = merge_sld_derived(db, &covered, &mut rows).await?;

    // Attach each Secret Lair drop's random bonus-card pool to every product of the drop as
    // `variable` ("may be in") — the "Bonus card unknown" cards MTGJSON never names (e.g. Avatar's
    // Command Tower / Fellwar Stone, one per drop at random). Unlike the drop-cards derivation this
    // is a distinct axis (not a product's own contents), so it is *not* gated on `covered` — it
    // stays linked even after MTGJSON authors the drop's deck — and it steps aside for a product
    // MTGJSON gave its own `variable` row, so an upstream-authored pool wins.
    let from_sld_bonus = merge_sld_bonus_cards(db, &mtgjson_variable_products, &mut rows).await?;

    // Resolve the composition rows (positions assigned per resolved product id, so a
    // duplicate-tcgId product's sequences union in order rather than colliding on the unique
    // key), and note which products MTGJSON gave a composition so the fallback fills gaps.
    let (mut component_rows, mtgjson_component_products) =
        resolve_component_rows(&components, &products, &cards);
    let components_from_mtgjson = component_rows.len();

    // Fill products MTGJSON left without a composition with curated fallback components.
    let components_from_fallback = merge_fallback_components(
        db,
        fallback::data(),
        &mtgjson_component_products,
        &mut component_rows,
    )
    .await?;

    // Derive booster pull-pools for products that contain boosters but carry none of their
    // own (both gated on "no own booster rows", so MTGJSON / the fallback stay authoritative):
    //   - a bundle / gift box inherits the pools of the boosters it *wraps* (its `sealed`
    //     components) — the play + collector pulls a bundle offers (issue #290). Purely
    //     in-memory over the just-resolved membership + component rows.
    //   - a booster *variant* with no pool of its own (a Sleeved Play Booster Pack, a
    //     language variant) inherits its canonical same-set/same-family sibling's pool.
    let from_contained = merge_contained_booster_pools(&mut rows, &component_rows);
    let from_siblings = merge_sibling_booster_pools(db, &mut rows).await?;

    // Materialise both merged row sets as models.
    let now = Utc::now();
    let models = rows_to_models(&rows, now);
    let component_models = components_to_models(&component_rows, now);
    let matched = models.len();
    let component_count = component_models.len();
    let product_count = rows.iter().map(|&(pid, ..)| pid).collect::<HashSet<i32>>().len();
    progress.set_rows("writing", (matched + component_count) as u64);

    // Replace the game's rows in one transaction so a reader never sees a half-rebuilt table
    // and stale membership / composition can't survive a product's contents changing.
    let txn = db.begin().await?;
    SealedContent::delete_many()
        .filter(sealed_content::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<sealed_content::ActiveModel> = iter.by_ref().take(INSERT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        // do_nothing on conflict is belt-and-braces: build_memberships already dedupes,
        // and the table was just cleared, so a conflict shouldn't occur.
        SealedContent::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([
                    sealed_content::Column::Game,
                    sealed_content::Column::ProductId,
                    sealed_content::Column::CardId,
                    sealed_content::Column::Membership,
                    sealed_content::Column::Foil,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&txn)
            .await?;
    }
    // Rebuild the composition table alongside the memberships, in the same transaction.
    SealedComponent::delete_many()
        .filter(sealed_component::Column::Game.eq(GAME))
        .exec(&txn)
        .await?;
    let mut component_iter = component_models.into_iter();
    loop {
        let chunk: Vec<sealed_component::ActiveModel> =
            component_iter.by_ref().take(INSERT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        // do_nothing on conflict is belt-and-braces: positions are unique per product by
        // construction, and the table was just cleared.
        SealedComponent::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([
                    sealed_component::Column::Game,
                    sealed_component::Column::ProductId,
                    sealed_component::Column::Position,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&txn)
            .await?;
    }
    txn.commit().await?;

    drop(progress);
    let version = compose_version(etag.as_deref(), fallback_version, sld_version, DERIVATION_VERSION);
    let detail = format!(
        "{matched} memberships across {product_count} products \
         ({from_mtgjson} from mtgjson, {from_fallback} from fallback, {from_sld} from sld drops, \
         {from_sld_bonus} from sld bonus pools, \
         {from_contained} from contained boosters, {from_siblings} from sibling boosters); \
         {component_count} components ({components_from_mtgjson} from mtgjson, \
         {components_from_fallback} from fallback)"
    );
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: DATASET,
            status: "complete",
            source_updated_at: Some(&version),
            detail: &detail,
            sets_imported: product_count as i32,
            cards_imported: matched as i32,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(
        memberships = matched,
        components = component_count,
        products = product_count,
        fallback = from_fallback,
        sld_derived = from_sld,
        sld_bonus_pools = from_sld_bonus,
        contained_boosters = from_contained,
        sibling_boosters = from_siblings,
        fallback_components = components_from_fallback,
        "mtgjson sealed contents sync complete"
    );
    Ok(())
}

/// Materialise a resolved row set as insertable models, all stamped `now`.
fn rows_to_models(rows: &HashSet<Row>, now: DateTimeUtc) -> Vec<sealed_content::ActiveModel> {
    rows.iter()
        .map(|&(product_id, card_id, membership, foil)| sealed_content::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            product_id: Set(product_id),
            card_id: Set(card_id),
            membership: Set(membership.to_string()),
            foil: Set(foil),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .collect()
}

/// Resolve raw composition components to insertable rows, mapping each component's parent
/// (required) and optional child product / card external ids to internal ids. `position` is
/// a running counter **per resolved product id** — not the source order — so two MTGJSON
/// `sealedProduct` entries that resolve to one `product_id` (a shared/duplicate TCGplayer
/// id) can't collide on the unique `(game, product_id, position)` key: their components
/// union in order (as the membership pass's `HashSet` does) rather than one silently
/// dropping. Returns the rows plus the set of products MTGJSON described (the fallback gate).
fn resolve_component_rows(
    components: &[RawComponent],
    products: &HashMap<String, i32>,
    cards: &HashMap<String, i32>,
) -> (Vec<ComponentRow>, HashSet<i32>) {
    let mut rows = Vec::new();
    let mut covered = HashSet::new();
    let mut next_position: HashMap<i32, i32> = HashMap::new();
    for c in components {
        let Some(&product_id) = products.get(&c.tcgplayer_product_id) else {
            continue;
        };
        covered.insert(product_id);
        let child_product_id = c
            .child_tcgplayer_product_id
            .as_ref()
            .and_then(|id| products.get(id).copied());
        let child_card_id = c.child_scryfall_id.as_ref().and_then(|id| cards.get(id).copied());
        let position = next_position.entry(product_id).or_insert(0);
        rows.push(ComponentRow {
            product_id,
            position: *position,
            kind: c.kind.to_string(),
            name: c.name.clone(),
            quantity: c.quantity,
            child_product_id,
            child_card_id,
        });
        *position += 1;
    }
    (rows, covered)
}

/// Materialise resolved composition rows as insertable models, all stamped `now`.
fn components_to_models(
    rows: &[ComponentRow],
    now: DateTimeUtc,
) -> Vec<sealed_component::ActiveModel> {
    rows.iter()
        .map(|r| sealed_component::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            product_id: Set(r.product_id),
            position: Set(r.position),
            kind: Set(r.kind.clone()),
            name: Set(r.name.clone()),
            quantity: Set(r.quantity),
            child_product_id: Set(r.child_product_id),
            child_card_id: Set(r.child_card_id),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .collect()
}

/// Compose the stored version string from MTGJSON's `ETag`, the fallback content hash, the
/// SLD-derivation hash, and the booster-pool derivation tag, joined by [`VERSION_SEP`].
fn compose_version(etag: Option<&str>, fallback: &str, sld: &str, derivation: &str) -> String {
    format!(
        "{}{VERSION_SEP}{fallback}{VERSION_SEP}{sld}{VERSION_SEP}{derivation}",
        etag.unwrap_or("")
    )
}

/// Split a stored version back into `(mtgjson_etag, fallback_hash, sld_hash,
/// derivation_tag)`. A value written before a given field existed simply parses that field
/// (and any after it) as `None`, so the first sync after an upgrade sees a change and
/// rebuilds once: a bare pre-feature ETag -> `(Some, None, None, None)`; a three-part
/// pre-derivation version -> `(Some, Some, Some, None)`.
fn split_version(stored: &str) -> (Option<&str>, Option<&str>, Option<&str>, Option<&str>) {
    let mut parts = stored.split(VERSION_SEP);
    let etag = parts.next().and_then(non_empty);
    let fallback = parts.next().and_then(non_empty);
    let sld = parts.next().and_then(non_empty);
    let derivation = parts.next().and_then(non_empty);
    (etag, fallback, sld, derivation)
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::super::fallback::{FallbackCard, FallbackComponent, FallbackData, FallbackProduct};
    use super::merge::*;
    use super::resolve::*;
    use super::*;
    use crate::entities::prelude::{SealedComponent, SealedContent};
    use crate::entities::{card, product};
    use crate::test_support::{insert_card, migrated_memory_db};
    use sea_orm::{PaginatorTrait, QueryOrder};

    /// Insert a product row and return its id (products carry only an external id + name).
    async fn insert_product(db: &DatabaseConnection, external_id: &str) -> i32 {
        let now = Utc::now();
        let model = product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Product {external_id}")),
            clean_name: Set(None),
            set_code: Set("set".to_string()),
            product_type: Set("bundle".to_string()),
            url: Set(None),
            image_url: Set(None),
            price_usd: Set(None),
            price_usd_foil: Set(None),
            released_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        product::Entity::insert(model)
            .exec(db)
            .await
            .unwrap()
            .last_insert_id
    }

    /// Insert a card at a specific `(set_code, collector_number)` and return its id (the
    /// fallback path resolves by set + number, not by Scryfall id).
    async fn insert_card_at(db: &DatabaseConnection, ext: &str, set: &str, number: &str) -> i32 {
        let now = Utc::now();
        let model = card::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(ext.to_string()),
            name: Set(format!("Card {ext}")),
            set_code: Set(set.to_string()),
            set_name: Set(set.to_uppercase()),
            collector_number: Set(number.to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        card::Entity::insert(model).exec(db).await.unwrap().last_insert_id
    }

    /// Build a one-product fallback dataset for the merge tests.
    fn one_product(tcg: &str, contents: Vec<FallbackCard>) -> FallbackData {
        FallbackData {
            products: vec![FallbackProduct {
                tcgplayer_product_id: tcg.to_string(),
                name: format!("Product {tcg}"),
                contents,
                components: Vec::new(),
            }],
        }
    }

    /// Build a one-product fallback dataset with authored composition components.
    fn one_product_components(tcg: &str, components: Vec<FallbackComponent>) -> FallbackData {
        FallbackData {
            products: vec![FallbackProduct {
                tcgplayer_product_id: tcg.to_string(),
                name: format!("Product {tcg}"),
                contents: Vec::new(),
                components,
            }],
        }
    }

    fn fb_component(
        kind: &str,
        name: &str,
        quantity: i32,
        child_product: Option<&str>,
    ) -> FallbackComponent {
        FallbackComponent {
            kind: kind.to_string(),
            name: name.to_string(),
            quantity,
            child_tcgplayer_product_id: child_product.map(str::to_string),
            child_set: None,
            child_number: None,
        }
    }

    fn fb_card(set: &str, number: &str, membership: &str, foil: bool) -> FallbackCard {
        FallbackCard {
            set: set.to_string(),
            number: number.to_string(),
            name: format!("{set} {number}"),
            membership: membership.to_string(),
            foil,
        }
    }

    /// `resolve_cards_by_setnum` indexes `(set, number)` -> id, lowercasing the set key.
    #[tokio::test]
    async fn resolve_cards_by_setnum_indexes_pairs() {
        let db = migrated_memory_db().await;
        let sol = insert_card_at(&db, "sf-sol", "tle", "316").await;
        insert_card_at(&db, "sf-swat", "tle", "311").await;
        let map = resolve_cards_by_setnum(&db, &[("tle".into(), "316".into())]).await.unwrap();
        assert_eq!(map.get(&("tle".to_string(), "316".to_string())), Some(&sol));
    }

    /// A fallback product MTGJSON left empty is merged in (resolving product + card).
    #[tokio::test]
    async fn merge_fallback_fills_empty_product() {
        let db = migrated_memory_db().await;
        let sol = insert_card_at(&db, "sf-sol", "tle", "316").await;
        let swat = insert_card_at(&db, "sf-swat", "tle", "311").await;
        let product = insert_product(&db, "648686").await;
        let data = one_product(
            "648686",
            vec![
                fb_card("tle", "316", "contains", false),
                fb_card("tle", "311", "variable", false),
            ],
        );

        let mut rows = HashSet::new();
        let added = merge_fallback(&db, &data, &HashSet::new(), &mut rows).await.unwrap();
        assert_eq!(added, 2);
        assert!(rows.contains(&(product, sol, "contains", false)));
        assert!(rows.contains(&(product, swat, "variable", false)));
    }

    /// MTGJSON stays authoritative: a product it already described takes no fallback rows.
    #[tokio::test]
    async fn merge_fallback_skips_covered_product() {
        let db = migrated_memory_db().await;
        insert_card_at(&db, "sf-sol", "tle", "316").await;
        let product = insert_product(&db, "648686").await;
        let data = one_product("648686", vec![fb_card("tle", "316", "contains", false)]);

        let mut rows = HashSet::new();
        let covered = HashSet::from([product]);
        let added = merge_fallback(&db, &data, &covered, &mut rows).await.unwrap();
        assert_eq!(added, 0);
        assert!(rows.is_empty());
    }

    /// An unresolved product or card (not in our catalog) is skipped, not fatal.
    #[tokio::test]
    async fn merge_fallback_skips_unresolved() {
        let db = migrated_memory_db().await;
        // Product exists but the card is absent from the catalog.
        insert_product(&db, "648686").await;
        let data = one_product("648686", vec![fb_card("tle", "316", "contains", false)]);
        let mut rows = HashSet::new();
        assert_eq!(merge_fallback(&db, &data, &HashSet::new(), &mut rows).await.unwrap(), 0);
        assert!(rows.is_empty());

        // Card exists but the product is not on TCGplayer / not in the catalog.
        insert_card_at(&db, "sf-sol", "tle", "316").await;
        let data = one_product("999999", vec![fb_card("tle", "316", "contains", false)]);
        assert_eq!(merge_fallback(&db, &data, &HashSet::new(), &mut rows).await.unwrap(), 0);
        assert!(rows.is_empty());
    }

    /// Positions are numbered **per resolved product id**, so two component sequences that
    /// resolve to the same `product_id` (a duplicate TCGplayer id across `sealedProduct`
    /// entries) union with distinct positions instead of colliding/dropping on the unique key.
    #[test]
    fn resolve_component_rows_numbers_positions_per_resolved_product() {
        let products = HashMap::from([("100".to_string(), 42), ("200".to_string(), 7)]);
        let cards: HashMap<String, i32> = HashMap::new();
        let raw = |tcg: &str, kind: &'static str, name: &str| RawComponent {
            tcgplayer_product_id: tcg.to_string(),
            kind,
            name: name.to_string(),
            quantity: 1,
            child_tcgplayer_product_id: None,
            child_scryfall_id: None,
        };
        // Product 100 (id 42) appears in two separate runs (as if a duplicate tcgId), with
        // product 200 (id 7) between them; an unresolved product ("999") is skipped.
        let components = vec![
            raw("100", "sealed", "A"),
            raw("100", "other", "B"),
            raw("200", "sealed", "C"),
            raw("999", "other", "skipped"),
            raw("100", "other", "D"),
        ];
        let (rows, covered) = resolve_component_rows(&components, &products, &cards);
        assert_eq!(covered, HashSet::from([42, 7]));
        // Product 42 keeps all three components, positions 0/1/2 (union, no collision/drop).
        let mut p42: Vec<(i32, &str)> = rows
            .iter()
            .filter(|r| r.product_id == 42)
            .map(|r| (r.position, r.name.as_str()))
            .collect();
        p42.sort();
        assert_eq!(p42, vec![(0, "A"), (1, "B"), (2, "D")]);
        // Product 7 is numbered independently from 0.
        let p7: Vec<(i32, &str)> = rows
            .iter()
            .filter(|r| r.product_id == 7)
            .map(|r| (r.position, r.name.as_str()))
            .collect();
        assert_eq!(p7, vec![(0, "C")]);
    }

    /// A fallback composition fills a product MTGJSON left empty, resolving a `sealed`
    /// component's child-product link and keeping an unresolved one as text; positions are
    /// assigned contiguously from 0.
    #[tokio::test]
    async fn merge_fallback_components_fills_and_links() {
        let db = migrated_memory_db().await;
        let bundle = insert_product(&db, "648686").await;
        let play = insert_product(&db, "648640").await;
        let data = one_product_components(
            "648686",
            vec![
                fb_component("sealed", "Play Booster", 9, Some("648640")),
                fb_component("other", "Card storage box", 1, None),
                // A sealed component whose child isn't in the catalog: kept, link null.
                fb_component("sealed", "Mystery Pack", 1, Some("999999")),
            ],
        );

        let mut rows = Vec::new();
        let added = merge_fallback_components(&db, &data, &HashSet::new(), &mut rows)
            .await
            .unwrap();
        assert_eq!(added, 3);
        assert_eq!(rows.iter().map(|r| r.position).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(rows[0].product_id, bundle);
        assert_eq!(rows[0].child_product_id, Some(play));
        assert_eq!(rows[0].quantity, 9);
        assert_eq!(rows[2].child_product_id, None, "unresolved child stays textual");
    }

    /// A `card` composition component links by `(set, number)` like the membership path.
    #[tokio::test]
    async fn merge_fallback_components_links_card_by_setnum() {
        let db = migrated_memory_db().await;
        insert_product(&db, "648686").await;
        let sol = insert_card_at(&db, "sf-sol", "tle", "316").await;
        let mut card = fb_component("card", "Sol Ring", 1, None);
        card.child_set = Some("tle".to_string());
        card.child_number = Some("316".to_string());
        let data = one_product_components("648686", vec![card]);

        let mut rows = Vec::new();
        merge_fallback_components(&db, &data, &HashSet::new(), &mut rows)
            .await
            .unwrap();
        assert_eq!(rows[0].child_card_id, Some(sol));
    }

    /// MTGJSON stays authoritative for composition too: a product it already described
    /// takes no fallback components.
    #[tokio::test]
    async fn merge_fallback_components_skips_covered_product() {
        let db = migrated_memory_db().await;
        let bundle = insert_product(&db, "648686").await;
        let data = one_product_components("648686", vec![fb_component("sealed", "Play Booster", 9, None)]);

        let mut rows = Vec::new();
        let covered = HashSet::from([bundle]);
        let added = merge_fallback_components(&db, &data, &covered, &mut rows)
            .await
            .unwrap();
        assert_eq!(added, 0);
        assert!(rows.is_empty());
    }

    /// The **shipped** fallback composition resolves against a catalog: seed the Avatar
    /// Beginner Box + its case, run the real merge, and confirm the case links the box (3×).
    /// Guards the committed data + the resolution path together — a typo'd child tcgid would
    /// silently drop the link, which the pure fallback-data test can't catch.
    #[tokio::test]
    async fn shipped_fallback_components_resolve_against_the_catalog() {
        let db = migrated_memory_db().await;
        let beginner = insert_product(&db, "648682").await;
        let case = insert_product(&db, "662272").await;

        let mut rows = Vec::new();
        let added = merge_fallback_components(&db, fallback::data(), &HashSet::new(), &mut rows)
            .await
            .unwrap();
        assert!(added > 0, "the shipped fallback authored components");
        // The Beginner Box Case (662272) lists 3× Beginner Box, linked to the box product.
        let case_rows: Vec<&ComponentRow> = rows.iter().filter(|r| r.product_id == case).collect();
        assert_eq!(case_rows.len(), 1);
        assert_eq!(case_rows[0].quantity, 3);
        assert_eq!(case_rows[0].child_product_id, Some(beginner));
    }

    /// The **shipped** Avatar Commander's Bundle composition links its three guaranteed
    /// borderless staples to the catalog cards (tle 315-317) and keeps the randomised
    /// 2-of-10 pool as a textual line — so a typo'd set/number in the committed data (which
    /// would silently drop a card link) fails CI, not just at runtime. Mirrors
    /// [`shipped_fallback_components_resolve_against_the_catalog`] for the `card` link path.
    #[tokio::test]
    async fn shipped_avatar_bundle_links_guaranteed_cards() {
        let db = migrated_memory_db().await;
        insert_product(&db, "648686").await; // the Commander's Bundle
        // The guaranteed three, at their borderless (set, number).
        let signet = insert_card_at(&db, "sf-signet", "tle", "315").await;
        let sol = insert_card_at(&db, "sf-sol", "tle", "316").await;
        let boots = insert_card_at(&db, "sf-boots", "tle", "317").await;

        let mut rows = Vec::new();
        merge_fallback_components(&db, fallback::data(), &HashSet::new(), &mut rows)
            .await
            .unwrap();

        // Every `card` component the bundle authored, in position order.
        let cards: Vec<&ComponentRow> = {
            let bundle = rows.iter().find(|r| r.name == "Sol Ring").map(|r| r.product_id);
            let mut v: Vec<&ComponentRow> = rows
                .iter()
                .filter(|r| Some(r.product_id) == bundle && r.kind == "card")
                .collect();
            v.sort_by_key(|r| r.position);
            v
        };
        // Three guaranteed cards resolve their links, one each, in staple order.
        assert_eq!(
            cards
                .iter()
                .take(3)
                .map(|r| (r.child_card_id, r.quantity))
                .collect::<Vec<_>>(),
            vec![(Some(signet), 1), (Some(sol), 1), (Some(boots), 1)],
        );
        // The randomised pool is one textual card line: quantity 2, no card link.
        let pool = cards.last().expect("a pool line");
        assert_eq!(pool.quantity, 2);
        assert_eq!(pool.child_card_id, None, "the 2-of-10 pool stays unlinked");
    }

    /// The component write path: delete-then-insert replaces (not duplicates) the game's
    /// rows, and round-trips every column in `position` order.
    #[tokio::test]
    async fn components_write_replaces_and_round_trips() {
        let db = migrated_memory_db().await;
        let bundle = insert_product(&db, "648686").await;
        let play = insert_product(&db, "648640").await;
        let rows = vec![
            ComponentRow {
                product_id: bundle,
                position: 0,
                kind: "sealed".to_string(),
                name: "Play Booster".to_string(),
                quantity: 9,
                child_product_id: Some(play),
                child_card_id: None,
            },
            ComponentRow {
                product_id: bundle,
                position: 1,
                kind: "other".to_string(),
                name: "Storage box".to_string(),
                quantity: 1,
                child_product_id: None,
                child_card_id: None,
            },
        ];

        write_components_for_test(&db, &rows).await;
        let stored = SealedComponent::find()
            .order_by_asc(sealed_component::Column::Position)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].kind, "sealed");
        assert_eq!(stored[0].name, "Play Booster");
        assert_eq!(stored[0].quantity, 9);
        assert_eq!(stored[0].child_product_id, Some(play));
        assert_eq!(stored[1].kind, "other");

        // Re-run replaces rather than duplicating (the transaction wipes first).
        write_components_for_test(&db, &rows).await;
        assert_eq!(SealedComponent::find().count(&db).await.unwrap(), 2);
    }

    /// Drives the composition delete + insert without the network fetch, mirroring the
    /// `sealed_components` write inside `refresh_inner`.
    async fn write_components_for_test(db: &DatabaseConnection, rows: &[ComponentRow]) {
        let models = components_to_models(rows, Utc::now());
        let txn = db.begin().await.unwrap();
        SealedComponent::delete_many()
            .filter(sealed_component::Column::Game.eq(GAME))
            .exec(&txn)
            .await
            .unwrap();
        if !models.is_empty() {
            SealedComponent::insert_many(models)
                .exec_without_returning(&txn)
                .await
                .unwrap();
        }
        txn.commit().await.unwrap();
    }

    /// The composite version round-trips, and a version written before a field existed
    /// reads that field (and any after it) as `None`, so the first post-upgrade sync
    /// rebuilds: a bare ETag -> `(Some, None, None)`; a two-part pre-SLD version ->
    /// `(Some, Some, None)`.
    #[test]
    fn version_composition_round_trips() {
        let composed = compose_version(Some("\"etag-123\""), "fbhash", "sldhash", "dvtag");
        assert_eq!(
            split_version(&composed),
            (Some("\"etag-123\""), Some("fbhash"), Some("sldhash"), Some("dvtag"))
        );
        // A pre-feature bare ETag: nothing after it recorded.
        assert_eq!(split_version("\"legacy-etag\""), (Some("\"legacy-etag\""), None, None, None));
        // A three-part version from before the derivation field: it reads as absent -> rebuild once.
        let three_part = format!("\"etag\"{VERSION_SEP}fbhash{VERSION_SEP}sldhash");
        assert_eq!(
            split_version(&three_part),
            (Some("\"etag\""), Some("fbhash"), Some("sldhash"), None)
        );
        assert_eq!(
            split_version(&compose_version(None, "fb", "sld", "dv")),
            (None, Some("fb"), Some("sld"), Some("dv"))
        );
    }

    // ---- Derived booster-pool synthesis (contained + sibling) ----

    fn boosters_of(rows: &HashSet<Row>, product_id: i32) -> Vec<(i32, bool)> {
        let mut v: Vec<(i32, bool)> = rows
            .iter()
            .filter(|&&(p, _, m, _)| p == product_id && m == "booster")
            .map(|&(_, c, _, f)| (c, f))
            .collect();
        v.sort_unstable();
        v
    }

    fn sealed_child(product_id: i32, child_product_id: i32) -> ComponentRow {
        ComponentRow {
            product_id,
            position: 0,
            kind: "sealed".to_string(),
            name: "Booster".to_string(),
            quantity: 1,
            child_product_id: Some(child_product_id),
            child_card_id: None,
        }
    }

    /// A bundle with no booster rows inherits the pools of every booster it wraps (play +
    /// collector), carrying each card's foil finish; a card it also guarantees stays put.
    #[test]
    fn contained_booster_pools_inherit_from_all_children() {
        let bundle = 100;
        let play = 101;
        let collector = 102;
        let mut rows: HashSet<Row> = HashSet::from([
            // The bundle's own guaranteed card (not a booster row) — must be left untouched.
            (bundle, 1, "contains", false),
            // Play booster pool.
            (play, 10, "booster", false),
            (play, 11, "booster", false),
            // Collector booster pool (one foil).
            (collector, 20, "booster", true),
        ]);
        let components = vec![sealed_child(bundle, play), sealed_child(bundle, collector)];

        let added = merge_contained_booster_pools(&mut rows, &components);
        assert_eq!(added, 3, "3 booster cards inherited (2 play + 1 collector)");
        assert_eq!(boosters_of(&rows, bundle), vec![(10, false), (11, false), (20, true)]);
        // The children and the bundle's own guarantee are unchanged.
        assert!(rows.contains(&(bundle, 1, "contains", false)));
    }

    /// A parent that already carries its own booster pool (MTGJSON recursion) is left as-is.
    #[test]
    fn contained_booster_pools_skip_a_parent_with_its_own_pool() {
        let bundle = 100;
        let collector = 102;
        let mut rows: HashSet<Row> = HashSet::from([
            (bundle, 5, "booster", false), // the bundle already has a pool
            (collector, 20, "booster", true),
        ]);
        let components = vec![sealed_child(bundle, collector)];

        let added = merge_contained_booster_pools(&mut rows, &components);
        assert_eq!(added, 0);
        assert_eq!(boosters_of(&rows, bundle), vec![(5, false)]);
    }

    /// Insert a booster product of a given type + set, returning its id.
    async fn insert_booster_product(
        db: &DatabaseConnection,
        external_id: &str,
        set_code: &str,
        product_type: &str,
    ) -> i32 {
        let now = Utc::now();
        product::Entity::insert(product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Product {external_id}")),
            clean_name: Set(None),
            set_code: Set(set_code.to_string()),
            product_type: Set(product_type.to_string()),
            url: Set(None),
            image_url: Set(None),
            price_usd: Set(None),
            price_usd_foil: Set(None),
            released_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .exec(db)
        .await
        .unwrap()
        .last_insert_id
    }

    /// A sleeved play pack (no pool) inherits its set's canonical play booster's pool, while a
    /// same-family pack that already carries its own (smaller) pool keeps it, and a
    /// different-set variant is untouched.
    #[tokio::test]
    async fn sibling_booster_pools_fill_empty_same_family_variants() {
        let db = migrated_memory_db().await;
        let pack = insert_booster_product(&db, "p-pack", "fin", "play_pack").await;
        let sleeved = insert_booster_product(&db, "p-sleeved", "fin", "play_pack").await;
        let display = insert_booster_product(&db, "p-display", "fin", "play_display").await;
        let sample = insert_booster_product(&db, "p-sample", "fin", "play_pack").await;
        let other_set = insert_booster_product(&db, "p-other", "blb", "play_pack").await;

        let mut rows: HashSet<Row> = HashSet::from([
            // The canonical play pack's pool (the largest in the fin/play group).
            (pack, 10, "booster", false),
            (pack, 11, "booster", false),
            (pack, 12, "booster", true),
            // The sample pack carries its own smaller curated pool — must be preserved.
            (sample, 99, "booster", false),
            // A different set's play pack, empty, but its family group has no pool at all.
            // (other_set intentionally has no rows.)
        ]);
        let _ = other_set;

        let added = merge_sibling_booster_pools(&db, &mut rows).await.unwrap();

        // The empty sleeved pack + the empty display both inherit the 3-card canonical pool.
        assert_eq!(added, 6);
        assert_eq!(boosters_of(&rows, sleeved), vec![(10, false), (11, false), (12, true)]);
        assert_eq!(boosters_of(&rows, display), vec![(10, false), (11, false), (12, true)]);
        // The sample pack keeps ONLY its own curated pool (not inherited).
        assert_eq!(boosters_of(&rows, sample), vec![(99, false)]);
        // The other set (no pool anywhere in its group) gains nothing.
        assert_eq!(boosters_of(&rows, other_set), Vec::new());
    }

    /// The resolve + write path: memberships whose product AND card resolve are written;
    /// the rest are skipped, and a re-run replaces (not duplicates) the rows.
    #[tokio::test]
    async fn resolve_and_write_replaces_rows() {
        let db = migrated_memory_db().await;
        let card_a = insert_card(&db, "sf-a").await; // scryfall id "sf-a"
        let _card_b = insert_card(&db, "sf-b").await;
        let product_id = insert_product(&db, "1001").await;

        // Two resolvable rows + one whose product isn't in our catalog (skipped).
        let memberships = vec![
            RawMembership {
                tcgplayer_product_id: "1001".to_string(),
                scryfall_id: "sf-a".to_string(),
                membership: "contains",
                foil: false,
            },
            RawMembership {
                tcgplayer_product_id: "1001".to_string(),
                scryfall_id: "sf-a".to_string(),
                membership: "booster",
                foil: true,
            },
            RawMembership {
                tcgplayer_product_id: "9999".to_string(), // no such product
                scryfall_id: "sf-b".to_string(),
                membership: "contains",
                foil: false,
            },
        ];

        let written = write_for_test(&db, &memberships).await;
        assert_eq!(written, 2, "only the two rows with a matched product are written");

        let rows = SealedContent::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.product_id == product_id && r.card_id == card_a));
        assert!(rows.iter().any(|r| r.membership == "contains" && !r.foil));
        assert!(rows.iter().any(|r| r.membership == "booster" && r.foil));

        // Re-run replaces rather than duplicating (the transaction wipes first).
        let written_again = write_for_test(&db, &memberships).await;
        assert_eq!(written_again, 2);
        let count = SealedContent::find().count(&db).await.unwrap();
        assert_eq!(count, 2, "re-run replaces, not duplicates");
    }

    /// Drives the resolve + transactional replace without the network fetch, so the DB
    /// path is testable offline. Returns the number of rows written.
    async fn write_for_test(db: &DatabaseConnection, memberships: &[RawMembership]) -> usize {
        let product_ext: Vec<String> =
            distinct(memberships.iter().map(|m| &m.tcgplayer_product_id));
        let card_ext: Vec<String> = distinct(memberships.iter().map(|m| &m.scryfall_id));
        let products = resolve_products(db, &product_ext).await.unwrap();
        let cards = resolve_cards(db, &card_ext).await.unwrap();

        let now = Utc::now();
        let mut models: Vec<sealed_content::ActiveModel> = Vec::new();
        for m in memberships {
            let (Some(&product_id), Some(&card_id)) =
                (products.get(&m.tcgplayer_product_id), cards.get(&m.scryfall_id))
            else {
                continue;
            };
            models.push(sealed_content::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(product_id),
                card_id: Set(card_id),
                membership: Set(m.membership.to_string()),
                foil: Set(m.foil),
                created_at: Set(now),
                updated_at: Set(now),
            });
        }
        let written = models.len();
        let txn = db.begin().await.unwrap();
        SealedContent::delete_many()
            .filter(sealed_content::Column::Game.eq(GAME))
            .exec(&txn)
            .await
            .unwrap();
        SealedContent::insert_many(models)
            .on_conflict(
                OnConflict::columns([
                    sealed_content::Column::Game,
                    sealed_content::Column::ProductId,
                    sealed_content::Column::CardId,
                    sealed_content::Column::Membership,
                    sealed_content::Column::Foil,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&txn)
            .await
            .unwrap();
        txn.commit().await.unwrap();
        written
    }

    // ---- Secret Lair drop→cards derivation (`merge_sld_derived`) ----

    /// Insert a Secret Lair sealed product (`set_code = "sld"`) with a real storefront
    /// name and return its id — the candidates the derivation matches by name.
    async fn insert_sld_product(db: &DatabaseConnection, external_id: &str, name: &str) -> i32 {
        let now = Utc::now();
        let model = product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(name.to_string()),
            clean_name: Set(None),
            set_code: Set(sld::SET_CODE.to_string()),
            product_type: Set("secret_lair".to_string()),
            url: Set(None),
            image_url: Set(None),
            price_usd: Set(None),
            price_usd_foil: Set(None),
            released_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        product::Entity::insert(model).exec(db).await.unwrap().last_insert_id
    }

    /// Seed the five cards of the shipped "Cats of Chaos" drop (collector numbers
    /// 2690–2694) in the `sld` set, so the derivation has cards to attach.
    async fn seed_cats_of_chaos_cards(db: &DatabaseConnection) {
        for cn in ["2690", "2691", "2692", "2693", "2694"] {
            insert_card_at(db, &format!("sf-{cn}"), "sld", cn).await;
        }
    }

    #[tokio::test]
    async fn derives_sld_drop_cards_from_product_name() {
        let db = migrated_memory_db().await;
        seed_cats_of_chaos_cards(&db).await;
        // An unrelated sld card that must NOT be attached to the drop product.
        insert_card_at(&db, "sf-9999", "sld", "9999").await;
        let pid =
            insert_sld_product(&db, "700795", "Secret Lair Drop: Cats of Chaos - Non-Foil Edition").await;

        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_derived(&db, &HashSet::new(), &mut rows).await.unwrap();

        // Exactly the drop's five cards, as non-foil `contains` memberships of this product.
        assert_eq!(added, 5);
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|&(p, _, m, f)| p == pid && m == "contains" && !f));
    }

    #[tokio::test]
    async fn derived_foil_edition_marks_cards_foil() {
        let db = migrated_memory_db().await;
        seed_cats_of_chaos_cards(&db).await;
        insert_sld_product(&db, "700796", "Secret Lair Drop: Cats of Chaos - Traditional Foil Edition").await;

        let mut rows: HashSet<Row> = HashSet::new();
        merge_sld_derived(&db, &HashSet::new(), &mut rows).await.unwrap();

        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|&(_, _, m, f)| m == "contains" && f));
    }

    #[tokio::test]
    async fn sld_derivation_skips_already_covered_products() {
        let db = migrated_memory_db().await;
        seed_cats_of_chaos_cards(&db).await;
        let pid =
            insert_sld_product(&db, "700795", "Secret Lair Drop: Cats of Chaos - Non-Foil Edition").await;

        // MTGJSON (or the fallback) already described this product -> derivation is a no-op,
        // so upstream contents are never doubled up or overwritten.
        let covered = HashSet::from([pid]);
        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_derived(&db, &covered, &mut rows).await.unwrap();

        assert_eq!(added, 0);
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn sld_derivation_ignores_unmatched_and_non_sld_products() {
        let db = migrated_memory_db().await;
        seed_cats_of_chaos_cards(&db).await;
        // A non-sld product (a different set) is never a candidate.
        insert_product(&db, "111").await;
        // An sld product whose name matches no drop stays empty (never a wrong drop).
        insert_sld_product(&db, "222", "Secret Lair Drop: A Totally Made Up Nonexistent Drop - Non-Foil Edition")
            .await;

        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_derived(&db, &HashSet::new(), &mut rows).await.unwrap();

        assert_eq!(added, 0);
    }

    // ---- Secret Lair shared bonus cards (`merge_sld_bonus_cards`) ----

    /// Seed a set of `sld` cards by collector number so the bonus-pool derivation can attach them.
    async fn seed_sld_cards(db: &DatabaseConnection, numbers: &[&str]) {
        for cn in numbers {
            insert_card_at(db, &format!("sf-{cn}"), "sld", cn).await;
        }
    }

    #[tokio::test]
    async fn attaches_random_bonus_pool_as_variable() {
        let db = migrated_memory_db().await;
        // "Brain Dead: Creatures" draws its bonus from the shared 821–824 pool; its own cards
        // (1657–1661) are unrelated and not seeded here — the pool is a distinct axis.
        seed_sld_cards(&db, &["821", "822", "823", "824"]).await;
        let pid = insert_sld_product(
            &db,
            "930001",
            "Secret Lair Drop: Brain Dead: Creatures - Non-Foil Edition",
        )
        .await;

        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_bonus_cards(&db, &HashSet::new(), &mut rows).await.unwrap();

        // Exactly the four pool cards, as non-foil `variable` ("may be in") memberships.
        assert_eq!(added, 4);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|&(p, _, m, f)| p == pid && m == "variable" && !f));
    }

    #[tokio::test]
    async fn bonus_pool_foil_edition_marks_pool_foil() {
        let db = migrated_memory_db().await;
        seed_sld_cards(&db, &["821", "822", "823", "824"]).await;
        insert_sld_product(
            &db,
            "930002",
            "Secret Lair Drop: Brain Dead: Staples - Traditional Foil Edition",
        )
        .await;

        let mut rows: HashSet<Row> = HashSet::new();
        merge_sld_bonus_cards(&db, &HashSet::new(), &mut rows).await.unwrap();

        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|&(_, _, m, f)| m == "variable" && f));
    }

    #[tokio::test]
    async fn bonus_pool_steps_aside_when_mtgjson_enumerated_it() {
        let db = migrated_memory_db().await;
        seed_sld_cards(&db, &["821", "822", "823", "824"]).await;
        let pid = insert_sld_product(
            &db,
            "930001",
            "Secret Lair Drop: Brain Dead: Creatures - Non-Foil Edition",
        )
        .await;

        // MTGJSON already authored this product's own `variable` pool -> the curated pool self-
        // retires for it, so upstream stays authoritative and rows aren't doubled.
        let mtgjson_variable = HashSet::from([pid]);
        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_bonus_cards(&db, &mtgjson_variable, &mut rows).await.unwrap();

        assert_eq!(added, 0);
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn bonus_pool_ignores_drops_without_a_curated_pool() {
        let db = migrated_memory_db().await;
        seed_cats_of_chaos_cards(&db).await;
        // Cats of Chaos has no curated bonus pool -> nothing attaches (never a wrong card).
        insert_sld_product(&db, "930003", "Secret Lair Drop: Cats of Chaos - Non-Foil Edition")
            .await;

        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_bonus_cards(&db, &HashSet::new(), &mut rows).await.unwrap();

        assert_eq!(added, 0);
    }

    #[tokio::test]
    async fn avatar_bonus_pool_attaches_as_variable() {
        let db = migrated_memory_db().await;
        // Avatar's shared random bonus: one of Fellwar Stone (7062) / Command Tower (7063).
        seed_sld_cards(&db, &["7062", "7063"]).await;
        let pid = insert_sld_product(
            &db,
            "930010",
            "Secret Lair Drop: Avatar: The Last Airbender: My Cabbages! - Non-Foil Edition",
        )
        .await;

        let mut rows: HashSet<Row> = HashSet::new();
        let added = merge_sld_bonus_cards(&db, &HashSet::new(), &mut rows).await.unwrap();

        // Both pool cards linked as `variable` ("may be in"), at the edition's foil.
        assert_eq!(added, 2);
        assert!(rows.iter().all(|&(p, _, m, f)| p == pid && m == "variable" && !f));
    }

    #[tokio::test]
    async fn covered_avatar_product_keeps_its_bonus_pool_through_the_pass_sequence() {
        let db = migrated_memory_db().await;
        // The drop's own cards (2295–2299) plus the shared bonus pool (7062/7063).
        seed_sld_cards(&db, &["2295", "2296", "2297", "2298", "2299", "7062", "7063"]).await;
        let pid = insert_sld_product(
            &db,
            "930011",
            "Secret Lair Drop: Avatar: The Last Airbender: My Cabbages! - Non-Foil Edition",
        )
        .await;

        // Run the two passes in orchestration order for a product MTGJSON already covered (its own
        // deck): merge_sld_derived must skip it, but merge_sld_bonus_cards must still link the
        // bonus pool as `variable`. This is the regression the feature guards against.
        let covered = HashSet::from([pid]);
        let mut rows: HashSet<Row> = HashSet::new();
        assert_eq!(merge_sld_derived(&db, &covered, &mut rows).await.unwrap(), 0);
        merge_sld_bonus_cards(&db, &HashSet::new(), &mut rows).await.unwrap();

        // Only the bonus pool is present, as `variable` — the drop's own cards came from (skipped)
        // MTGJSON, so a covered product still surfaces its "may be in" bonus.
        assert_eq!(rows.len(), 2, "the two bonus-pool cards are linked");
        assert!(rows.iter().all(|&(p, _, m, _)| p == pid && m == "variable"));
    }
}
