//! The fabricated MTG catalog: vocabulary, set definitions, and the per-shape card
//! generators that build the same `ScryfallCard`/`ScryfallSet` values the real
//! importer consumes. Pure data — no DB access, no clock-derived identities.

use super::super::model::{CardFace, Prices, ScryfallCard, ScryfallSet};

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
        usd_etched: None,
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
            // Parity fields the dummy catalog doesn't fabricate default to None/absent.
            ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
            usd_etched: None,
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
            usd_etched: None,
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
pub(super) fn dummy_cards() -> Vec<ScryfallCard> {
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
pub(super) fn dummy_sets(cards: &[ScryfallCard]) -> Vec<ScryfallSet> {
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
}
