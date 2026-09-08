//! Fabricated **booster configurations** for the offline dummy catalog: a Play Booster and
//! a Collector Booster for the base set, a Draft Booster for the Universe set, and the
//! product links that say how many of each a copy of a sealed product opens — so a sealed
//! product's expected value and its pack opener (issue #682) have data to serve with no
//! network, and the e2e suite has something to click.
//!
//! Pure data plus one write, like [`super::precons`]: the configurations are declared here
//! by *card external id* and resolved against the just-seeded catalog. The write itself is
//! the **real** rebuild ([`crate::mtgjson::ingest::boosters::rebuild`]), not a second copy
//! of it — so an offline row is shaped exactly like a synced one (the recomputed
//! `total_weight`, the JSON encodings, the delete-then-insert per game), and a change to
//! how the ingest stores a sheet can't leave the dummy catalog behind.
//!
//! The three cover the shapes the read has to handle: a booster with several sheets and a
//! foil slot; one with a `fixed` sheet (its cards are taken in order, not at random) and a
//! foil sheet with `allowDuplicates`; and a booster with **two variants**, so the weighted
//! roll between configurations is exercised. One card on the play booster's rare sheet is
//! the catalog's foil-only printing, which carries no regular price — a non-foil sheet
//! holding it exercises the unpriced path offline.
//!
//! `900004` (the commander deck) deliberately gets **no** link: a product with no booster
//! data must answer `null` rather than an expected value of zero, and that path needs a
//! seeded product too.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use super::super::GAME;
use super::super::ingest::IngestError;
use super::catalog::BASE_NUMBERED;
use crate::entities::prelude::{Card, Product};
use crate::entities::{card, product};
use crate::mtgjson::MtgjsonError;
use crate::mtgjson::boosters::{RawBoosterConfig, RawPack, RawSheet, RawVariant};

/// Numbered cards in the Universe set (`dummy_cards` seeds `1..=12` of them).
const UNIVERSE_NUMBERED: i32 = 12;

/// The dummy catalog's rarity cycle: `catalog::numbered_card` walks
/// `["common", "uncommon", "rare", "mythic"]` by `(n - 1) % 4`, so a rarity is an offset.
const COMMON: i32 = 0;
const UNCOMMON: i32 = 1;
const RARE: i32 = 2;
const MYTHIC: i32 = 3;

/// Stable per-card external id — the same scheme `catalog::card_id` mints.
fn card(set: &str, n: i32) -> String {
    format!("dummy-{set}-{n:04}")
}

/// The seeded card external ids of one rarity in a set, in collector-number order.
fn rarity_cards(set: &str, last: i32, rarity: i32) -> Vec<String> {
    (1..=last)
        .filter(|n| (n - 1) % 4 == rarity)
        .map(|n| card(set, n))
        .collect()
}

/// `(card, weight)` pairs at one weight.
fn evenly(cards: Vec<String>, weight: u32) -> Vec<(String, u32)> {
    cards.into_iter().map(|id| (id, weight)).collect()
}

/// One sheet, its `total_weight` summed from its entries — every dummy card resolves, so
/// the stated total and the stored one agree (a real sheet's can be larger; see
/// [`crate::mtgjson::boosters`]).
fn sheet(
    name: &str,
    foil: bool,
    balance_colors: bool,
    allow_duplicates: bool,
    fixed: bool,
    cards: Vec<(String, u32)>,
) -> RawSheet {
    RawSheet {
        name: name.to_string(),
        foil,
        balance_colors,
        allow_duplicates,
        fixed,
        total_weight: cards.iter().map(|&(_, w)| u64::from(w)).sum(),
        cards,
    }
}

/// One pack variant. Slots are declared **sorted by sheet name**, matching what the real
/// derivation stores (upstream states them as a JSON object, so there is no order to keep).
fn variant(weight: u64, slots: &[(&str, u32)]) -> RawVariant {
    RawVariant {
        weight,
        slots: slots
            .iter()
            .map(|(sheet, count)| ((*sheet).to_string(), *count))
            .collect(),
    }
}

