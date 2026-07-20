//! Daily sync of MTG sealed products from TCGCSV into the `products` table.
//!
//! The sweep is: fetch every group (`/tcgplayer/1/groups`), then per group fetch its
//! products and prices, keep only **sealed** products (see [`super::classify`]),
//! classify each into a coarse `product_type`, and upsert them with their current
//! market prices. The whole sweep is **version-gated** on TCGCSV's `last-updated.txt`
//! (recorded in an `ingest_state` row keyed `(mtg, tcgcsv_products)`), so an unchanged
//! day costs one request. Requests are paced ~100 ms apart — a full sweep is ~900
//! requests, well under TCGCSV's ~10k/day budget.
//!
//! Mirrors `scryfall::ingest`'s shape (version gate, batched upserts with the
//! update-column list derived from the entity columns, `ingest_state` bookkeeping) but
//! reuses this module's [`BackfillError`](super::BackfillError) since it shares the
//! client and models.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, Iterable, QueryFilter, QuerySelect,
    prelude::DateTimeUtc,
    sea_query::OnConflict,
};

use super::model::{Group, aggregate_prices, published_on_to_date};
use super::progress::SyncProgress;
use super::{BackfillError, GAME, MTG_CATEGORY_ID, PRODUCTS_DATASET};
use crate::catalog::ingest_state::{self, StateFields};
use crate::db::upsert_changed_guard;
use crate::entities::prelude::{Card, Product};
use crate::entities::{card, product};
use crate::mtgjson::sld;

/// Courtesy pacing between provider requests (TCGCSV asks for < 10k req/day; a full
/// sweep is ~900 requests, so this keeps us well-behaved).
const REQUEST_SPACING: Duration = Duration::from_millis(100);

/// Rows per product upsert. A product row has ~15 columns, so ~1000 rows ≈ 15k bound
/// parameters — under SQLite's 32 766 limit.
const PRODUCT_BATCH: usize = 1000;

/// Separator joining TCGCSV's `last-updated` value with the curated MSRP file's content
/// hash in the stored version (a US control byte, which can't occur in either part — the
/// `last-updated` value is a timestamp and the MSRP hash is hex). Mirrors the coupling
/// [`crate::mtgjson::ingest`] uses for its ETag + fallback hash.
const VERSION_SEP: char = '\u{1f}';

/// Compose the stored sync version from TCGCSV's `last-updated` value and the curated MSRP
/// file's content hash, so an MSRP-data-only edit changes the version and forces a
/// re-sweep on the next tick even when TCGCSV itself is unchanged.
fn compose_version(tcgcsv: &str, msrp_hash: &str) -> String {
    format!("{tcgcsv}{VERSION_SEP}{msrp_hash}")
}

/// Sync MTG sealed products from TCGCSV, recording status in `ingest_state`. On error
/// the state row is best-effort marked `"error"` (so the next tick retries) and the
/// error is returned for the caller to log.
pub async fn refresh(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    match refresh_inner(db, http, user_agent, source).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest_state::mark_error(db, GAME, PRODUCTS_DATASET, &err.to_string()).await;
            Err(err)
        }
    }
}

