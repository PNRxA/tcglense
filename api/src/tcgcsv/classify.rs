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
}
