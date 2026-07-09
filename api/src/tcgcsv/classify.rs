//! Pure classification of TCGplayer products: telling sealed products from single
//! cards, and deriving a coarse product *type* from the product name.
//!
//! Neither classification is a structured field in TCGCSV, so both are derived here
//! (and unit-tested against real-shaped fixtures) rather than trusted from the feed.

use super::model::ExtendedData;

/// Whether a product is **sealed** (a booster box, bundle, deck, …) rather than a
/// single card. Per TCGCSV's own guidance, a product with a `Rarity` or `Number`
/// entry in its `extendedData` is a card; sealed products have neither. (A `UPC`
/// entry corroborates "sealed" but isn't required, so it isn't relied on here.)
pub fn is_sealed(extended_data: &[ExtendedData]) -> bool {
    !extended_data.iter().any(|e| {
        e.name.eq_ignore_ascii_case("Rarity") || e.name.eq_ignore_ascii_case("Number")
    })
}

/// Derive a coarse product type from the product name via ordered keyword matching
/// (most specific first). Returns one of a small, fixed vocabulary; anything
/// unrecognised falls back to `"other"`. Kept deliberately small and cheap so the
/// stored `product_type` column powers a plain-equality filter.
pub fn classify_product_type(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let has = |needle: &str| n.contains(needle);
    // A "box" or "display" is the multi-pack case; a "pack" is a single pack.
    let is_display = |n: &str| n.contains("display") || n.contains("box");

    // Booster families, each split into the display (box) vs single-pack form. Most
    // specific prefixes first so "Collector Booster Pack" never matches plain
    // "Booster Pack" below.
    if has("collector booster") {
        return if is_display(&n) { "collector_display" } else { "collector_pack" };
    }
    if has("play booster") {
        return if is_display(&n) { "play_display" } else { "play_pack" };
    }
    if has("set booster") {
        return if is_display(&n) { "set_display" } else { "set_pack" };
    }
    if has("draft booster") {
        return if is_display(&n) { "draft_display" } else { "draft_pack" };
    }

    if has("prerelease") {
        return "prerelease";
    }
    if has("commander deck") || has("commander precon") {
        return "commander_deck";
    }
    if has("secret lair") {
        return "secret_lair";
    }
    if has("bundle") || has("fat pack") || has("gift") {
        return "bundle";
    }
    if has("case") {
        return "case";
    }
    if has("starter") || has("welcome") {
        return "starter";
    }
    // Generic booster forms once the specific families above are exhausted.
    if is_display(&n) {
        return "display";
    }
    if has("booster") || has("pack") {
        return "pack";
    }
    "other"
}

/// A booster **family**: products of the same family draw from the same booster pool, so a
/// card can be pulled from any product in a family iff it's on that family's sheets. The
/// families are disjoint by card pool — a collector booster's special-treatment printings
/// (borderless / extended-art / …) never appear on the play/set/draft/jumpstart sheets —
/// which is exactly what makes "exclusive to the collector booster" a meaningful question:
/// a card is exclusive to a family when *no other family in the set* can produce it.
///
/// Derived from the classified [`product_type`](classify_product_type). Non-booster
/// products (decks, bundles, cases, prereleases, …) have no family: they're neither judged
/// for exclusivity nor counted as a comparison pool (a bundle nominally *contains* boosters,
/// but a "gift bundle" that tucks in a collector pack would wrongly mark every collector card
/// as shared, so bundles are deliberately left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoosterFamily {
    /// Collector boosters (the special-treatment sheets).
    Collector,
    /// Play boosters (the modern main-set booster).
    Play,
    /// Set boosters.
    Set,
    /// Draft boosters.
    Draft,
    /// A generic booster line with no family keyword in its name — Jumpstart, Mystery
    /// Booster, etc. (`pack` / `display`). Distinct per set, so treated as one family.
    Generic,
}

/// Every `product_type` slug that represents a draftable booster, i.e. the union of the
/// [`BoosterFamily`] members. A card's booster exclusivity is judged against the booster
/// products of a set whose type is in here but whose family differs from the viewed one.
pub const BOOSTER_PRODUCT_TYPES: &[&str] = &[
    "collector_pack",
    "collector_display",
    "play_pack",
    "play_display",
    "set_pack",
    "set_display",
    "draft_pack",
    "draft_display",
    "pack",
    "display",
];

/// The booster family a `product_type` belongs to, or `None` for a non-booster product.
pub fn booster_family(product_type: &str) -> Option<BoosterFamily> {
    Some(match product_type {
        "collector_pack" | "collector_display" => BoosterFamily::Collector,
        "play_pack" | "play_display" => BoosterFamily::Play,
        "set_pack" | "set_display" => BoosterFamily::Set,
        "draft_pack" | "draft_display" => BoosterFamily::Draft,
        "pack" | "display" => BoosterFamily::Generic,
        _ => return None,
    })
}

