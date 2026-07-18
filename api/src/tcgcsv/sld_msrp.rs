//! Secret Lair Drop **MSRP derivation** — the price a drop listed for on the Secret Lair
//! store at initial sale.
//!
//! Sibling to [`super::msrp`] (hand-curated MSRP keyed by TCGplayer product id) and, in
//! spirit, to [`crate::mtgjson::sld`] (which *derives* a drop product's contents rather than
//! hand-authoring them). No feed carries sealed-product MSRP, and Secret Lair drops can't be
//! enumerated by product id for `msrp.json` — but each individual drop has a known
//! initial-sale price on secretlair.wizards.com. The overwhelming majority listed at a
//! **standard price per foilness** ($29.99 non-foil / $39.99 foil), so we **derive** each
//! drop product's MSRP: resolve the product to its gallery drop (reusing
//! [`crate::mtgjson::sld`]'s name-matching), then take the drop's per-edition initial-sale
//! price. Only genuine individual drops resolve; non-drop `SLD` products (commander decks,
//! bundles, single-card promos) resolve to `None` and stay unpriced.
//!
//! Drops whose initial-sale price **differed** from the standard — premium/galaxy-foil
//! editions, larger or charity drops, price-increase-era drops — are captured per-edition in
//! the committed [`sld_msrp.json`], keyed by drop slug (the slugs in
//! [`crate::scryfall`]'s `sld_drops.json`), each with a citation. An override may set just
//! one edition (`non_foil` or `foil`); the omitted edition falls back to the standard, so a
//! drop whose *foil* edition was a premium galaxy foil while its non-foil sold at $29.99 is
//! expressed exactly. A drop not listed there sold at the standard. Keeping the standard in
//! code (not the file) means a malformed data file degrades to standard-priced, never to a
//! wrong or empty price.
//!
//! [`version`] hashes the standard prices, the overrides file, and
//! [`crate::mtgjson::sld::derivation_version`] (the drop snapshot), and the products ingest
//! folds it into its sync version gate (see [`super::ingest`]) — so editing a price, an
//! override, or the drop snapshot re-applies MSRP on the next sweep even when TCGCSV is
//! unchanged, mirroring [`super::msrp::version`].

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::mtgjson::sld;

/// Standard initial-sale price of an individual Secret Lair Drop's **non-foil** edition —
/// the price the vast majority of individual drops listed at on the Secret Lair store.
/// Per-drop exceptions live in [`sld_msrp.json`]. (WotC Secret Lair; see the module docs.)
const STANDARD_NON_FOIL: &str = "29.99";

/// Standard initial-sale price of an individual Secret Lair Drop's **foil** (traditional /
/// rainbow) edition.
const STANDARD_FOIL: &str = "39.99";

/// The committed per-drop initial-sale MSRP overrides, embedded at compile time.
const OVERRIDES_JSON: &str = include_str!("sld_msrp.json");

/// One curated override: a drop's initial-sale price(s) where they deviated from the
/// standard. `non_foil`/`foil` are each optional — an omitted edition sold at the standard.
/// `name` and `source` are documentation only (a human label and a citation URL).
#[derive(Debug, Deserialize)]
struct OverrideEntry {
    #[serde(default)]
    non_foil: Option<String>,
    #[serde(default)]
    foil: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // documentation only (human label); not read at runtime
    name: String,
    #[serde(default)]
    #[allow(dead_code)] // documentation only (citation URL); not read at runtime
    source: Option<String>,
}

/// The parsed overrides file: `{ "overrides": { "<slug>": { "non_foil": …, "foil": … } } }`.
/// The `"//"` documentation key and any other top-level field are ignored.
#[derive(Debug, Default, Deserialize)]
struct OverridesFile {
    #[serde(default)]
    overrides: HashMap<String, OverrideEntry>,
}

/// `drop slug -> initial-sale override`, built once. A malformed committed file degrades to
/// "no overrides" — every drop then uses the standard price — rather than taking the sync
/// down; [`bundled_data_is_valid`] guards the shipped file at test time.
static OVERRIDES: LazyLock<HashMap<String, OverrideEntry>> = LazyLock::new(|| {
    serde_json::from_str::<OverridesFile>(OVERRIDES_JSON)
        .map(|f| f.overrides)
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "failed to parse sld_msrp.json; SLD overrides disabled");
            HashMap::new()
        })
});

/// Derive the initial-sale MSRP for a sealed product, or `None` when it doesn't apply: not
/// the `SLD` set, no drop snapshot loaded, or the product resolves to no gallery drop (a
/// non-drop `SLD` product — commander deck, bundle, single-card promo — stays unpriced).
pub fn derive(set_code: &str, external_id: &str, name: &str) -> Option<String> {
    if set_code != sld::SET_CODE {
        return None;
    }
    let table = sld::table()?;
    let pd = sld::resolve_product_drop(&table, external_id, name)?;
    Some(price_for(&OVERRIDES, &pd.drop.slug, pd.foil).to_string())
}

/// The initial-sale price for a resolved drop edition: the curated per-drop override for that
/// edition wins, else the standard price for the edition. Pure over its `overrides` argument
/// so the precedence (per-edition override, else standard) is unit-tested with controlled
/// data rather than whatever the shipped file happens to hold.
fn price_for<'a>(overrides: &'a HashMap<String, OverrideEntry>, slug: &str, foil: bool) -> &'a str {
    let over = overrides.get(slug);
    if foil {
        over.and_then(|o| o.foil.as_deref())
            .unwrap_or(STANDARD_FOIL)
    } else {
        over.and_then(|o| o.non_foil.as_deref())
            .unwrap_or(STANDARD_NON_FOIL)
    }
}

