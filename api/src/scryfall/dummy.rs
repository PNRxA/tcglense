//! Deterministic dummy MTG catalog for offline development, CI, and tests.
//!
//! When `SEED_DUMMY_DATA` is set the server seeds this fabricated catalog instead
//! of streaming Scryfall's ~550 MB bulk file — no network, no images. The fake
//! sets/cards are built as the same `ScryfallSet`/`ScryfallCard` shapes the real
//! importer consumes and then run through the *exact* `ingest::import_sets` /
//! `ingest::map_card` / `ingest::flush_cards` / `ingest::put_state` path, so seeded
//! rows are byte-identical in shape to production rows (collector-number sort key,
//! comma-joined colours, faces JSON) and no upsert column list is duplicated.
//!
//! Everything is **deterministic**: ids, set codes, and collector numbers are fixed
//! (no clock-derived identities), and the fabricated year of daily price history is a
//! per-card *seeded* random walk (the RNG is seeded from the card id), so it looks
//! random yet reseeds to byte-identical values. The upserts key on
//! `(game, external_id)` / `(game, code)`, so re-seeding on every boot overwrites the
//! same rows rather than growing the catalog. Cards carry no image URLs, so the image
//! proxy is never hit and `has_image` resolves to false everywhere.

use chrono::{Duration, Utc};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
};

use super::GAME;
use super::ingest::{self, IngestError, PriceColumns};
use super::model::{CardFace, Prices, ScryfallCard, ScryfallSet};
use crate::entities::prelude::Card;
use crate::entities::{card, card_price_history};

/// Synthetic `ingest_state.source_updated_at` recorded for a dummy seed. It never
/// equals a real Scryfall RFC3339 timestamp, so a later real sync's version gate
/// (`scryfall::ingest::refresh`) sees a mismatch and re-imports — dummy mode never
/// locks out a switch back to real data. The seed runs unconditionally on every boot,
/// so changing the generated data takes effect on the next restart with no version
/// bump needed; this value only needs to stay distinct from a real Scryfall timestamp.
const DUMMY_SOURCE_VERSION: &str = "dummy-seed-v1";

/// A year of fabricated price history seeded per card (one row per day, ending today),
/// so the chart shows a year of movement rather than a single flat point.
const PRICE_HISTORY_DAYS: i64 = 365;

/// Per-step volatility of the fabricated price random walk: each day back in time
/// multiplies the running factor by `1 ±` up to this fraction.
const PRICE_STEP: f64 = 0.04;
/// Per-step mean reversion pulling the walk back toward the card's current price, so a
/// year of daily steps wanders realistically without drifting far from day 0's price.
const PRICE_REVERSION: f64 = 0.02;

/// Colour the generated cards cycle through (Scryfall single-letter code, a display
/// word for the card name, and the mana symbol).
struct Color {
    code: &'static str,
    name: &'static str,
    mana: &'static str,
}

const COLORS: &[Color] = &[
    Color {
        code: "W",
        name: "White",
        mana: "{W}",
    },
    Color {
        code: "U",
        name: "Blue",
        mana: "{U}",
    },
    Color {
        code: "B",
        name: "Black",
        mana: "{B}",
    },
    Color {
        code: "R",
        name: "Red",
        mana: "{R}",
    },
    Color {
        code: "G",
        name: "Green",
        mana: "{G}",
    },
];

const RARITIES: &[&str] = &["common", "uncommon", "rare", "mythic"];
const NOUNS: &[&str] = &[
    "Sentinel",
    "Drake",
    "Golem",
    "Wraith",
    "Phoenix",
    "Elemental",
    "Knight",
    "Serpent",
    "Beast",
    "Sprite",
    "Warden",
    "Hydra",
];
const TYPES: &[&str] = &[
    "Creature — Construct",
    "Instant",
    "Sorcery",
    "Enchantment",
    "Artifact",
    "Creature — Spirit",
];

/// Static definition of a seeded set; `card_count` is derived from [`dummy_cards`].
struct SetDef {
    code: &'static str,
    name: &'static str,
    set_type: &'static str,
    released: &'static str,
    parent: Option<&'static str>,
}

