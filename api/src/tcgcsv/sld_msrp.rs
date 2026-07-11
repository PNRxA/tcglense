//! Secret Lair Drop **MSRP derivation**.
//!
//! Sibling to [`super::msrp`] (hand-curated MSRP keyed by TCGplayer product id) and, in
//! spirit, to [`crate::mtgjson::sld`] (which *derives* a drop product's contents rather
//! than hand-authoring them): no feed carries sealed-product MSRP, but WotC prices Secret
//! Lair Drops at a **flat standard price per foilness** — the individual-drop non-foil
//! edition is $29.99 and the traditional-foil edition $39.99 across the whole line
//! ([WotC Secret Lair](https://mtg.fandom.com/wiki/Secret_Lair)). So instead of listing
//! every drop's product id in `msrp.json`, we **derive** each individual drop product's
//! MSRP: resolve the product to its gallery drop (reusing [`crate::mtgjson::sld`]'s
//! name-matching) and pick the foil or non-foil standard price. Only genuine individual
//! drops resolve; non-drop `SLD` products (commander decks, bundles, single-card promos)
//! resolve to `None` and stay unpriced — never mispriced. A resolved drop then takes the
//! standard price for its foilness: correct for the line's standard editions, and no worse
//! than the previous `NULL` for the rare premium foil editions that list higher (see the
//! [`OVERRIDES`] note below) — the standard-vs-premium finish isn't a signal we can derive.
//!
//! A curated [`OVERRIDES`] table (slug -> price) is the escape hatch for individual drops
//! whose MSRP deviates from the two standard values. It's intentionally **empty**: the
//! known deviations (e.g. the *Adventures of the Little Witch* foil edition at $59.99, and
//! the "galaxy foil" premium editions) are all *edition*-specific uplifts where the drop's
//! standard non-foil edition still lists at $29.99 — and a slug is shared by every edition
//! of a drop (`resolve_product_drop` reports only a `foil: bool`, which can't tell a
//! premium "galaxy foil" from a standard traditional foil). A slug-only override would
//! therefore misprice the drop's other editions, so it's left as the seam for a genuine
//! *whole-drop* deviation only. The lookup is factored into [`price_for`] so its precedence
//! (override wins, else standard-by-foilness) is unit-tested without shipping such an entry.
//!
//! [`version`] hashes the two standard prices, the overrides, and
//! [`crate::mtgjson::sld::derivation_version`] (the drop snapshot + its overrides), and the
//! products ingest folds it into its sync version gate (see [`super::ingest`]) — so editing
//! a price constant, an override, or the drop snapshot re-applies the derivation on the
//! next sweep even when TCGCSV itself is unchanged, mirroring [`super::msrp::version`].

use std::sync::LazyLock;

use sha2::{Digest, Sha256};

use crate::mtgjson::sld;

/// The standard WotC MSRP of an individual Secret Lair Drop's **non-foil** edition.
const NON_FOIL: &str = "29.99";

/// The standard WotC MSRP of an individual Secret Lair Drop's **traditional-foil** edition.
const FOIL: &str = "39.99";

/// Curated `drop slug -> MSRP` overrides for individual drops whose price deviates from the
/// two standard values across *all* editions. Deliberately empty — see the module docs: the
/// known deviations are edition-specific (only the foil/premium edition is uplifted) and a
/// slug is shared by every edition, so an entry here would misprice a drop's other editions.
/// Reserved for a genuine whole-drop deviation.
const OVERRIDES: &[(&str, &str)] = &[];

/// Derive the MSRP for a sealed product, or `None` when it doesn't apply: not the `SLD`
/// set, no drop snapshot loaded, or the product resolves to no gallery drop (a non-drop
/// `SLD` product — commander deck, bundle, single-card promo — which stays unpriced).
pub fn derive(set_code: &str, external_id: &str, name: &str) -> Option<String> {
    if set_code != sld::SET_CODE {
        return None;
    }
    let table = sld::table()?;
    let pd = sld::resolve_product_drop(table, external_id, name)?;
    Some(price_for(OVERRIDES, &pd.drop.slug, pd.foil).to_string())
}

/// The MSRP for a resolved drop: a curated `overrides` entry for the slug wins, else the
/// standard price for the product's foilness. Pure over its `overrides` argument so the
/// precedence is testable without a (deliberately empty) shipped [`OVERRIDES`] entry.
fn price_for<'a>(overrides: &'a [(&'a str, &'a str)], slug: &str, foil: bool) -> &'a str {
    if let Some((_, price)) = overrides.iter().find(|(s, _)| *s == slug) {
        return price;
    }
    if foil { FOIL } else { NON_FOIL }
}

/// A stable content hash (64 bits of SHA-256, hex) of everything this derivation reads: the
/// two standard prices, the curated overrides, and the underlying drop snapshot
/// ([`sld::derivation_version`]). The products ingest folds it into its version gate so an
/// edit to a price constant, an override, or the drop snapshot re-applies MSRP on the next
/// sync even when TCGCSV is byte-identical.
pub fn version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        let mut hasher = Sha256::new();
        hasher.update(NON_FOIL.as_bytes());
        hasher.update(b";");
        hasher.update(FOIL.as_bytes());
        hasher.update(b";");
        for (slug, price) in OVERRIDES {
            hasher.update(slug.as_bytes());
            hasher.update(b"=");
            hasher.update(price.as_bytes());
            hasher.update(b";");
        }
        hasher.update(sld::derivation_version().as_bytes());
        hex::encode(&hasher.finalize()[..8])
    });
    &VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real product names kept as consts so the long literals don't blow the line width.
    const CATS_NON_FOIL: &str = "Secret Lair Drop: Cats of Chaos - Non-Foil Edition";
    const CATS_FOIL: &str = "Secret Lair Drop: Cats of Chaos - Traditional Foil Edition";

    #[test]
    fn derives_standard_prices_by_foilness() {
        // A real individual drop present in the shipped snapshot resolves to the standard
        // non-foil / foil price by its edition.
        assert_eq!(derive("sld", "700795", CATS_NON_FOIL).as_deref(), Some(NON_FOIL));
        assert_eq!(derive("sld", "700796", CATS_FOIL).as_deref(), Some(FOIL));
    }

    #[test]
    fn non_sld_set_code_is_never_derived() {
        // Gated on the SLD set so a same-named product in another set never gets a price.
        assert!(derive("mkm", "700795", CATS_NON_FOIL).is_none());
    }

    #[test]
    fn unresolvable_sld_product_stays_unpriced() {
        // A non-drop SLD product (a single-card promo with no gallery drop) resolves to no
        // drop, so it gets no MSRP rather than a wrong one.
        let promo = "Secret Lair Drop: Secret Lair Promo: Seedborn Muse - Rainbow Foil Edition";
        assert!(derive("sld", "554987", promo).is_none());
    }

    #[test]
    fn override_wins_over_standard_price() {
        // A curated override for the slug wins over the standard-by-foilness price…
        let overrides = &[("cats-of-chaos", "59.99")][..];
        assert_eq!(price_for(overrides, "cats-of-chaos", true), "59.99");
        assert_eq!(price_for(overrides, "cats-of-chaos", false), "59.99");
        // …while an unlisted slug falls through to the standard price for its foilness.
        assert_eq!(price_for(overrides, "purr-majesty", true), FOIL);
        assert_eq!(price_for(overrides, "purr-majesty", false), NON_FOIL);
    }

    #[test]
    fn version_is_stable_and_16_hex_chars() {
        assert_eq!(version(), version());
        assert_eq!(version().len(), 16); // 8 bytes hex-encoded
        assert!(version().bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
