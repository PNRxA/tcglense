//! Rebuild of the `booster_configs` / `booster_sheets` / `sealed_packs` tables from the
//! booster data the pure pass resolved ([`crate::mtgjson::boosters`]).
//!
//! Same shape as the membership, composition and precon rebuilds it runs beside: map
//! external ids (Scryfall id -> `cards.id`, TCGplayer product id -> `products.id`) onto our
//! catalog, then **replace** the game's rows inside the caller's transaction, so a reader
//! never sees a half-rebuilt table and a configuration upstream dropped can't linger.
//!
//! Two things are decided here rather than at read time:
//!
//! * A configuration's stored `total_weight` is Σ of **the variants we kept**, never
//!   upstream's `boostersTotalWeight`, so a variant dropped for a zero weight can't leave
//!   the shares summing to less than one.
//! * A sheet's cards become internal `cards.id`s, dropping any our catalog doesn't hold
//!   (on a **fixed** sheet, read by position, it keeps its place as
//!   [`booster_sheet::UNRESOLVED_CARD_ID`] instead) —
//!   but the sheet's `total_weight` still counts them, so the read can report the share it
//!   can't price instead of quietly re-normalising ([`crate::mtgjson::boosters`]).
//!
//! A sheet whose every card was dropped is still written, as an empty `[]`: the slot that
//! names it is real, and a read that finds no row can't tell "the pack has no such slot"
//! from "we hold none of its cards".
//!
//! Child rows are deleted **explicitly** rather than through the tables' `ON DELETE
//! CASCADE`: SQLite (the default backend) doesn't enforce foreign keys unless
//! `PRAGMA foreign_keys` is on, so relying on the cascade would silently orphan every sheet
//! on a self-host while working fine on Postgres.

use std::collections::{BTreeMap, HashMap};

use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
    prelude::DateTimeUtc,
};

use super::super::boosters::{RawBoosterConfig, RawPack};
use super::super::{GAME, MtgjsonError};
use super::INSERT_BATCH;
use crate::entities::prelude::{BoosterConfig, BoosterSheet, SealedPack};
use crate::entities::{booster_config, booster_sheet, sealed_pack};

/// What a rebuild wrote, for the sync's `ingest_state` detail line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoosterStats {
    pub configs: usize,
    pub sheets: usize,
    pub packs: usize,
}