async fn refresh_inner(
    db: &DatabaseConnection,
    http: &Client,
    user_agent: &str,
    source: &crate::datasets::SyncSource,
) -> Result<(), BackfillError> {
    let base_url = source.tcgcsv_base_url();
    // Version gate: one cheap request. Skip the whole sweep if TCGCSV hasn't refreshed
    // since our last complete sync. The stored version couples TCGCSV's `last-updated`
    // value with the curated MSRP hashes (see `compose_version`), so editing `msrp.json`
    // or the Secret Lair Drop MSRP defaults re-applies MSRP on the next sync even when
    // TCGCSV itself is unchanged.
    let remote_version = super::client::last_updated(http, &base_url, user_agent).await?;
    // Both curated MSRP hashes gate the sweep: the hand-curated `msrp.json` and the derived
    // Secret Lair Drop defaults (`sld_msrp`, which also covers the drop snapshot). Editing
    // either re-applies MSRP on the next tick even when TCGCSV itself is unchanged.
    let curated = format!("{}{}", super::msrp::version(), super::sld_msrp::version());
    let version = compose_version(&remote_version, &curated);
    let existing = ingest_state::load(db, GAME, PRODUCTS_DATASET).await?;
    if let Some(state) = &existing
        && state.status == "complete"
        && state.source_updated_at.as_deref() == Some(version.as_str())
    {
        tracing::info!(version = %remote_version, "tcgcsv products already up to date");
        return Ok(());
    }

    let started = existing
        .as_ref()
        .and_then(|s| s.started_at)
        .unwrap_or_else(Utc::now);
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: PRODUCTS_DATASET,
            status: "running",
            source_updated_at: None,
            detail: "fetching groups",
            sets_imported: 0,
            cards_imported: 0,
            started_at: started,
            finished_at: None,
        },
    )
    .await?;

    let groups = super::client::fetch_groups(http, &base_url, user_agent, MTG_CATEGORY_ID)
        .await?
        .results;
    tracing::info!(groups = groups.len(), "tcgcsv products: sweeping groups");

    let now = Utc::now();
    // Curated MSRP map (TCGplayer product id -> retail price), applied to every sweep so a
    // product's `msrp` stays set (or NULL) idempotently on each re-upsert.
    let msrp = super::msrp::price_map();
    // Per-drop Secret Lair release dates: TCGCSV files every drop under one `SLD` group with a
    // single `publishedOn`, so without this every SLD product would share that one date. Map
    // each `sld` collector number to its card's `released_at` so each SLD product's drop can
    // take the earliest date among its cards (see `super::sld_release`). Cards sync before
    // products in `catalog::refresh_all`, so this is populated on the first sweep; an empty map
    // (fresh DB) just falls back to the group date. Built once and reused for every group.
    let sld_card_dates = sld_card_release_dates(db).await?;
    let mut total_products: i32 = 0;
    let groups_total = groups.len() as i32;
    // Live terminal progress: a determinate bar over the groups being swept, with a
    // running sealed-product tally (see `super::progress`). Dropping it (incl. on any
    // `?` below) closes the span and clears the bar.
    let progress = SyncProgress::start_products(groups_total as u64);
    for (i, group) in groups.iter().enumerate() {
        tokio::time::sleep(REQUEST_SPACING).await;
        let products = super::client::fetch_products(
            http,
            &base_url,
            user_agent,
            MTG_CATEGORY_ID,
            group.group_id,
        )
        .await?
        .results;

        tokio::time::sleep(REQUEST_SPACING).await;
        let prices = aggregate_prices(
            super::client::fetch_prices(
                http,
                &base_url,
                user_agent,
                MTG_CATEGORY_ID,
                group.group_id,
            )
            .await?
            .results,
        );

        let models = build_group_products(group, products, &prices, msrp, &sld_card_dates, now);
        let sealed = models.len();
        total_products += upsert_products(db, models).await? as i32;
        progress.inc();
        progress.set_count(total_products as u64);
        tracing::debug!(
            group = group.group_id,
            name = group.name.as_deref().unwrap_or(""),
            sealed,
            "tcgcsv products: swept group"
        );

        // Periodic progress so the sweep is observable and a crash resumes cleanly
        // (the version gate re-runs the whole sweep — idempotent upserts make that safe).
        if i % 25 == 0 {
            ingest_state::put(
                db,
                StateFields {
                    game: GAME,
                    dataset: PRODUCTS_DATASET,
                    status: "running",
                    source_updated_at: None,
                    detail: &format!("swept {} of {groups_total} groups", i + 1),
                    sets_imported: groups_total,
                    cards_imported: total_products,
                    started_at: started,
                    finished_at: None,
                },
            )
            .await?;
        }
    }

    // Clear the progress bar before the completion line so it prints cleanly.
    drop(progress);
    ingest_state::put(
        db,
        StateFields {
            game: GAME,
            dataset: PRODUCTS_DATASET,
            status: "complete",
            source_updated_at: Some(&version),
            detail: &format!(
                "imported {total_products} sealed products from {groups_total} groups"
            ),
            sets_imported: groups_total,
            cards_imported: total_products,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(
        products = total_products,
        groups = groups_total,
        "tcgcsv products sync complete"
    );
    Ok(())
}

/// Every Secret Lair card's `released_at`, keyed by its `sld` collector number — the source
/// for a drop product's per-drop release date (see [`super::sld_release`]). Rows without a
/// known date are dropped, so a present key always maps to a real date. One indexed scan of
/// the small `sld` partition; an empty map (cards not yet synced) simply leaves SLD products
/// on the group date.
async fn sld_card_release_dates(
    db: &DatabaseConnection,
) -> Result<HashMap<String, String>, BackfillError> {
    let rows: Vec<(String, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::CollectorNumber)
        .column(card::Column::ReleasedAt)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::SetCode.eq(sld::SET_CODE))
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(cn, date)| date.map(|d| (cn, d)))
        .collect())
}

