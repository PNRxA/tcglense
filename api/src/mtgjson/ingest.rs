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
    ColumnTrait, DatabaseConnection, EntityTrait, Iterable, QueryFilter, QuerySelect,
    TransactionTrait,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use super::client::{FetchOutcome, fetch_all_printings};
use super::fallback::{self, FallbackData, FallbackProduct};
use super::model::{self, RawComponent, RawMembership};
use super::progress::SyncProgress;
use super::sld;
use super::{DATASET, GAME, MtgjsonError};
use crate::entities::prelude::{Card, IngestState, Product, SealedComponent, SealedContent};
use crate::entities::{card, ingest_state, product, sealed_component, sealed_content};

/// Rows per external-id `IN` lookup — under SQLite's 32 766 bound-parameter limit.
const IN_CHUNK: usize = 900;

/// Rows per membership insert. Eight columns, so ~2000 rows ≈ 16k binds — under the limit.
const INSERT_BATCH: usize = 2000;

/// A resolved-to-internal-ids membership row: `(product_id, card_id, membership, foil)`.
/// Both the MTGJSON pass and the fallback merge accumulate into a `HashSet<Row>`, which
/// deduplicates across the two sources for free.
type Row = (i32, i32, &'static str, bool);

/// A resolved-to-internal-ids composition row, ready to insert into `sealed_components`.
/// The MTGJSON pass and the fallback merge both accumulate into a `Vec<ComponentRow>`
/// (ordered by `position` within each product; not deduplicated — position is identity).
struct ComponentRow {
    product_id: i32,
    position: i32,
    kind: String,
    name: String,
    quantity: i32,
    child_product_id: Option<i32>,
    child_card_id: Option<i32>,
}

/// Separator between MTGJSON's `ETag` and the fallback content hash in the stored
/// `ingest_state.source_updated_at`. A US control byte can't occur in an HTTP `ETag`
/// (RFC 9110 `etagc` excludes control chars), so splitting on it is unambiguous.
const VERSION_SEP: char = '\u{1f}';

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
            let _ = mark_error(db, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    http: &Client,
    source: &crate::datasets::SyncSource,
) -> Result<(), MtgjsonError> {
    let existing = load_state(db).await?;
    // The stored version couples MTGJSON's ETag with the committed fallback file's hash
    // (see `compose_version`), so a fallback-data edit forces a rebuild even when
    // AllPrintings is byte-identical.
    let stored = existing
        .as_ref()
        .filter(|s| s.status == "complete")
        .and_then(|s| s.source_updated_at.clone());
    let (prior_etag, prior_fallback, prior_sld) =
        stored.as_deref().map(split_version).unwrap_or((None, None, None));
    let fallback_version = fallback::version();
    let fallback_changed = prior_fallback != Some(fallback_version);
    // The Secret Lair drop→cards derivation reads `sld_drops.json` + curated overrides;
    // hash them into the gate so regenerating that data rebuilds even if MTGJSON didn't.
    let sld_version = sld::derivation_version();
    let sld_changed = prior_sld != Some(sld_version);

    let progress = SyncProgress::start("checking for updates");

    // Conditional fetch: a 304 (unchanged file) short-circuits the whole rebuild — but
    // only when the fallback data and the SLD derivation inputs are also unchanged. If
    // either local source changed we must re-fetch AllPrintings to rebuild the merged
    // table, so skip the conditional request. In mirror mode the file streams from the
    // mirror; upstream mode hits MTGJSON directly.
    let base_url = source.mtgjson_base_url();
    let conditional = if fallback_changed || sld_changed { None } else { prior_etag };
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
    put_state(db, "running", None, "resolving contents", started, None, 0, 0).await?;

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
    for m in &memberships {
        if let (Some(&product_id), Some(&card_id)) =
            (products.get(&m.tcgplayer_product_id), cards.get(&m.scryfall_id))
        {
            rows.insert((product_id, card_id, m.membership, m.foil));
            mtgjson_products.insert(product_id);
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
    let version = compose_version(etag.as_deref(), fallback_version, sld_version);
    let detail = format!(
        "{matched} memberships across {product_count} products \
         ({from_mtgjson} from mtgjson, {from_fallback} from fallback, {from_sld} from sld drops); \
         {component_count} components ({components_from_mtgjson} from mtgjson, \
         {components_from_fallback} from fallback)"
    );
    put_state(
        db,
        "complete",
        Some(&version),
        &detail,
        started,
        Some(Utc::now()),
        product_count as i32,
        matched as i32,
    )
    .await?;
    tracing::info!(
        memberships = matched,
        components = component_count,
        products = product_count,
        fallback = from_fallback,
        sld_derived = from_sld,
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

/// Merge curated [`fallback`] memberships into `rows`, returning the number of new rows
/// added. A fallback product is applied **only when MTGJSON emitted no rows for it**
/// (`mtgjson_products`), so upstream stays authoritative and the fallback fills genuine
/// gaps (e.g. Avatar's `contents: null` Commander's Bundle). A fallback product or card
/// absent from our catalog, or carrying an unknown membership, is skipped and logged.
async fn merge_fallback(
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
        if mtgjson_products.contains(&product_id) {
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
            if rows.insert((product_id, card_id, membership.as_str(), card.foil)) {
                added += 1;
            }
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: merged curated fallback memberships");
    }
    Ok(added)
}

/// Merge curated [`fallback`] composition components into `rows`, returning how many were
/// added. Applied per product **only when MTGJSON gave that product no composition**
/// (`mtgjson_component_products`), mirroring [`merge_fallback`] — so upstream stays
/// authoritative and this fills genuine gaps (e.g. Avatar's `contents: null` Commander's
/// Bundle: "9× Play Booster, 1× Collector Booster, …"). A component's optional child
/// product / card links resolve to internal ids the same way MTGJSON's do; an unresolved
/// link keeps the line item as text. A fallback product absent from our catalog, or a
/// component with an unknown kind, is skipped and logged.
async fn merge_fallback_components(
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
/// left empty. Each such product's name identifies its drop (see [`sld`]); the drop's
/// cards (by collector number in the `sld` set) become `contains` memberships, foil per
/// the product's edition. `covered` is the set of products that already have a membership
/// row, so this only fills genuine gaps — mirroring the fallback gate. Returns rows added.
async fn merge_sld_derived(
    db: &DatabaseConnection,
    covered: &HashSet<i32>,
    rows: &mut HashSet<Row>,
) -> Result<usize, MtgjsonError> {
    let Some(table) = sld::table() else {
        return Ok(0);
    };
    let products = load_sld_products(db).await?;

    // Resolve each still-empty SLD product to its drop, keeping the drop's collector
    // numbers (a `'static` slice — the drop table is `'static`) and the product's foilness.
    let mut resolved: Vec<(i32, bool, &'static [String])> = Vec::new();
    for (product_id, external_id, name) in &products {
        if covered.contains(product_id) {
            continue;
        }
        if let Some(pd) = sld::resolve_product_drop(table, external_id, name) {
            resolved.push((*product_id, pd.foil, pd.drop.collector_numbers.as_slice()));
        }
    }
    if resolved.is_empty() {
        return Ok(0);
    }

    // Resolve those drops' collector numbers (all in the `sld` set) to internal card ids,
    // in one pass over the set.
    let card_keys: Vec<(String, String)> = resolved
        .iter()
        .flat_map(|(_, _, cns)| cns.iter().map(|cn| (sld::SET_CODE.to_string(), cn.clone())))
        .collect();
    let cards = resolve_cards_by_setnum(db, &card_keys).await?;

    let membership = sealed_content::Membership::Contains.as_str();
    let mut added = 0;
    for (product_id, foil, cns) in resolved {
        for cn in cns {
            let Some(&card_id) = cards.get(&(sld::SET_CODE.to_string(), cn.clone())) else {
                continue;
            };
            if rows.insert((product_id, card_id, membership, foil)) {
                added += 1;
            }
        }
    }
    if added > 0 {
        tracing::info!(rows = added, "mtgjson: derived Secret Lair drop card contents");
    }
    Ok(added)
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

/// Compose the stored version string from MTGJSON's `ETag`, the fallback content hash, and
/// the SLD-derivation hash, joined by [`VERSION_SEP`].
fn compose_version(etag: Option<&str>, fallback: &str, sld: &str) -> String {
    format!("{}{VERSION_SEP}{fallback}{VERSION_SEP}{sld}", etag.unwrap_or(""))
}

/// Split a stored version back into `(mtgjson_etag, fallback_hash, sld_hash)`. A value
/// written before a given field existed simply parses that field (and any after it) as
/// `None`, so the first sync after an upgrade sees a change and rebuilds once: a bare
/// pre-feature ETag -> `(Some, None, None)`; a two-part pre-SLD version -> `(Some, Some,
/// None)`.
fn split_version(stored: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
    let mut parts = stored.split(VERSION_SEP);
    let etag = parts.next().and_then(non_empty);
    let fallback = parts.next().and_then(non_empty);
    let sld = parts.next().and_then(non_empty);
    (etag, fallback, sld)
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

/// Collect the distinct owned strings from an iterator of `&String`.
fn distinct<'a, I: Iterator<Item = &'a String>>(iter: I) -> Vec<String> {
    let set: std::collections::HashSet<&String> = iter.collect();
    set.into_iter().cloned().collect()
}

/// Resolve TCGplayer product ids -> internal `products.id` for the game, chunked under
/// SQLite's bind limit.
async fn resolve_products(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Product::find()
            .select_only()
            .column(product::Column::ExternalId)
            .column(product::Column::Id)
            .filter(product::Column::Game.eq(GAME))
            .filter(product::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Resolve Scryfall ids -> internal `cards.id` for the game, chunked under SQLite's bind
/// limit.
async fn resolve_cards(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Card::find()
            .select_only()
            .column(card::Column::ExternalId)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Resolve `(set_code, collector_number)` pairs -> internal `cards.id` for the game (the
/// fallback data keys cards this way rather than by Scryfall id). Fetches by the distinct
/// set codes and indexes in memory — the fallback is a handful of cards, so this is a few
/// small queries. Keys are lowercased set codes; the returned map's keys match.
async fn resolve_cards_by_setnum(
    db: &DatabaseConnection,
    keys: &[(String, String)],
) -> Result<HashMap<(String, String), i32>, MtgjsonError> {
    let set_codes: Vec<String> = distinct(keys.iter().map(|(set, _)| set));
    let mut map = HashMap::new();
    for chunk in set_codes.chunks(IN_CHUNK) {
        let rows: Vec<(String, String, i32)> = Card::find()
            .select_only()
            .column(card::Column::SetCode)
            .column(card::Column::CollectorNumber)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::SetCode.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        for (set, number, id) in rows {
            map.insert((set.to_lowercase(), number), id);
        }
    }
    Ok(map)
}

// ----- ingest_state bookkeeping (dataset = mtgjson_sealed_contents) -----

async fn load_state(db: &DatabaseConnection) -> Result<Option<ingest_state::Model>, MtgjsonError> {
    Ok(IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(DATASET))
        .one(db)
        .await?)
}

#[allow(clippy::too_many_arguments)]
async fn put_state(
    db: &DatabaseConnection,
    status: &str,
    source_updated_at: Option<&str>,
    detail: &str,
    started_at: DateTimeUtc,
    finished_at: Option<DateTimeUtc>,
    products: i32,
    memberships: i32,
) -> Result<(), MtgjsonError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(source_updated_at.map(str::to_string)),
        status: Set(status.to_string()),
        detail: Set(Some(detail.to_string())),
        sets_imported: Set(products),
        cards_imported: Set(memberships),
        started_at: Set(Some(started_at)),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns(ingest_state::Column::iter().filter(|c| {
                    !matches!(
                        c,
                        ingest_state::Column::Id
                            | ingest_state::Column::Game
                            | ingest_state::Column::Dataset
                    )
                }))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

async fn mark_error(db: &DatabaseConnection, message: &str) -> Result<(), MtgjsonError> {
    let existing = load_state(db).await?;
    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    // Keep the last known-good ETag so a transient failure doesn't force a full re-fetch
    // *unless* the file also changed.
    let last = existing.and_then(|s| s.source_updated_at);
    let detail: String = message.chars().take(500).collect();
    put_state(db, "error", last.as_deref(), &detail, started, Some(Utc::now()), 0, 0).await
}

#[cfg(test)]
mod tests {
    use super::super::fallback::{FallbackCard, FallbackComponent, FallbackProduct};
    use super::*;
    use crate::entities::prelude::{SealedComponent, SealedContent};
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
        let composed = compose_version(Some("\"etag-123\""), "fbhash", "sldhash");
        assert_eq!(split_version(&composed), (Some("\"etag-123\""), Some("fbhash"), Some("sldhash")));
        // A pre-feature bare ETag: nothing after it recorded.
        assert_eq!(split_version("\"legacy-etag\""), (Some("\"legacy-etag\""), None, None));
        // A two-part version from before the SLD field: SLD reads as absent -> rebuild once.
        let two_part = format!("\"etag\"{VERSION_SEP}fbhash");
        assert_eq!(split_version(&two_part), (Some("\"etag\""), Some("fbhash"), None));
        assert_eq!(split_version(&compose_version(None, "fb", "sld")), (None, Some("fb"), Some("sld")));
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
}