const BASE_SET: SetDef = SetDef {
    code: "dmb",
    name: "Dummy Base Set",
    set_type: "expansion",
    released: "2024-01-15",
    parent: None,
};
const UNIVERSE_SET: SetDef = SetDef {
    code: "dmu",
    name: "Dummy Universe",
    set_type: "expansion",
    released: "2024-06-20",
    parent: None,
};
const TOKEN_SET: SetDef = SetDef {
    code: "tdmb",
    name: "Dummy Base Set Tokens",
    set_type: "token",
    released: "2024-01-15",
    parent: Some("dmb"),
};

/// Number of plain numbered cards in the base set. Kept above `DEFAULT_PAGE_SIZE`
/// (60) so the set view exercises pagination / `has_more`.
const BASE_NUMBERED: i32 = 75;

/// Stable per-card external id, e.g. `dummy-dmb-0007`. Embeds the set code so ids are
/// unique across sets and fixed across reboots (the upsert conflict key).
fn card_id(set_code: &str, n: i32) -> String {
    format!("dummy-{set_code}-{n:04}")
}

/// Deterministic, well-formed decimal price strings. The API stores and returns these
/// verbatim (`Option<String>`), so they only need to look like prices.
fn dummy_prices(n: i32) -> Prices {
    let base = f64::from(n);
    Prices {
        usd: Some(format!("{:.2}", base * 0.25)),
        usd_foil: Some(format!("{:.2}", base * 0.75)),
        eur: Some(format!("{:.2}", base * 0.20)),
        tix: Some(format!("{:.2}", base * 0.03)),
    }
}

/// The fields that vary per generated card; the constant ones (paper, English,
/// non-digital, no images) are filled in by [`SeedCard::into_scryfall`].
struct SeedCard {
    external_id: String,
    /// Gameplay identity shared across a card's printings. `None` for cards with a
    /// single printing; reprints share one id so the "other printings" list groups them.
    oracle_id: Option<String>,
    name: String,
    set_code: &'static str,
    set_name: &'static str,
    released: &'static str,
    collector_number: String,
    rarity: &'static str,
    layout: &'static str,
    mana_cost: Option<String>,
    cmc: Option<f64>,
    type_line: Option<String>,
    colors: Vec<String>,
    prices: Prices,
    card_faces: Option<Vec<CardFace>>,
}

impl SeedCard {
    fn into_scryfall(self) -> ScryfallCard {
        let colors = if self.colors.is_empty() {
            None
        } else {
            Some(self.colors)
        };
        ScryfallCard {
            id: self.external_id,
            oracle_id: self.oracle_id,
            name: self.name,
            lang: "en".to_string(),
            released_at: Some(self.released.to_string()),
            set: self.set_code.to_string(),
            set_name: self.set_name.to_string(),
            collector_number: self.collector_number,
            rarity: Some(self.rarity.to_string()),
            layout: Some(self.layout.to_string()),
            mana_cost: self.mana_cost,
            cmc: self.cmc,
            type_line: self.type_line,
            oracle_text: None,
            power: None,
            toughness: None,
            loyalty: None,
            color_identity: colors.clone(),
            colors,
            digital: Some(false),
            // Paper-only and no images keeps the catalog fully offline.
            games: vec!["paper".to_string()],
            image_uris: None,
            card_faces: self.card_faces,
            prices: Some(self.prices),
        }
    }
}

/// A standard numbered card; its attributes cycle deterministically by number.
fn numbered_card(set: &SetDef, n: i32) -> ScryfallCard {
    let idx = (n - 1) as usize;
    let color = &COLORS[idx % COLORS.len()];
    let rarity = RARITIES[idx % RARITIES.len()];
    let noun = NOUNS[idx % NOUNS.len()];
    let type_line = TYPES[idx % TYPES.len()];
    let generic = (idx % 4) as i64 + 1;
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: None,
        name: format!("Dummy {} {}", color.name, noun),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: n.to_string(),
        rarity,
        layout: "normal",
        mana_cost: Some(format!("{{{generic}}}{symbol}", symbol = color.mana)),
        cmc: Some(generic as f64 + 1.0),
        type_line: Some(type_line.to_string()),
        colors: vec![color.code.to_string()],
        prices: dummy_prices(n),
        card_faces: None,
    }
    .into_scryfall()
}