/// Build product `ActiveModel`s for a group's **sealed** products, attaching each
/// product's current market prices from `prices` and its retail price. The curated `msrp`
/// map (keyed by TCGplayer product id) wins; a Secret Lair Drop product not listed there
/// falls back to a price *derived* from its gallery drop (see [`super::sld_msrp`]), so
/// individual drops get MSRP without a per-product curated entry.
///
/// The release date is the group's `publishedOn` for ordinary sets (one group per set, so the
/// group date *is* every product's release), but Secret Lair files every drop under one group
/// with a single `publishedOn` — so an SLD product's date is *derived* per drop from its cards
/// (`sld_card_dates` maps an `sld` collector number to its `released_at`; see
/// [`super::sld_release`]), falling back to the group date when a product resolves to no drop.
///
/// Cards (products with a `Rarity`/`Number` attribute) are filtered out. Pure so it's
/// unit-testable without a DB.
fn build_group_products(
    group: &Group,
    products: Vec<super::model::Product>,
    prices: &HashMap<i64, super::model::DayPrice>,
    msrp: &HashMap<i64, String>,
    sld_card_dates: &HashMap<String, String>,
    now: DateTimeUtc,
) -> Vec<product::ActiveModel> {
    let set_code = group
        .abbreviation
        .as_deref()
        .map(|a| a.trim().to_lowercase())
        .unwrap_or_default();
    let group_released_at = published_on_to_date(group.published_on.as_deref());

    products
        .into_iter()
        .filter(|p| super::classify::is_sealed(&p.extended_data))
        .map(|p| {
            let product_type = super::classify::classify_product_type(&p.name);
            let day = prices.get(&p.product_id);
            let external_id = p.product_id.to_string();
            // Curated MSRP wins; otherwise derive it for individual Secret Lair Drops from
            // their gallery drop. Computed before the struct literal moves `p.name`.
            let msrp_value = msrp
                .get(&p.product_id)
                .cloned()
                .or_else(|| super::sld_msrp::derive(&set_code, &external_id, &p.name));
            // A Secret Lair drop's own release date (from its cards) wins over the shared group
            // date; every other product uses the group date. Computed before `p.name` moves.
            let released_at =
                super::sld_release::derive(&set_code, &external_id, &p.name, sld_card_dates)
                    .or_else(|| group_released_at.clone());
            product::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                external_id: Set(external_id),
                name: Set(p.name),
                clean_name: Set(p.clean_name),
                set_code: Set(set_code.clone()),
                product_type: Set(product_type.to_string()),
                url: Set(p.url),
                image_url: Set(p.image_url),
                price_usd: Set(day.and_then(|d| d.usd.clone())),
                price_usd_foil: Set(day.and_then(|d| d.usd_foil.clone())),
                msrp: Set(msrp_value),
                released_at: Set(released_at),
                created_at: Set(now),
                updated_at: Set(now),
            }
        })
        .collect()
}