/// Replace the game's booster configurations, sheets and product links inside `txn`.
///
/// A pack link is written only when **both** its product and its configuration resolved:
/// a product our catalog doesn't carry has nothing to hang the link off, and a
/// configuration that failed to insert would leave a link pointing at nothing.
pub(crate) async fn rebuild<C: ConnectionTrait>(
    txn: &C,
    configs: &[RawBoosterConfig],
    packs: &[RawPack],
    card_ids: &HashMap<String, i32>,
    product_ids: &HashMap<String, i32>,
    now: DateTimeUtc,
) -> Result<BoosterStats, MtgjsonError> {
    // 1. Clear the game's rows, children first (see the module note on cascades).
    SealedPack::delete_many()
        .filter(sealed_pack::Column::Game.eq(GAME))
        .exec(txn)
        .await?;
    BoosterSheet::delete_many()
        .filter(booster_sheet::Column::ConfigId.in_subquery(game_config_ids()))
        .exec(txn)
        .await?;
    BoosterConfig::delete_many()
        .filter(booster_config::Column::Game.eq(GAME))
        .exec(txn)
        .await?;

    // 2. Insert the configurations in batches, then read back the ids their
    // `(set_code, code)` identities took — one round trip per batch plus one, rather than
    // one per configuration (the difference a networked Postgres feels).
    for batch in configs.chunks(INSERT_BATCH) {
        let chunk: Vec<booster_config::ActiveModel> =
            batch.iter().map(|raw| config_model(raw, now)).collect();
        BoosterConfig::insert_many(chunk)
            .exec_without_returning(txn)
            .await?;
    }
    let ids: HashMap<(String, String), i32> = BoosterConfig::find()
        .select_only()
        .column(booster_config::Column::SetCode)
        .column(booster_config::Column::Code)
        .column(booster_config::Column::Id)
        .filter(booster_config::Column::Game.eq(GAME))
        .into_tuple::<(String, String, i32)>()
        .all(txn)
        .await?
        .into_iter()
        .map(|(set_code, code, id)| ((set_code, code), id))
        .collect();

    // 3. Insert the sheets against those ids, materialised per insert batch (never as one
    // whole-run `Vec` — this runs inside the sealed-sync write transaction, on top of
    // everything else that phase holds; see `refresh_inner`'s write loop).
    let mut sheets_written = 0usize;
    let mut buffer: Vec<booster_sheet::ActiveModel> = Vec::with_capacity(INSERT_BATCH);
    for raw in configs {
        let Some(&config_id) = ids.get(&(raw.set_code.clone(), raw.code.clone())) else {
            continue;
        };
        for sheet in &raw.sheets {
            let cards: Vec<(i32, u32)> = sheet
                .cards
                .iter()
                .filter_map(|(scryfall, weight)| match card_ids.get(scryfall) {
                    Some(&id) => Some((id, *weight)),
                    // A fixed sheet is read by position, so a card the catalog doesn't hold
                    // keeps its place as `UNRESOLVED_CARD_ID` instead of shifting every
                    // card after it. Everywhere else it is dropped, its weight still
                    // standing in `total_weight`.
                    None if sheet.fixed => Some((booster_sheet::UNRESOLVED_CARD_ID, *weight)),
                    None => None,
                })
                .collect();
            buffer.push(booster_sheet::ActiveModel {
                id: NotSet,
                config_id: Set(config_id),
                name: Set(sheet.name.clone()),
                foil: Set(sheet.foil),
                balance_colors: Set(sheet.balance_colors),
                allow_duplicates: Set(sheet.allow_duplicates),
                fixed: Set(sheet.fixed),
                total_weight: Set(clamp_i64(sheet.total_weight)),
                cards: Set(booster_sheet::encode_cards(&cards)),
                created_at: Set(now),
                updated_at: Set(now),
            });
            if buffer.len() >= INSERT_BATCH {
                sheets_written += buffer.len();
                let chunk = std::mem::replace(&mut buffer, Vec::with_capacity(INSERT_BATCH));
                BoosterSheet::insert_many(chunk)
                    .exec_without_returning(txn)
                    .await?;
            }
        }
    }
    if !buffer.is_empty() {
        sheets_written += buffer.len();
        BoosterSheet::insert_many(buffer)
            .exec_without_returning(txn)
            .await?;
    }

    // 4. Insert the product -> configuration links, folded by `(product, configuration)` so
    // the table's unique key holds even if two source rows land on one pair (two
    // `sealedProduct` entries sharing a TCGplayer id). A BTreeMap keeps the insert order
    // deterministic.
    let mut links: BTreeMap<(i32, i32), i64> = BTreeMap::new();
    for pack in packs {
        let Some(&product_id) = product_ids.get(&pack.tcgplayer_product_id) else {
            continue;
        };
        let Some(&config_id) = ids.get(&(pack.set_code.clone(), pack.booster_code.clone())) else {
            continue;
        };
        let quantity = links.entry((product_id, config_id)).or_insert(0);
        *quantity = quantity.saturating_add(i64::from(pack.quantity));
    }
    let packs_written = links.len();
    let link_models: Vec<sealed_pack::ActiveModel> = links
        .into_iter()
        .map(
            |((product_id, config_id), quantity)| sealed_pack::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                product_id: Set(product_id),
                config_id: Set(config_id),
                quantity: Set(i32::try_from(quantity).unwrap_or(i32::MAX).max(1)),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .collect();
    let mut iter = link_models.into_iter();
    loop {
        let chunk: Vec<sealed_pack::ActiveModel> = iter.by_ref().take(INSERT_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        SealedPack::insert_many(chunk)
            .exec_without_returning(txn)
            .await?;
    }

    Ok(BoosterStats {
        configs: ids.len(),
        sheets: sheets_written,
        packs: packs_written,
    })
}

/// Sub-select of this game's `booster_configs.id` — what the sheet delete filters against,
/// so "the game's sheets" is expressed once, through the query API (parameterised and
/// dialect-neutral) like every other query here.
fn game_config_ids() -> sea_orm::sea_query::SelectStatement {
    BoosterConfig::find()
        .select_only()
        .column(booster_config::Column::Id)
        .filter(booster_config::Column::Game.eq(GAME))
        .into_query()
}

/// Materialise one configuration as an insertable model. `total_weight` is recomputed from
/// the variants actually stored (see the module note).
fn config_model(raw: &RawBoosterConfig, now: DateTimeUtc) -> booster_config::ActiveModel {
    let variants: Vec<booster_config::Variant> = raw
        .variants
        .iter()
        .map(|variant| booster_config::Variant {
            weight: variant.weight,
            slots: variant.slots.clone(),
        })
        .collect();
    let total: u64 = variants
        .iter()
        .fold(0u64, |acc, variant| acc.saturating_add(variant.weight));
    booster_config::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        set_code: Set(raw.set_code.clone()),
        code: Set(raw.code.clone()),
        name: Set(raw.name.clone()),
        total_weight: Set(clamp_i64(total)),
        variants: Set(booster_config::encode_variants(&variants)),
        created_at: Set(now),
        updated_at: Set(now),
    }
}

/// The stored weight columns are signed 64-bit; upstream's are unsigned. A weight past
/// `i64::MAX` is upstream nonsense, and saturating keeps it a denominator rather than a
/// wrapped negative.
fn clamp_i64(weight: u64) -> i64 {
    i64::try_from(weight).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Σ of a stored sheet's card weights — the share of `total_weight` the row can deal.
    fn stored_weight(sheet: &booster_sheet::Model) -> u64 {
        sheet.cards().iter().map(|&(_, w)| u64::from(w)).sum()
    }
    use crate::entities::prelude::{Card, Product};
    use crate::mtgjson::boosters::{RawSheet, RawVariant};
    use crate::test_support::{insert_card, insert_product, migrated_memory_db};
    use chrono::Utc;
    use sea_orm::{DatabaseConnection, QueryOrder, TransactionTrait};

    fn variant(weight: u64, slots: &[(&str, u32)]) -> RawVariant {
        RawVariant {
            weight,
            slots: slots
                .iter()
                .map(|(name, count)| ((*name).to_string(), *count))
                .collect(),
        }
    }

    fn sheet(name: &str, foil: bool, total_weight: u64, cards: &[(&str, u32)]) -> RawSheet {
        RawSheet {
            name: name.to_string(),
            foil,
            balance_colors: false,
            allow_duplicates: false,
            fixed: false,
            total_weight,
            cards: cards
                .iter()
                .map(|(scryfall, weight)| ((*scryfall).to_string(), *weight))
                .collect(),
        }
    }

    fn config(code: &str, variants: Vec<RawVariant>, sheets: Vec<RawSheet>) -> RawBoosterConfig {
        RawBoosterConfig {
            set_code: "tst".to_string(),
            code: code.to_string(),
            name: Some(format!("{code} Booster")),
            variants,
            sheets,
        }
    }

    fn pack(product: &str, code: &str, quantity: u32) -> RawPack {
        RawPack {
            tcgplayer_product_id: product.to_string(),
            set_code: "tst".to_string(),
            booster_code: code.to_string(),
            quantity,
        }
    }

    /// Read the seeded catalog back as the `(external id -> internal id)` maps the sync
    /// hands `rebuild`.
    async fn ids(db: &DatabaseConnection) -> (HashMap<String, i32>, HashMap<String, i32>) {
        let cards = Card::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| (c.external_id, c.id))
            .collect();
        let products = Product::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| (p.external_id, p.id))
            .collect();
        (cards, products)
    }

    /// Run one rebuild inside its own transaction, as the sync does.
    async fn run(
        db: &DatabaseConnection,
        configs: &[RawBoosterConfig],
        packs: &[RawPack],
    ) -> BoosterStats {
        let (cards, products) = ids(db).await;
        let txn = db.begin().await.unwrap();
        let stats = rebuild(&txn, configs, packs, &cards, &products, Utc::now())
            .await
            .unwrap();
        txn.commit().await.unwrap();
        stats
    }

    /// The write path end to end: the configuration's total weight is recomputed from its
    /// stored variants, the variants and cards round-trip through their JSON columns with
    /// their order and weights intact, an unresolved card is dropped while its weight stays
    /// in the sheet's total, and the pack links carry the right quantities.
    #[tokio::test]
    async fn rebuild_writes_configs_sheets_and_links() {
        let db = migrated_memory_db().await;
        let common = insert_card(&db, "sf-common").await;
        let rare = insert_card(&db, "sf-rare").await;
        let mythic = insert_card(&db, "sf-mythic").await;
        insert_product(&db, "1001", "Pack", "tst", "play_pack", Some("4.99")).await;
        insert_product(&db, "1002", "Box", "tst", "draft_display", Some("129.99")).await;

        let configs = vec![config(
            "play",
            vec![
                variant(3, &[("common", 6), ("rareMythic", 1)]),
                variant(1, &[("common", 5), ("list", 1), ("rareMythic", 1)]),
            ],
            vec![
                // `sf-ghost` isn't in the catalog: dropped from `cards`, still counted in
                // the sheet's total weight.
                sheet("common", false, 12, &[("sf-common", 3), ("sf-ghost", 9)]),
                sheet("rareMythic", true, 3, &[("sf-rare", 2), ("sf-mythic", 1)]),
            ],
        )];
        let packs = vec![
            pack("1001", "play", 1),
            pack("1002", "play", 36),
            // A product our catalog doesn't carry has nothing to link.
            pack("9999", "play", 12),
        ];

        let stats = run(&db, &configs, &packs).await;
        assert_eq!(
            (stats.configs, stats.sheets, stats.packs),
            (1, 2, 2),
            "the unknown product's link is not written"
        );

        let stored = BoosterConfig::find()
            .one(&db)
            .await
            .unwrap()
            .expect("a configuration");
        assert_eq!(
            (stored.set_code.as_str(), stored.code.as_str()),
            ("tst", "play")
        );
        assert_eq!(stored.name.as_deref(), Some("play Booster"));
        assert_eq!(
            stored.total_weight, 4,
            "the total is Σ of the stored variants, not upstream's claim"
        );
        let variants = stored.variants();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].weight, 3);
        assert_eq!(
            variants[0].slots,
            vec![("common".to_string(), 6), ("rareMythic".to_string(), 1)]
        );
        assert_eq!(
            variants[1].slots,
            vec![
                ("common".to_string(), 5),
                ("list".to_string(), 1),
                ("rareMythic".to_string(), 1)
            ],
            "a slot naming a sheet the configuration lacks is kept"
        );

        let sheets = BoosterSheet::find()
            .order_by_asc(booster_sheet::Column::Name)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].name, "common");
        assert!(!sheets[0].foil);
        assert_eq!(sheets[0].config_id, stored.id);
        assert_eq!(
            sheets[0].cards,
            "[[".to_string() + &common.to_string() + ",3]]"
        );
        assert_eq!(sheets[0].cards(), vec![(common, 3)]);
        assert_eq!(
            sheets[0].total_weight, 12,
            "the dropped card's weight stays in the denominator"
        );
        assert_eq!(stored_weight(&sheets[0]), 3);
        assert!(sheets[1].foil);
        assert_eq!(
            sheets[1].cards(),
            vec![(rare, 2), (mythic, 1)],
            "cards keep upstream's order"
        );

        let links = SealedPack::find()
            .order_by_asc(sealed_pack::Column::Quantity)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].quantity, 1);
        assert_eq!(links[1].quantity, 36);
        assert!(links.iter().all(|l| l.config_id == stored.id));
    }

    /// A sheet whose every card is missing from the catalog is still written, as an empty
    /// list: the slot that names it is real, and no row at all would read as "the pack has
    /// no such slot".
    #[tokio::test]
    async fn a_wholly_unresolved_sheet_is_still_written_empty() {
        let db = migrated_memory_db().await;
        insert_product(&db, "1001", "Pack", "tst", "play_pack", None).await;
        let configs = vec![config(
            "play",
            vec![variant(1, &[("ghost", 1)])],
            vec![sheet("ghost", false, 5, &[("sf-nobody", 5)])],
        )];

        let stats = run(&db, &configs, &[pack("1001", "play", 1)]).await;
        assert_eq!((stats.configs, stats.sheets, stats.packs), (1, 1, 1));
        let sheet = BoosterSheet::find()
            .one(&db)
            .await
            .unwrap()
            .expect("a sheet");
        assert_eq!(sheet.cards, "[]");
        assert_eq!(sheet.total_weight, 5);
        assert_eq!(stored_weight(&sheet), 0);
    }

    /// A rebuild **replaces**: a configuration upstream dropped leaves with its sheets and
    /// its product links, rather than lingering as an orphan (which the tables' `ON DELETE
    /// CASCADE` would not achieve on SQLite, where foreign keys aren't enforced by default).
    #[tokio::test]
    async fn rebuild_replaces_the_previous_run() {
        let db = migrated_memory_db().await;
        insert_card(&db, "sf-a").await;
        insert_product(&db, "1001", "Pack", "tst", "play_pack", None).await;
        insert_product(&db, "1002", "Box", "tst", "draft_display", None).await;

        let first = vec![
            config(
                "play",
                vec![variant(1, &[("common", 1)])],
                vec![sheet("common", false, 1, &[("sf-a", 1)])],
            ),
            config(
                "gone",
                vec![variant(1, &[("common", 1)])],
                vec![sheet("common", false, 1, &[("sf-a", 1)])],
            ),
        ];
        let stats = run(
            &db,
            &first,
            &[pack("1001", "play", 1), pack("1002", "gone", 6)],
        )
        .await;
        assert_eq!((stats.configs, stats.sheets, stats.packs), (2, 2, 2));

        let second = vec![config(
            "play",
            vec![variant(2, &[("common", 2)])],
            vec![sheet("common", false, 1, &[("sf-a", 1)])],
        )];
        let stats = run(&db, &second, &[pack("1001", "play", 1)]).await;
        assert_eq!((stats.configs, stats.sheets, stats.packs), (1, 1, 1));

        let configs = BoosterConfig::find().all(&db).await.unwrap();
        assert_eq!(configs.len(), 1, "the dropped configuration is gone");
        assert_eq!(configs[0].code, "play");
        assert_eq!(configs[0].total_weight, 2);
        let sheets = BoosterSheet::find().all(&db).await.unwrap();
        assert_eq!(sheets.len(), 1, "its sheets went with it");
        assert_eq!(sheets[0].config_id, configs[0].id);
        let links = SealedPack::find().all(&db).await.unwrap();
        assert_eq!(links.len(), 1, "and so did its product links");
        assert_eq!(links[0].config_id, configs[0].id);
    }

    /// Two source rows landing on one `(product, configuration)` pair fold into one row
    /// rather than tripping the table's unique key.
    #[tokio::test]
    async fn duplicate_links_fold_into_one_row() {
        let db = migrated_memory_db().await;
        insert_product(&db, "1001", "Bundle", "tst", "bundle", None).await;
        let configs = vec![config(
            "play",
            vec![variant(1, &[("common", 1)])],
            Vec::new(),
        )];

        let stats = run(
            &db,
            &configs,
            &[pack("1001", "play", 2), pack("1001", "play", 4)],
        )
        .await;
        assert_eq!(stats.packs, 1);
        let links = SealedPack::find().all(&db).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].quantity, 6);
    }

    /// Nothing to write is not an error — an empty run clears the tables and reports zeroes.
    #[tokio::test]
    async fn an_empty_rebuild_clears_and_reports_nothing() {
        let db = migrated_memory_db().await;
        let stats = run(&db, &[], &[]).await;
        assert_eq!((stats.configs, stats.sheets, stats.packs), (0, 0, 0));
        assert!(BoosterConfig::find().all(&db).await.unwrap().is_empty());
    }
}