/// The fabricated booster configurations — the single source of truth for the offline
/// booster tables.
pub(super) fn dummy_booster_configs() -> Vec<RawBoosterConfig> {
    let base_commons = rarity_cards("dmb", BASE_NUMBERED, COMMON);
    let base_uncommons = rarity_cards("dmb", BASE_NUMBERED, UNCOMMON);
    let base_rares = rarity_cards("dmb", BASE_NUMBERED, RARE);
    let base_mythics = rarity_cards("dmb", BASE_NUMBERED, MYTHIC);

    // The rare slot is weighted: a rare is twice as likely as a mythic.
    let mut play_rare = evenly(base_rares.clone(), 2);
    play_rare.extend(evenly(base_mythics.clone(), 1));
    // …and it also carries the catalog's foil-only printing, which has no regular price,
    // so the read's "counted as $0" coverage path has something to report offline.
    play_rare.push((card("dmb", BASE_NUMBERED + 4), 2));

    // The foil slot mixes every rarity, commons far likelier than rares, and may repeat a
    // card across packs (`allowDuplicates`).
    let mut play_foil = evenly(base_commons.clone(), 12);
    play_foil.extend(evenly(base_uncommons.clone(), 4));
    play_foil.extend(evenly(base_rares.clone(), 1));

    let mut collector_foil = evenly(base_rares.clone(), 2);
    collector_foil.extend(evenly(base_mythics, 1));

    let universe_commons = rarity_cards("dmu", UNIVERSE_NUMBERED, COMMON);
    let universe_uncommons = rarity_cards("dmu", UNIVERSE_NUMBERED, UNCOMMON);
    let universe_rares = rarity_cards("dmu", UNIVERSE_NUMBERED, RARE);
    let universe_mythics = rarity_cards("dmu", UNIVERSE_NUMBERED, MYTHIC);
    let mut draft_rare = evenly(universe_rares.clone(), 2);
    draft_rare.extend(evenly(universe_mythics, 1));
    let mut draft_foil = evenly(universe_commons.clone(), 12);
    draft_foil.extend(evenly(universe_uncommons.clone(), 4));
    draft_foil.extend(evenly(universe_rares.clone(), 1));

    vec![
        RawBoosterConfig {
            set_code: "dmb".to_string(),
            code: "play".to_string(),
            name: Some("Play Booster".to_string()),
            variants: vec![variant(
                1,
                &[("common", 7), ("foil", 1), ("rare", 1), ("uncommon", 3)],
            )],
            sheets: vec![
                sheet("common", false, true, false, false, evenly(base_commons, 1)),
                sheet("foil", true, false, true, false, play_foil),
                sheet("rare", false, false, false, false, play_rare),
                sheet(
                    "uncommon",
                    false,
                    false,
                    false,
                    false,
                    evenly(base_uncommons, 1),
                ),
            ],
        },
        RawBoosterConfig {
            set_code: "dmb".to_string(),
            code: "collector".to_string(),
            name: Some("Collector Booster".to_string()),
            variants: vec![variant(1, &[("foilRare", 2), ("promo", 1)])],
            sheets: vec![
                sheet("foilRare", true, false, true, false, collector_foil),
                // A fixed sheet: its slot takes the first `count` cards **in order**, so
                // the order here is the data, not a presentation detail.
                sheet(
                    "promo",
                    false,
                    false,
                    false,
                    true,
                    vec![
                        (card("dmb", BASE_NUMBERED + 3), 1),
                        (card("dmb", BASE_NUMBERED + 2), 1),
                    ],
                ),
            ],
        },
        RawBoosterConfig {
            set_code: "dmu".to_string(),
            code: "draft".to_string(),
            name: Some("Draft Booster".to_string()),
            // Two variants, so the weighted roll between configurations is exercised: three
            // packs in four hold no foil.
            variants: vec![
                variant(3, &[("common", 10), ("rare", 1), ("uncommon", 3)]),
                variant(
                    1,
                    &[("common", 10), ("foil", 1), ("rare", 1), ("uncommon", 3)],
                ),
            ],
            sheets: vec![
                sheet(
                    "common",
                    false,
                    true,
                    false,
                    false,
                    evenly(universe_commons, 1),
                ),
                sheet("foil", true, false, true, false, draft_foil),
                sheet("rare", false, false, false, false, draft_rare),
                sheet(
                    "uncommon",
                    false,
                    false,
                    false,
                    false,
                    evenly(universe_uncommons, 1),
                ),
            ],
        },
    ]
}

/// Which fabricated product opens how many of which booster. Product ids match
/// [`super::products::dummy_products`]; `900004` (the commander deck) is deliberately
/// absent, so one seeded product answers "no booster data".
pub(super) fn dummy_sealed_packs() -> Vec<RawPack> {
    [
        ("900002", "dmb", "play", 1),       // one play booster pack
        ("900003", "dmb", "play", 6),       // the bundle's six packs
        ("900001", "dmb", "collector", 12), // a collector booster box
        ("900005", "dmu", "draft", 36),     // a draft booster box
    ]
    .into_iter()
    .map(|(product, set_code, booster_code, quantity)| RawPack {
        tcgplayer_product_id: product.to_string(),
        set_code: set_code.to_string(),
        booster_code: booster_code.to_string(),
        quantity,
    })
    .collect()
}