/// A double-faced (transform) card, exercising the `card_faces` JSON path. No face
/// carries an image, so it stays offline and `has_image` is false.
fn transform_card(set: &SetDef, n: i32) -> ScryfallCard {
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: None,
        name: "Dummy Daybound Werewolf // Dummy Nightbound Wolf".to_string(),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: n.to_string(),
        rarity: "rare",
        layout: "transform",
        // Transform cards carry per-face costs, not a top-level mana cost.
        mana_cost: None,
        cmc: Some(3.0),
        type_line: Some("Creature — Human Werewolf // Creature — Werewolf".to_string()),
        colors: vec!["G".to_string()],
        prices: dummy_prices(n),
        card_faces: Some(vec![
            CardFace {
                name: Some("Dummy Daybound Werewolf".to_string()),
                mana_cost: Some("{2}{G}".to_string()),
                type_line: Some("Creature — Human Werewolf".to_string()),
                oracle_text: None,
                power: None,
                toughness: None,
                loyalty: None,
                image_uris: None,
            },
            CardFace {
                name: Some("Dummy Nightbound Wolf".to_string()),
                mana_cost: Some(String::new()),
                type_line: Some("Creature — Werewolf".to_string()),
                oracle_text: None,
                power: None,
                toughness: None,
                loyalty: None,
                image_uris: None,
            },
        ]),
    }
    .into_scryfall()
}

/// A card whose collector number has no leading digit, so `collector_number_int` is
/// NULL — exercises the NULLS-LAST ordering in `list_set_cards`.
fn special_card(set: &SetDef, n: i32, collector_number: &str, name: &str) -> ScryfallCard {
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: None,
        name: name.to_string(),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: collector_number.to_string(),
        rarity: "mythic",
        layout: "normal",
        mana_cost: Some("{3}{W}".to_string()),
        cmc: Some(4.0),
        type_line: Some("Legendary Creature — Avatar".to_string()),
        colors: vec!["W".to_string()],
        prices: dummy_prices(n),
        card_faces: None,
    }
    .into_scryfall()
}

/// A foil-only printing: no regular `usd` price, only a `usd_foil` one. Some real
/// cards are sold only as foils, so the browse views surface (and price-sort on)
/// the foil price as a fallback — this card exercises that path offline.
fn foil_only_card(set: &SetDef, n: i32) -> ScryfallCard {
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: None,
        name: "Dummy Foil-Only Showcase".to_string(),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: n.to_string(),
        rarity: "rare",
        layout: "normal",
        mana_cost: Some("{2}{R}".to_string()),
        cmc: Some(3.0),
        type_line: Some("Creature — Dragon".to_string()),
        colors: vec!["R".to_string()],
        prices: Prices {
            usd: None,
            usd_foil: Some("19.99".to_string()),
            eur: None,
            tix: None,
        },
        card_faces: None,
    }
    .into_scryfall()
}

/// A token printing (no mana cost, no market price) for the token child set.
fn token_card(set: &SetDef, n: i32) -> ScryfallCard {
    let idx = (n - 1) as usize;
    let color = &COLORS[idx % COLORS.len()];
    let noun = NOUNS[idx % NOUNS.len()];
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: None,
        name: format!("Dummy {} {} Token", color.name, noun),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: n.to_string(),
        rarity: "common",
        layout: "token",
        mana_cost: None,
        cmc: Some(0.0),
        type_line: Some(format!("Token Creature — {noun}")),
        colors: vec![color.code.to_string()],
        prices: Prices {
            usd: None,
            usd_foil: None,
            eur: None,
            tix: None,
        },
        card_faces: None,
    }
    .into_scryfall()
}

/// Shared gameplay identity for the reprinted card, so its printings group as one
/// (mirrors a real Scryfall `oracle_id`; only needs to be stable and distinct).
const REPRINT_ORACLE_ID: &str = "dummy-oracle-reprint-0001";