/// A stable content hash (64 bits of SHA-256, hex) of everything this derivation reads: the
/// two standard prices, the committed overrides file, and the underlying drop snapshot
/// ([`sld::derivation_version`]). The products ingest folds it into its version gate so an
/// edit to a standard price, an override, or the drop snapshot re-applies MSRP on the next
/// sync even when TCGCSV is byte-identical.
///
/// Computed live (not memoised) because [`sld::derivation_version`] now tracks the *runtime*
/// drop snapshot (the mirror's daily scrape / a consumer's daily import); a cached value would
/// freeze at the first snapshot seen. Evaluated once per sync tick's version check — negligible.
pub fn version() -> String {
    let mut hasher = Sha256::new();
    hasher.update(STANDARD_NON_FOIL.as_bytes());
    hasher.update(b";");
    hasher.update(STANDARD_FOIL.as_bytes());
    hasher.update(b";");
    hasher.update(OVERRIDES_JSON.as_bytes());
    hasher.update(sld::derivation_version().as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(non_foil: Option<&str>, foil: Option<&str>) -> OverrideEntry {
        OverrideEntry {
            non_foil: non_foil.map(str::to_string),
            foil: foil.map(str::to_string),
            name: String::new(),
            source: None,
        }
    }

    #[test]
    fn price_for_uses_standard_without_an_override() {
        let empty = HashMap::new();
        assert_eq!(price_for(&empty, "cats-of-chaos", false), STANDARD_NON_FOIL);
        assert_eq!(price_for(&empty, "cats-of-chaos", true), STANDARD_FOIL);
    }

    #[test]
    fn per_edition_override_wins_else_standard() {
        let mut m = HashMap::new();
        // A drop whose foil edition was a premium galaxy foil above standard, but whose
        // non-foil edition still listed at $29.99 (so it's omitted, falling back to standard).
        m.insert("premium-foil".to_string(), entry(None, Some("59.99")));
        assert_eq!(price_for(&m, "premium-foil", true), "59.99");
        assert_eq!(price_for(&m, "premium-foil", false), STANDARD_NON_FOIL);
        // A drop where both editions deviated from the standard.
        m.insert("both".to_string(), entry(Some("39.99"), Some("49.99")));
        assert_eq!(price_for(&m, "both", false), "39.99");
        assert_eq!(price_for(&m, "both", true), "49.99");
    }

    #[test]
    fn derives_for_a_real_snapshot_drop_and_gates_the_rest() {
        // A real individual drop resolves to Some(_); a non-SLD set and a non-drop SLD
        // product both stay None.
        let cats = "Secret Lair Drop: Cats of Chaos - Non-Foil Edition";
        let promo = "Secret Lair Drop: Secret Lair Promo: Seedborn Muse - Rainbow Foil Edition";
        assert!(derive("sld", "700795", cats).is_some());
        assert!(derive("mkm", "700795", cats).is_none());
        assert!(derive("sld", "554987", promo).is_none());
    }

    #[test]
    fn confetti_foil_products_resolve_to_their_own_drop_and_curated_msrp() {
        // Regression: "(Confetti Foil)" is a finish clause, so name matching strips it and
        // collides with the base drop — these products must be pinned by id to reach their
        // own drop and the curated $59.99 foil MSRP (not the $39.99 standard foil fallback).
        let name = "Secret Lair x Furby: Doo-ay Noo-lah - Confetti Foil Edition";
        assert_eq!(derive("sld", "656357", name).as_deref(), Some("59.99"));
    }

    #[test]
    fn bundled_data_is_valid() {
        // The shipped overrides file parses; every entry keys a real drop slug (a typo'd slug
        // would silently never apply), sets at least one edition, and every price is a
        // positive 2-decimal USD amount — so bad data fails CI, not silently at runtime.
        assert!(is_valid_price(STANDARD_NON_FOIL) && is_valid_price(STANDARD_FOIL));
        let file: OverridesFile =
            serde_json::from_str(OVERRIDES_JSON).expect("sld_msrp.json parses");
        let table = sld::table().expect("sld drop table present");
        for (slug, e) in &file.overrides {
            assert!(
                table.drop_by_slug(slug).is_some(),
                "override slug {slug:?} exists in the drop snapshot"
            );
            assert!(
                sld::slug_is_reachable(&table, slug),
                "override slug {slug:?} is reachable by product resolution (else the price is \
                 dead data — pin the product id in PRODUCT_DROP_OVERRIDES, as the Confetti \
                 Foil drops require)"
            );
            assert!(
                e.non_foil.is_some() || e.foil.is_some(),
                "override {slug:?} sets at least one edition price"
            );
            for price in [e.non_foil.as_deref(), e.foil.as_deref()]
                .into_iter()
                .flatten()
            {
                assert!(
                    is_valid_price(price),
                    "override {slug:?} price {price:?} is a valid 2-dp USD amount"
                );
            }
        }
    }

    #[test]
    fn version_is_stable_and_16_hex_chars() {
        assert_eq!(version(), version());
        assert_eq!(version().len(), 16); // 8 bytes hex-encoded
        assert!(version().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    /// A positive amount with exactly two decimal places, matching how market prices are
    /// stored (e.g. `"29.99"`). Kept strict so a typo in the committed file fails CI.
    fn is_valid_price(s: &str) -> bool {
        let Some((whole, frac)) = s.split_once('.') else {
            return false;
        };
        frac.len() == 2
            && !whole.is_empty()
            && whole.bytes().all(|b| b.is_ascii_digit())
            && frac.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<f64>().is_ok_and(|v| v > 0.0)
    }
}