/// Seed the fabricated booster configurations, sheets and product links, so the expected
/// value and pack opener have data offline. Resolves the fabricated card/product external
/// ids to the internal ids the just-seeded rows carry (so it runs after both are seeded),
/// then hands them to the real rebuild, which replaces the game's rows wholesale — the
/// tables have no external-id upsert key, so a reseed replaces rather than upserts.
/// Returns the number of rows written.
pub(super) async fn seed_boosters(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let configs = dummy_booster_configs();
    let packs = dummy_sealed_packs();

    let card_exts: Vec<String> = configs
        .iter()
        .flat_map(|config| {
            config
                .sheets
                .iter()
                .flat_map(|s| s.cards.iter().map(|(id, _)| id.clone()))
        })
        .collect::<HashSet<String>>()
        .into_iter()
        .collect();
    let card_ids: HashMap<String, i32> = Card::find()
        .select_only()
        .column(card::Column::ExternalId)
        .column(card::Column::Id)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::ExternalId.is_in(card_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    let product_exts: Vec<String> = packs
        .iter()
        .map(|p| p.tcgplayer_product_id.clone())
        .collect();
    let product_ids: HashMap<String, i32> = Product::find()
        .select_only()
        .column(product::Column::ExternalId)
        .column(product::Column::Id)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::ExternalId.is_in(product_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    let now = Utc::now();
    let stats = crate::mtgjson::ingest::boosters::rebuild(
        db,
        &configs,
        &packs,
        &card_ids,
        &product_ids,
        now,
    )
    .await
    // The rebuild only ever fails on the database; keep that category rather than
    // stringifying it into `Other`, whose text reaches `ingest_state.detail`.
    .map_err(|err| match err {
        MtgjsonError::Db(db) => IngestError::Db(db),
        other => IngestError::Other(other.to_string()),
    })?;

    Ok((stats.configs + stats.sheets + stats.packs) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::{BoosterConfig, BoosterSheet, SealedPack};
    use crate::entities::{booster_config, booster_sheet, sealed_pack};
    use crate::test_support::migrated_memory_db;
    use sea_orm::QueryOrder;

    /// The declared configurations are internally sound: distinct identities, every slot
    /// names a sheet the configuration defines, slots are sorted by sheet name (as the real
    /// derivation stores them), and every variant can actually be rolled.
    #[test]
    fn declared_configs_are_consistent() {
        let configs = dummy_booster_configs();
        let ids: HashSet<(&str, &str)> = configs
            .iter()
            .map(|c| (c.set_code.as_str(), c.code.as_str()))
            .collect();
        assert_eq!(ids.len(), configs.len(), "identities must be unique");

        for config in &configs {
            let sheets: HashSet<&str> = config.sheets.iter().map(|s| s.name.as_str()).collect();
            assert_eq!(sheets.len(), config.sheets.len(), "sheet names are unique");
            assert!(!config.variants.is_empty());
            for variant in &config.variants {
                assert!(variant.weight > 0, "a weight-0 variant can never be rolled");
                let mut sorted = variant.slots.clone();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                assert_eq!(variant.slots, sorted, "slots are stored sorted by sheet");
                for (name, count) in &variant.slots {
                    assert!(*count > 0);
                    assert!(
                        sheets.contains(name.as_str()),
                        "{} names undefined sheet {name}",
                        config.code
                    );
                }
            }
            for sheet in &config.sheets {
                assert!(!sheet.cards.is_empty(), "{} has an empty sheet", sheet.name);
                assert!(sheet.cards.iter().all(|&(_, w)| w > 0));
                assert_eq!(
                    sheet.total_weight,
                    sheet.cards.iter().map(|&(_, w)| u64::from(w)).sum::<u64>()
                );
            }
        }

        // The shapes the read has to handle are all present offline.
        assert!(configs.iter().any(|c| c.variants.len() > 1));
        assert!(
            configs
                .iter()
                .any(|c| c.sheets.iter().any(|s| s.fixed && s.cards.len() > 1))
        );
        assert!(
            configs
                .iter()
                .any(|c| c.sheets.iter().any(|s| s.foil && s.allow_duplicates))
        );
    }

    /// Every declared card resolves and the sheets land with their weights, flags and
    /// order intact; the fixed sheet keeps the order it was declared in.
    #[tokio::test]
    async fn seeded_boosters_resolve_and_keep_their_shape() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");

        let configs = BoosterConfig::find()
            .order_by_asc(booster_config::Column::SetCode)
            .order_by_asc(booster_config::Column::Code)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(configs.len(), 3);
        let collector = &configs[0];
        assert_eq!(
            (collector.set_code.as_str(), collector.code.as_str()),
            ("dmb", "collector")
        );
        assert_eq!(collector.name.as_deref(), Some("Collector Booster"));
        assert_eq!(collector.total_weight, 1);
        let draft = configs
            .iter()
            .find(|c| c.code == "draft")
            .expect("the draft booster");
        assert_eq!(
            draft.total_weight, 4,
            "the total is Σ of the stored variants' weights"
        );
        assert_eq!(draft.variants().len(), 2);

        // Every declared card is in the catalog, so nothing was dropped on the way in.
        for declared in dummy_booster_configs() {
            let stored_config = configs
                .iter()
                .find(|c| c.set_code == declared.set_code && c.code == declared.code)
                .expect("every declared configuration is stored");
            for declared_sheet in &declared.sheets {
                let stored = BoosterSheet::find()
                    .filter(booster_sheet::Column::ConfigId.eq(stored_config.id))
                    .filter(booster_sheet::Column::Name.eq(declared_sheet.name.clone()))
                    .one(&db)
                    .await
                    .unwrap()
                    .expect("every declared sheet is stored");
                assert_eq!(
                    stored.cards().len(),
                    declared_sheet.cards.len(),
                    "{}/{} loses a card to the catalog",
                    declared.code,
                    declared_sheet.name
                );
                assert_eq!(
                    stored
                        .cards()
                        .iter()
                        .map(|&(_, w)| u64::from(w))
                        .sum::<u64>(),
                    stored.total_weight as u64
                );
                assert_eq!(stored.foil, declared_sheet.foil);
                assert_eq!(stored.fixed, declared_sheet.fixed);
                assert_eq!(stored.allow_duplicates, declared_sheet.allow_duplicates);
            }
        }

        // The fixed sheet's order is its data: the prerelease promo leads.
        let promo = BoosterSheet::find()
            .filter(booster_sheet::Column::ConfigId.eq(collector.id))
            .filter(booster_sheet::Column::Name.eq("promo"))
            .one(&db)
            .await
            .unwrap()
            .expect("the fixed promo sheet");
        let first = promo.cards().first().map(|&(id, _)| id).expect("a card");
        let leading = Card::find_by_id(first)
            .one(&db)
            .await
            .unwrap()
            .expect("the leading card");
        assert_eq!(leading.external_id, card("dmb", BASE_NUMBERED + 3));
    }

    /// The play booster's rare sheet carries the catalog's foil-only printing, which has no
    /// regular price — the unpriced path the coverage number reports on.
    #[tokio::test]
    async fn a_sheet_carries_an_unpriced_card() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");

        let unpriced = Card::find()
            .filter(card::Column::ExternalId.eq(card("dmb", BASE_NUMBERED + 4)))
            .one(&db)
            .await
            .unwrap()
            .expect("the foil-only printing is seeded");
        assert!(unpriced.price_usd.is_none(), "it has no regular price");

        let play = BoosterConfig::find()
            .filter(booster_config::Column::Code.eq("play"))
            .one(&db)
            .await
            .unwrap()
            .expect("the play booster");
        let rare = BoosterSheet::find()
            .filter(booster_sheet::Column::ConfigId.eq(play.id))
            .filter(booster_sheet::Column::Name.eq("rare"))
            .one(&db)
            .await
            .unwrap()
            .expect("its rare sheet");
        assert!(!rare.foil, "a non-foil sheet prices at the regular price");
        assert!(rare.cards().iter().any(|&(id, _)| id == unpriced.id));
    }

    /// The product links carry the quantities the sealed catalog states, and the commander
    /// deck gets none — a product with no booster data must answer `null`, not zero.
    #[tokio::test]
    async fn products_link_their_packs_and_one_is_deliberately_unlinked() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");

        let products: HashMap<i32, String> = Product::find()
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| (p.id, p.external_id))
            .collect();
        let links = SealedPack::find()
            .order_by_asc(sealed_pack::Column::Quantity)
            .all(&db)
            .await
            .unwrap();
        let by_product: HashMap<&str, i32> = links
            .iter()
            .map(|l| (products[&l.product_id].as_str(), l.quantity))
            .collect();
        assert_eq!(by_product.get("900002"), Some(&1));
        assert_eq!(by_product.get("900003"), Some(&6));
        assert_eq!(by_product.get("900001"), Some(&12));
        assert_eq!(by_product.get("900005"), Some(&36));
        assert!(
            !by_product.contains_key("900004"),
            "the commander deck opens no boosters"
        );
    }

    /// Reseeding replaces rather than duplicating — the tables are rebuilt wholesale, so a
    /// dev who reruns the seed doesn't end up with two copies of every sheet.
    #[tokio::test]
    async fn seeding_twice_is_idempotent() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");
        let counts = |db: &DatabaseConnection| {
            let db = db.clone();
            async move {
                (
                    BoosterConfig::find().all(&db).await.unwrap().len(),
                    BoosterSheet::find().all(&db).await.unwrap().len(),
                    SealedPack::find().all(&db).await.unwrap().len(),
                )
            }
        };
        let first = counts(&db).await;
        assert!(first.0 > 0 && first.1 > 0 && first.2 > 0);
        super::super::seed(&db).await.expect("reseed dummy catalog");
        assert_eq!(counts(&db).await, first);
    }
}