/// One printing of a card reprinted across sets: every printing shares the same
/// name and `oracle_id` but lives in its own set with its own collector number and
/// price. Seeding two of these gives the card-detail "other printings" list (issue
/// #63) something to show offline.
fn reprint_card(set: &SetDef, n: i32) -> ScryfallCard {
    SeedCard {
        external_id: card_id(set.code, n),
        oracle_id: Some(REPRINT_ORACLE_ID.to_string()),
        name: "Dummy Reprinted Relic".to_string(),
        set_code: set.code,
        set_name: set.name,
        released: set.released,
        collector_number: n.to_string(),
        rarity: "rare",
        layout: "normal",
        mana_cost: Some("{2}".to_string()),
        cmc: Some(2.0),
        type_line: Some("Artifact".to_string()),
        colors: vec![],
        prices: dummy_prices(n),
        card_faces: None,
    }
    .into_scryfall()
}

/// The fabricated card list — the single source of truth for what gets seeded.
fn dummy_cards() -> Vec<ScryfallCard> {
    let mut cards = Vec::new();

    // Base set: enough numbered cards to paginate, plus a double-faced card and two
    // non-numeric collector numbers for edge coverage.
    for n in 1..=BASE_NUMBERED {
        cards.push(numbered_card(&BASE_SET, n));
    }
    cards.push(transform_card(&BASE_SET, BASE_NUMBERED + 1));
    cards.push(special_card(
        &BASE_SET,
        BASE_NUMBERED + 2,
        "★",
        "Dummy Starlit Promo",
    ));
    cards.push(special_card(
        &BASE_SET,
        BASE_NUMBERED + 3,
        "P1",
        "Dummy Prerelease Promo",
    ));
    // A foil-only card (no regular USD price) to exercise the foil-price fallback
    // in the browse views' display and price sort.
    cards.push(foil_only_card(&BASE_SET, BASE_NUMBERED + 4));
    // First printing of a reprinted card (its sibling is in the Universe set below),
    // so the card-detail "other printings" list has something to show offline.
    cards.push(reprint_card(&BASE_SET, BASE_NUMBERED + 5));

    // A second standalone set (a single page).
    for n in 1..=12 {
        cards.push(numbered_card(&UNIVERSE_SET, n));
    }
    // Second printing of the reprinted card, in a different set with a later
    // release date — the newest printing, so it sorts first under the other.
    cards.push(reprint_card(&UNIVERSE_SET, 13));

    // A token child set hanging off the base set (exercises set grouping).
    for n in 1..=5 {
        cards.push(token_card(&TOKEN_SET, n));
    }

    cards
}

/// The fabricated sets; `card_count` is derived from the seeded cards so it always
/// matches them. Takes the card list (rather than calling [`dummy_cards`] itself) so
/// the seed path builds it only once.
fn dummy_sets(cards: &[ScryfallCard]) -> Vec<ScryfallSet> {
    let count = |code: &str| cards.iter().filter(|c| c.set == code).count() as i64;
    [&BASE_SET, &UNIVERSE_SET, &TOKEN_SET]
        .into_iter()
        .map(|def| ScryfallSet {
            id: format!("dummy-set-{}", def.code),
            code: def.code.to_string(),
            name: def.name.to_string(),
            set_type: Some(def.set_type.to_string()),
            released_at: Some(def.released.to_string()),
            card_count: Some(count(def.code)),
            digital: Some(false),
            icon_svg_uri: None,
            parent_set_code: def.parent.map(str::to_string),
        })
        .collect()
}

/// Build a deterministic "random" price series for one currency, indexed by day offset
/// (`series[0]` = today, `series[days - 1]` = a year ago). Day 0 is anchored to the
/// card's current `base` price so the chart's newest point matches the price shown
/// elsewhere; each older day applies a seeded random-walk step (a small shock plus mild
/// mean reversion back toward today's price, so a year of steps wanders without running
/// away). A `None` base (tokens, or a foil-only card's missing regular price) yields an
/// all-`None` series. The walk draws from `rng`, so a reseed with the same per-card seed
/// reproduces byte-identical values; values are clamped to a positive minimum and
/// formatted as 2-decimal strings, matching how real prices are stored.
fn price_walk(base: &Option<String>, rng: &mut StdRng, days: usize) -> Vec<Option<String>> {
    let Some(base_val) = base.as_deref().and_then(|s| s.parse::<f64>().ok()) else {
        return vec![None; days];
    };
    let mut series = Vec::with_capacity(days);
    let mut factor = 1.0_f64;
    for d in 0..days {
        if d > 0 {
            let shock: f64 = rng.random_range(-PRICE_STEP..PRICE_STEP);
            factor *= 1.0 + shock;
            factor += PRICE_REVERSION * (1.0 - factor);
        }
        let price = (base_val * factor).max(0.01);
        series.push(Some(format!("{price:.2}")));
    }
    series
}