/// Batched upsert on `(game, external_id)`, updating every provider-owned column (all
/// but the identity/conflict keys and `created_at`). Returns the number of rows sent.
async fn upsert_products(
    db: &DatabaseConnection,
    models: Vec<product::ActiveModel>,
) -> Result<u64, BackfillError> {
    let mut total: u64 = 0;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<product::ActiveModel> = iter.by_ref().take(PRODUCT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        total += chunk.len() as u64;
        Product::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([product::Column::Game, product::Column::ExternalId])
                    .update_columns(product::Column::iter().filter(|c| {
                        !matches!(
                            c,
                            product::Column::Id
                                | product::Column::Game
                                | product::Column::ExternalId
                                | product::Column::CreatedAt
                        )
                    }))
                    // Skip the write when the sealed product is unchanged — the daily
                    // sweep re-upserts every row on each version bump, and most are
                    // identical. `updated_at` stays in the SET list but out of the
                    // compare (same rationale as the card upsert in `scryfall::ingest`).
                    .action_and_where(upsert_changed_guard::<product::Column>("products", |c| {
                        matches!(
                            c,
                            product::Column::Id
                                | product::Column::Game
                                | product::Column::ExternalId
                                | product::Column::CreatedAt
                                | product::Column::UpdatedAt
                        )
                    }))
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::Product;
    use crate::tcgcsv::model::{DayPrice, ExtendedData, Product as SrcProduct};
    use sea_orm::EntityTrait;

    fn src_product(id: i64, name: &str, extended: &[&str]) -> SrcProduct {
        SrcProduct {
            product_id: id,
            name: name.to_string(),
            clean_name: Some(name.to_string()),
            image_url: Some(format!("https://img/{id}.jpg")),
            url: Some(format!("https://www.tcgplayer.com/product/{id}")),
            extended_data: extended
                .iter()
                .map(|n| ExtendedData {
                    name: n.to_string(),
                })
                .collect(),
        }
    }

    fn group() -> Group {
        Group {
            group_id: 2377,
            name: Some("Murders at Karlov Manor".to_string()),
            abbreviation: Some("MKM".to_string()),
            published_on: Some("2024-02-09T00:00:00".to_string()),
        }
    }

    #[test]
    fn builds_only_sealed_products_with_prices_and_metadata() {
        let products = vec![
            // Sealed (no Rarity/Number): kept, with prices attached.
            src_product(100, "Collector Booster Box", &["UPC"]),
            // A single card (has Rarity + Number): filtered out.
            src_product(200, "Some Rare Card", &["Rarity", "Number"]),
            // Sealed with no price row: kept, prices NULL.
            src_product(300, "Bundle", &[]),
        ];
        let mut prices: HashMap<i64, DayPrice> = HashMap::new();
        prices.insert(
            100,
            DayPrice {
                usd: Some("199.99".into()),
                usd_foil: None,
            },
        );
        // Curated MSRP for the box only; the bundle isn't listed.
        let msrp: HashMap<i64, String> = HashMap::from([(100, "249.99".to_string())]);
        let now = Utc::now();
        let models = build_group_products(&group(), products, &prices, &msrp, &HashMap::new(), now);

        assert_eq!(models.len(), 2, "the single card is filtered out");
        let box_model = models
            .iter()
            .find(|m| m.external_id.as_ref() == "100")
            .expect("sealed box present");
        assert_eq!(box_model.set_code.as_ref(), "mkm");
        assert_eq!(box_model.product_type.as_ref(), "collector_display");
        assert_eq!(
            box_model.released_at.as_ref().as_deref(),
            Some("2024-02-09")
        );
        assert_eq!(box_model.price_usd.as_ref().as_deref(), Some("199.99"));
        assert!(box_model.price_usd_foil.as_ref().is_none());
        // The curated MSRP is attached to the listed product.
        assert_eq!(box_model.msrp.as_ref().as_deref(), Some("249.99"));

        let bundle = models
            .iter()
            .find(|m| m.external_id.as_ref() == "300")
            .expect("sealed bundle present");
        assert_eq!(bundle.product_type.as_ref(), "bundle");
        assert!(bundle.price_usd.as_ref().is_none());
        // A product absent from the curated map has no MSRP.
        assert!(bundle.msrp.as_ref().is_none());
    }

    #[test]
    fn missing_abbreviation_yields_blank_set_code() {
        let group = Group {
            abbreviation: None,
            published_on: None,
            ..group()
        };
        let models = build_group_products(
            &group,
            vec![src_product(1, "Booster Box", &[])],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            Utc::now(),
        );
        assert_eq!(models[0].set_code.as_ref(), "");
        assert!(models[0].released_at.as_ref().is_none());
    }

    #[tokio::test]
    async fn upsert_products_is_idempotent_on_game_external_id() {
        let db = crate::test_support::migrated_memory_db().await;
        let now = Utc::now();

        let first = build_group_products(
            &group(),
            vec![src_product(100, "Collector Booster Box", &["UPC"])],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            now,
        );
        upsert_products(&db, first).await.expect("first upsert");

        // Re-sweep with a changed price: upserts the same row rather than duplicating.
        let mut prices: HashMap<i64, DayPrice> = HashMap::new();
        prices.insert(
            100,
            DayPrice {
                usd: Some("149.99".into()),
                usd_foil: None,
            },
        );
        let second = build_group_products(
            &group(),
            vec![src_product(100, "Collector Booster Box", &["UPC"])],
            &prices,
            &HashMap::new(),
            &HashMap::new(),
            now,
        );
        upsert_products(&db, second).await.expect("second upsert");

        let all = Product::find().all(&db).await.unwrap();
        assert_eq!(
            all.len(),
            1,
            "same (game, external_id) upserts, not duplicates"
        );
        assert_eq!(all[0].price_usd.as_deref(), Some("149.99"));
    }

    /// The `SLD` group name/abbreviation TCGCSV uses for Secret Lair; `set_code` derives
    /// from the abbreviation (lowercased), so this gates the derivation on `"sld"`.
    fn sld_group() -> Group {
        Group {
            group_id: 2350,
            name: Some("Secret Lair".to_string()),
            abbreviation: Some("SLD".to_string()),
            published_on: None,
        }
    }

    #[test]
    fn derives_sld_msrp_for_uncurated_drop_products() {
        // Two editions of a real drop present in the shipped snapshot, neither in the
        // curated map: MSRP is derived from the gallery drop by foilness.
        let products = vec![
            src_product(
                700795,
                "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                &[],
            ),
            src_product(
                700796,
                "Secret Lair Drop: Cats of Chaos - Traditional Foil Edition",
                &[],
            ),
        ];
        let models = build_group_products(
            &sld_group(),
            products,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            Utc::now(),
        );
        let non_foil = models
            .iter()
            .find(|m| m.external_id.as_ref() == "700795")
            .expect("non-foil drop present");
        assert_eq!(non_foil.set_code.as_ref(), "sld");
        assert_eq!(non_foil.msrp.as_ref().as_deref(), Some("29.99"));
        let foil = models
            .iter()
            .find(|m| m.external_id.as_ref() == "700796")
            .expect("foil drop present");
        assert_eq!(foil.msrp.as_ref().as_deref(), Some("39.99"));
    }

    #[test]
    fn curated_msrp_wins_over_sld_derivation() {
        // A curated entry for the drop product overrides the derived default.
        let msrp: HashMap<i64, String> = HashMap::from([(700795, "49.99".to_string())]);
        let models = build_group_products(
            &sld_group(),
            vec![src_product(
                700795,
                "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                &[],
            )],
            &HashMap::new(),
            &msrp,
            &HashMap::new(),
            Utc::now(),
        );
        assert_eq!(models[0].msrp.as_ref().as_deref(), Some("49.99"));
    }

    #[test]
    fn sld_products_take_their_drops_release_date_over_the_group_date() {
        // The `SLD` group carries one `publishedOn` for every drop; a resolvable drop's product
        // instead takes the earliest release date among its own cards, while a non-drop `SLD`
        // product (which resolves to no gallery drop) keeps the shared group date.
        let group = Group {
            published_on: Some("2019-12-02T00:00:00".to_string()),
            ..sld_group()
        };
        // "Cats of Chaos" is collector numbers 2690–2694 in the shipped snapshot; give two of
        // them dates so the earlier one wins.
        let card_dates: HashMap<String, String> = HashMap::from([
            ("2691".to_string(), "2024-05-03".to_string()),
            ("2690".to_string(), "2024-05-06".to_string()),
        ]);
        let models = build_group_products(
            &group,
            vec![
                src_product(
                    700795,
                    "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                    &[],
                ),
                src_product(
                    554987,
                    "Secret Lair Drop: Secret Lair Promo: Seedborn Muse - Rainbow Foil Edition",
                    &[],
                ),
            ],
            &HashMap::new(),
            &HashMap::new(),
            &card_dates,
            Utc::now(),
        );
        let drop = models
            .iter()
            .find(|m| m.external_id.as_ref() == "700795")
            .expect("drop product present");
        assert_eq!(drop.released_at.as_ref().as_deref(), Some("2024-05-03"));
        let promo = models
            .iter()
            .find(|m| m.external_id.as_ref() == "554987")
            .expect("promo product present");
        // No gallery drop → the shared group `publishedOn` (date part only) stands.
        assert_eq!(promo.released_at.as_ref().as_deref(), Some("2019-12-02"));
    }

    #[test]
    fn compose_version_couples_tcgcsv_and_msrp_hash() {
        // Same TCGCSV version but a different MSRP hash yields a different stored version,
        // so an MSRP-file edit alone re-runs the sweep on the next tick.
        let a = compose_version("2024-02-08", "aaaa");
        let b = compose_version("2024-02-08", "bbbb");
        assert_ne!(a, b);
        // …and the same inputs are stable (the equality the version gate compares on).
        assert_eq!(a, compose_version("2024-02-08", "aaaa"));
    }
}