impl BoosterFamily {
    /// The booster `product_type` slugs belonging to a family *other* than `self` — the
    /// comparison pool for judging exclusivity to `self`. A card that this product's
    /// booster can pull but none of these can is exclusive to `self`'s family.
    pub fn other_booster_types(self) -> Vec<&'static str> {
        BOOSTER_PRODUCT_TYPES
            .iter()
            .copied()
            .filter(|pt| booster_family(pt) != Some(self))
            .collect()
    }

    /// A representative `product_type` slug for this family — the single-pack form. Lets a
    /// caller name the family with the same `product_type` -> label map the SPA already
    /// uses (`web/src/lib/productType.ts`), so a bundle's exclusive section can be titled
    /// after the *contained* booster's family (e.g. `collector_pack` -> "Collector Booster")
    /// without the SPA knowing which booster the bundle wraps.
    pub fn representative_type(self) -> &'static str {
        match self {
            BoosterFamily::Collector => "collector_pack",
            BoosterFamily::Play => "play_pack",
            BoosterFamily::Set => "set_pack",
            BoosterFamily::Draft => "draft_pack",
            BoosterFamily::Generic => "pack",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extended(names: &[&str]) -> Vec<ExtendedData> {
        names
            .iter()
            .map(|n| ExtendedData {
                name: n.to_string(),
            })
            .collect()
    }

    #[test]
    fn sealed_vs_card_from_extended_data() {
        // A card: has a Rarity and/or Number entry.
        assert!(!is_sealed(&extended(&["Rarity", "Number", "P", "T"])));
        assert!(!is_sealed(&extended(&["Number"])));
        assert!(!is_sealed(&extended(&["Rarity"])));
        // Case-insensitive on the entry name.
        assert!(!is_sealed(&extended(&["rarity"])));

        // Sealed: neither Rarity nor Number (UPC alone, or nothing, is sealed).
        assert!(is_sealed(&extended(&["UPC"])));
        assert!(is_sealed(&extended(&[])));
        assert!(is_sealed(&extended(&["Description"])));
    }

    #[test]
    fn classifies_booster_families() {
        assert_eq!(
            classify_product_type("Murders at Karlov Manor Collector Booster Box"),
            "collector_display"
        );
        assert_eq!(
            classify_product_type("Murders at Karlov Manor Collector Booster Display"),
            "collector_display"
        );
        assert_eq!(
            classify_product_type("Collector Booster Pack"),
            "collector_pack"
        );
        assert_eq!(classify_product_type("Play Booster Box"), "play_display");
        assert_eq!(classify_product_type("Play Booster Pack"), "play_pack");
        assert_eq!(classify_product_type("Set Booster Box"), "set_display");
        assert_eq!(classify_product_type("Set Booster Pack"), "set_pack");
        assert_eq!(classify_product_type("Draft Booster Box"), "draft_display");
        assert_eq!(classify_product_type("Draft Booster Pack"), "draft_pack");
    }

    #[test]
    fn classifies_other_shapes_in_order() {
        assert_eq!(
            classify_product_type("Foundations Prerelease Pack"),
            "prerelease"
        );
        assert_eq!(
            classify_product_type("Bloomburrow Commander Deck - Animated Army"),
            "commander_deck"
        );
        assert_eq!(
            classify_product_type("Secret Lair Drop: Artist Series"),
            "secret_lair"
        );
        assert_eq!(classify_product_type("Bloomburrow Bundle"), "bundle");
        assert_eq!(classify_product_type("Dominaria Fat Pack"), "bundle");
        assert_eq!(classify_product_type("Gift Bundle"), "bundle");
        assert_eq!(
            classify_product_type("Murders at Karlov Manor Booster Box Case"),
            "case"
        );
        assert_eq!(classify_product_type("Starter Kit"), "starter");
        assert_eq!(classify_product_type("Welcome Deck 2017"), "starter");
        // Generic booster forms, once the specific families are exhausted.
        assert_eq!(classify_product_type("Some Booster Display"), "display");
        assert_eq!(classify_product_type("Mystery Booster Pack"), "pack");
        // Fallback: nothing in the vocabulary matches.
        assert_eq!(classify_product_type("Playmat - Jace"), "other");
    }

    #[test]
    fn booster_family_groups_the_booster_types() {
        use BoosterFamily::*;
        assert_eq!(booster_family("collector_pack"), Some(Collector));
        assert_eq!(booster_family("collector_display"), Some(Collector));
        assert_eq!(booster_family("play_pack"), Some(Play));
        assert_eq!(booster_family("play_display"), Some(Play));
        assert_eq!(booster_family("set_pack"), Some(Set));
        assert_eq!(booster_family("draft_display"), Some(Draft));
        assert_eq!(booster_family("pack"), Some(Generic));
        assert_eq!(booster_family("display"), Some(Generic));
        // Non-booster products have no family.
        assert_eq!(booster_family("bundle"), None);
        assert_eq!(booster_family("commander_deck"), None);
        assert_eq!(booster_family("case"), None);
        assert_eq!(booster_family("secret_lair"), None);
        assert_eq!(booster_family("other"), None);
        // Every listed booster type resolves to a family.
        for pt in BOOSTER_PRODUCT_TYPES {
            assert!(booster_family(pt).is_some(), "{pt} should have a family");
        }
    }

    #[test]
    fn other_booster_types_excludes_own_family() {
        // The collector comparison pool is every non-collector booster type — pack + box
        // forms of play/set/draft/generic — and never a collector one (so a collector
        // display/case can't count against a collector pack's exclusivity).
        let others = BoosterFamily::Collector.other_booster_types();
        assert!(!others.contains(&"collector_pack"));
        assert!(!others.contains(&"collector_display"));
        assert!(others.contains(&"play_pack"));
        assert!(others.contains(&"draft_display"));
        assert!(others.contains(&"pack"));
        // Every booster type except the two collector ones.
        assert_eq!(others.len(), BOOSTER_PRODUCT_TYPES.len() - 2);
    }

    #[test]
    fn representative_type_round_trips_through_family() {
        use BoosterFamily::*;
        // Each family's representative slug is a booster type that maps back to that family,
        // so naming the exclusive section by it reuses the SPA's product_type -> label map.
        for family in [Collector, Play, Set, Draft, Generic] {
            assert_eq!(booster_family(family.representative_type()), Some(family));
        }
        assert_eq!(Collector.representative_type(), "collector_pack");
        assert_eq!(Generic.representative_type(), "pack");
    }
}