/// Seed `PRICE_HISTORY_DAYS` of fabricated daily price history per seeded card,
/// reusing the real `(game, card_id, as_of_date)` unique key and upsert helper. Reads
/// the just-seeded `cards` rows for their ids and base prices (the same shape the live
/// `snapshot_prices` reads), then writes one walked row per card per day ending today.
/// Returns the number of rows written.
///
/// A reseed on the **same day** is byte-identical: the same dates and per-card seed
/// reproduce the same values, and the upsert preserves `created_at`. Like the dummy
/// seed generally, this is upsert-only (never deletes), and the window ends at "today",
/// so a long-lived on-disk dummy DB rebooted on a *later* calendar day re-stamps the
/// shifted older dates with fresh walk values and leaves rows past the year mark in
/// place — harmless drift on fabricated offline data; point `SEED_DUMMY_DATA` at a
/// fresh/dedicated DB as the module already advises.
async fn seed_price_history(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let cards: Vec<PriceColumns> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::PriceUsd)
        .column(card::Column::PriceUsdFoil)
        .column(card::Column::PriceEur)
        .column(card::Column::PriceTix)
        .filter(card::Column::Game.eq(GAME))
        .into_tuple()
        .all(db)
        .await?;

    let today = Utc::now().date_naive();
    let now = Utc::now();
    let days = PRICE_HISTORY_DAYS as usize;
    let mut models: Vec<card_price_history::ActiveModel> = Vec::with_capacity(cards.len() * days);
    for (card_id, usd, usd_foil, eur, tix) in &cards {
        // Seed the walk from the card id so every card has its own reproducible series
        // (independent of iteration order) and a reseed upserts identical values.
        let mut rng = StdRng::seed_from_u64(*card_id as u64);
        let usd_series = price_walk(usd, &mut rng, days);
        let foil_series = price_walk(usd_foil, &mut rng, days);
        let eur_series = price_walk(eur, &mut rng, days);
        let tix_series = price_walk(tix, &mut rng, days);
        for d in 0..days {
            let as_of = ingest::format_date(today - Duration::days(d as i64));
            models.push(card_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                card_id: Set(*card_id),
                as_of_date: Set(as_of),
                price_usd: Set(usd_series[d].clone()),
                price_usd_foil: Set(foil_series[d].clone()),
                price_eur: Set(eur_series[d].clone()),
                price_tix: Set(tix_series[d].clone()),
                created_at: Set(now),
            });
        }
    }

    let total = models.len() as u64;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card_price_history::ActiveModel> =
            iter.by_ref().take(ingest::PRICE_HISTORY_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        ingest::upsert_price_history(db, chunk).await?;
    }
    Ok(total)
}

/// Seed the dummy MTG catalog, recording status in `ingest_state`. On failure the
/// state row is best-effort marked `"error"` (mirroring `super::ingest::refresh`) so
/// `GET /status` stays honest, and the error is returned for the caller to log.
pub async fn seed(db: &DatabaseConnection) -> Result<(), IngestError> {
    match seed_inner(db).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest::put_state(
                db,
                None,
                "error",
                Some(ingest::truncate(&err.to_string(), 500)),
                0,
                0,
                None,
                Some(Utc::now()),
            )
            .await;
            Err(err)
        }
    }
}

async fn seed_inner(db: &DatabaseConnection) -> Result<(), IngestError> {
    let started = Utc::now();
    let cards = dummy_cards();
    let sets = dummy_sets(&cards);
    tracing::info!(
        sets = sets.len(),
        cards = cards.len(),
        "seeding dummy {GAME} catalog"
    );

    // Reuse the real set/card mapping + upsert path so dummy rows are shaped exactly
    // like imported ones (and no on_conflict column list is duplicated here).
    let sets_imported = ingest::import_sets(db, &sets).await?;

    let now = Utc::now();
    let models: Vec<card::ActiveModel> = cards
        .into_iter()
        .map(|c| ingest::map_card(c, now))
        .collect();
    let cards_imported = models.len() as i32;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card::ActiveModel> = iter.by_ref().take(ingest::CARD_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        ingest::flush_cards(db, chunk).await?;
    }

    // Seed a year of price history so the chart shows a real trend offline.
    let history_rows = seed_price_history(db).await?;
    tracing::info!(rows = history_rows, "seeded dummy price history");

    // Record ONE ingest_state row under the same dataset key the real importer uses,
    // because `ingest_status` loads it with `.one()` filtered only by game (not
    // dataset) and so relies on there being exactly one row per game. A synthetic
    // source version lets a later real sync detect the change and re-import. Do not
    // give dummy its own dataset key — that would create a second row and make the
    // status route ambiguous.
    ingest::put_state(
        db,
        Some(DUMMY_SOURCE_VERSION.to_string()),
        "complete",
        Some("seeded dummy offline catalog".to_string()),
        sets_imported,
        cards_imported,
        Some(started),
        Some(Utc::now()),
    )
    .await?;
    tracing::info!(
        sets = sets_imported,
        cards = cards_imported,
        "dummy catalog seed complete"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn set_codes() -> HashSet<String> {
        dummy_sets(&dummy_cards())
            .into_iter()
            .map(|s| s.code)
            .collect()
    }

    #[test]
    fn generators_are_deterministic() {
        let a: Vec<String> = dummy_cards().into_iter().map(|c| c.id).collect();
        let b: Vec<String> = dummy_cards().into_iter().map(|c| c.id).collect();
        assert_eq!(
            a, b,
            "card ids must be stable across calls so reseed is idempotent"
        );
        // Pin concrete values so a change to the id/name scheme is caught, not just
        // within-process equality (which is tautological with no randomness).
        assert_eq!(card_id("dmb", 7), "dummy-dmb-0007");
        let first = &dummy_cards()[0];
        assert_eq!(first.id, "dummy-dmb-0001");
        assert_eq!(first.collector_number, "1");
        assert_eq!(first.name, "Dummy White Sentinel");
    }

    #[test]
    fn external_ids_are_unique() {
        let ids: Vec<String> = dummy_cards().into_iter().map(|c| c.id).collect();
        let unique: HashSet<&String> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "every dummy card needs a unique external id"
        );
    }

    #[test]
    fn every_card_belongs_to_a_seeded_set() {
        let codes = set_codes();
        for card in dummy_cards() {
            assert!(
                codes.contains(&card.set),
                "card {} references unseeded set {}",
                card.id,
                card.set
            );
        }
    }

    #[test]
    fn collector_numbers_unique_within_each_set() {
        for code in set_codes() {
            let mut seen = HashSet::new();
            for card in dummy_cards().into_iter().filter(|c| c.set == code) {
                assert!(
                    seen.insert(card.collector_number.clone()),
                    "duplicate collector number {} in set {code}",
                    card.collector_number,
                );
            }
        }
    }

    #[test]
    fn set_card_count_matches_generated_cards() {
        let cards = dummy_cards();
        for set in dummy_sets(&cards) {
            let n = cards.iter().filter(|c| c.set == set.code).count() as i64;
            assert_eq!(
                set.card_count,
                Some(n),
                "card_count for {} must match seeded cards",
                set.code
            );
        }
    }

    #[test]
    fn base_set_exceeds_one_page() {
        let base = dummy_cards().into_iter().filter(|c| c.set == "dmb").count();
        assert!(
            base > 60,
            "base set ({base}) should exceed one page to exercise pagination"
        );
    }

    #[test]
    fn has_a_multifaced_card() {
        assert!(
            dummy_cards()
                .iter()
                .any(|c| c.card_faces.as_ref().is_some_and(|f| f.len() >= 2)),
            "expected at least one multi-faced card to exercise the faces path",
        );
    }

    #[test]
    fn a_child_set_points_at_its_parent() {
        let sets = dummy_sets(&dummy_cards());
        let codes: HashSet<String> = sets.iter().map(|s| s.code.clone()).collect();
        let child = sets
            .iter()
            .find(|s| s.parent_set_code.is_some())
            .expect("a child set exists");
        let parent = child.parent_set_code.as_ref().unwrap();
        assert!(
            codes.contains(parent),
            "child {}'s parent {parent} must also be seeded",
            child.code
        );
    }

    #[test]
    fn no_card_carries_an_image_url() {
        // The offline guarantee: no image URLs anywhere, so `has_image` is false and
        // the image proxy is never reached.
        for card in dummy_cards() {
            assert!(
                card.image_uris.is_none(),
                "{} must not have a top-level image",
                card.id
            );
            if let Some(faces) = &card.card_faces {
                for face in faces {
                    assert!(
                        face.image_uris.is_none(),
                        "{} face must not have an image",
                        card.id
                    );
                }
            }
        }
    }

    #[test]
    fn some_card_has_a_non_numeric_collector_number() {
        // At least one non-numeric collector number exercises the NULLS-LAST sort.
        assert!(
            dummy_cards().iter().any(|c| c
                .collector_number
                .chars()
                .next()
                .is_some_and(|ch| !ch.is_ascii_digit())),
            "expected a non-numeric collector number",
        );
    }

    #[test]
    fn has_a_foil_only_card() {
        // A card priced only in foil (no regular USD) exercises the browse views'
        // foil-price fallback for both display and the price sort.
        assert!(
            dummy_cards().iter().any(|c| c
                .prices
                .as_ref()
                .is_some_and(|p| p.usd.is_none() && p.usd_foil.is_some())),
            "expected a foil-only card (no usd, has usd_foil)",
        );
    }

    #[test]
    fn has_a_reprinted_card_across_sets() {
        // A card printed in more than one set, sharing its name and oracle id, so the
        // card-detail "other printings" list (issue #63) has something to show offline.
        let cards = dummy_cards();
        let printings: Vec<&ScryfallCard> = cards
            .iter()
            .filter(|c| c.oracle_id.as_deref() == Some(REPRINT_ORACLE_ID))
            .collect();
        assert!(
            printings.len() >= 2,
            "expected the reprinted card to have multiple printings"
        );
        // Same gameplay object: every printing shares the name...
        let name = &printings[0].name;
        assert!(
            printings.iter().all(|c| &c.name == name),
            "reprint printings must share a name"
        );
        // ...but lives in a distinct set (so they're genuinely "other" printings).
        let sets: HashSet<&str> = printings.iter().map(|c| c.set.as_str()).collect();
        assert!(
            sets.len() >= 2,
            "reprint printings must span at least two sets"
        );
    }

    #[test]
    fn price_walk_is_seeded_anchored_and_handles_none() {
        let days = PRICE_HISTORY_DAYS as usize;

        // A `None` base (e.g. a token) yields an all-`None` series of the right length.
        let none = price_walk(&None, &mut StdRng::seed_from_u64(1), days);
        assert_eq!(none.len(), days);
        assert!(none.iter().all(Option::is_none));

        // Day 0 (today) is anchored exactly to the card's current price.
        let series = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(42), days);
        assert_eq!(series.len(), days);
        assert_eq!(series[0].as_deref(), Some("10.00"));

        // Same seed → byte-identical series, so a reseed upserts the same values.
        let a = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(7), days);
        let b = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(7), days);
        assert_eq!(a, b, "same seed must reproduce the same walk");

        // The walk actually moves (not a flat line) and every value is a positive,
        // well-formed 2-decimal string.
        assert!(
            series.iter().flatten().any(|v| v != "10.00"),
            "a year of steps should move off the anchor price"
        );
        for v in series.iter().flatten() {
            let n: f64 = v.parse().expect("price parses as f64");
            assert!(n >= 0.01, "prices stay positive");
            assert_eq!(v.split('.').nth(1).map(str::len), Some(2));
        }
    }

    #[tokio::test]
    async fn seeds_multi_day_price_history_and_reseed_is_idempotent() {
        use crate::entities::prelude::CardPriceHistory;
        use crate::entities::card_price_history;
        use sea_orm::{
            ColumnTrait, Database, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
        };
        use sea_orm_migration::MigratorTrait;

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory sqlite");
        crate::migrator::Migrator::up(&db, None)
            .await
            .expect("run migrations");

        seed(&db).await.expect("seed succeeds");

        let card_count = dummy_cards().len() as u64;
        let expected_rows = card_count * PRICE_HISTORY_DAYS as u64;

        // One history row per (card, day).
        let rows = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(rows, expected_rows, "expected {PRICE_HISTORY_DAYS} days per card");

        // The series spans exactly `PRICE_HISTORY_DAYS` distinct dates.
        let dates: Vec<String> = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::AsOfDate)
            .filter(card_price_history::Column::Game.eq(GAME))
            .distinct()
            .into_tuple()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(dates.len(), PRICE_HISTORY_DAYS as usize);

        // End-to-end anchoring: the newest (today) history point equals the card's
        // current price, confirming day offset 0 maps to the series' first value.
        let today = ingest::format_date(Utc::now().date_naive());
        let sample = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::PriceUsd.is_not_null())
            .one(&db)
            .await
            .unwrap()
            .expect("a card with a usd price");
        let today_row = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .filter(card_price_history::Column::CardId.eq(sample.id))
            .filter(card_price_history::Column::AsOfDate.eq(today))
            .one(&db)
            .await
            .unwrap()
            .expect("a price row dated today");
        assert_eq!(
            today_row.price_usd, sample.price_usd,
            "today's history must equal the card's current price"
        );

        // Reseeding upserts on the unique key rather than duplicating rows.
        seed(&db).await.expect("reseed succeeds");
        let rows_again = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(rows_again, expected_rows, "reseed must not duplicate price history");
    }

    #[tokio::test]
    async fn seed_populates_catalog_and_reseed_is_idempotent() {
        use crate::entities::prelude::{Card, CardSet, IngestState};
        use crate::entities::{card_set, ingest_state};
        use sea_orm::{ColumnTrait, Database, EntityTrait, PaginatorTrait, QueryFilter};
        use sea_orm_migration::MigratorTrait;

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory sqlite");
        crate::migrator::Migrator::up(&db, None)
            .await
            .expect("run migrations");

        seed(&db).await.expect("seed succeeds");

        let expected_cards = dummy_cards().len() as u64;
        let expected_sets = dummy_sets(&dummy_cards()).len() as u64;
        let cards = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards, expected_cards);
        assert_eq!(sets, expected_sets);

        // The status row is one-per-game, marked complete, with honest counts.
        let state = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .one(&db)
            .await
            .unwrap()
            .expect("an ingest_state row");
        assert_eq!(state.status, "complete");
        assert_eq!(state.cards_imported as u64, expected_cards);
        assert_eq!(state.sets_imported as u64, expected_sets);

        // The multi-faced card round-trips with faces JSON and no stored image.
        let dfc = Card::find()
            .filter(card::Column::Layout.eq("transform"))
            .one(&db)
            .await
            .unwrap()
            .expect("a transform card");
        assert!(dfc.card_faces.is_some());
        assert!(dfc.image_normal.is_none() && dfc.image_small.is_none());

        // A non-numeric collector number maps to a NULL collector_number_int — the
        // NULLS-LAST sort the set-cards read path relies on.
        let promo = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::CollectorNumber.eq("★"))
            .one(&db)
            .await
            .unwrap()
            .expect("the ★ promo card");
        assert!(promo.collector_number_int.is_none());

        // Re-seeding upserts the same rows rather than inserting duplicates, and keeps
        // exactly one ingest_state row per game (the status route's `.one()` needs it).
        seed(&db).await.expect("reseed succeeds");
        let cards_again = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets_again = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards_again, expected_cards, "reseed must be idempotent");
        assert_eq!(sets_again, expected_sets, "reseed must not add sets");
        let state_rows = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            state_rows.len(),
            1,
            "reseed must upsert the single status row"
        );
    }
}
