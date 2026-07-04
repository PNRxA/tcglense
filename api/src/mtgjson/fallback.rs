//! Curated **fallback** sealed-product memberships, merged into `sealed_contents` for
//! products MTGJSON leaves without contents.
//!
//! MTGJSON's `AllPrintings.json` is the authoritative source for which sealed products a
//! card is found in (see [`super`]), but its contents are hand-curated upstream and lag —
//! some products ship with `contents: null` (e.g. Avatar's "Commander's Bundle"), so the
//! cards physically inside them (the Avatar "Eternal" borderless Commander reprints —
//! Sol Ring, Deflecting Swat, …) resolve to no sealed product at all. This module holds a
//! small committed snapshot (`fallback_sealed.json`, embedded like
//! [`crate::scryfall::drops`]'s `sld_drops.json`) that fills those gaps.
//!
//! It is applied **per-product only when MTGJSON emitted zero rows for that product**
//! (see [`super::ingest`]), so MTGJSON always wins where it has data and this file
//! silently steps aside the moment upstream starts describing a product — no code change,
//! no migration. Cards are keyed by `(set, collector_number)` so entries are
//! human-authorable and reviewable; the ingest resolves them to internal ids the same way
//! it resolves MTGJSON rows.
//!
//! [`version`] is a content hash of the embedded file. The ingest folds it into its
//! version gate alongside MTGJSON's ETag, so editing this file re-runs the merge on the
//! next sync even when `AllPrintings.json` itself is unchanged.

use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::entities::sealed_content::Membership;

/// The committed fallback snapshot, embedded at compile time.
const FALLBACK_JSON: &str = include_str!("fallback_sealed.json");

/// The parsed fallback file: a list of products, each with the cards it contains.
#[derive(Debug, Default, Deserialize)]
pub struct FallbackData {
    #[serde(default)]
    pub products: Vec<FallbackProduct>,
}

/// One sealed product's curated contents. `tcgplayer_product_id` resolves to
/// `products.external_id`; `name` is documentation only.
#[derive(Debug, Deserialize)]
pub struct FallbackProduct {
    pub tcgplayer_product_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub contents: Vec<FallbackCard>,
}

/// A curated card membership within a product, keyed by `(set, number)` matching
/// `cards.set_code` / `cards.collector_number`. `name` is documentation only.
#[derive(Debug, Deserialize)]
pub struct FallbackCard {
    pub set: String,
    pub number: String,
    #[serde(default)]
    pub name: String,
    /// `"contains"` | `"booster"` | `"variable"` — see [`Membership`].
    pub membership: String,
    #[serde(default)]
    pub foil: bool,
}

impl FallbackCard {
    /// Parse the `membership` string into the enum, `None` for an unrecognised value
    /// (the ingest skips such a row and logs it; [`bundled_data_is_valid`] guards the
    /// shipped file so this never fires in practice).
    pub fn parsed_membership(&self) -> Option<Membership> {
        match self.membership.as_str() {
            "contains" => Some(Membership::Contains),
            "booster" => Some(Membership::Booster),
            "variable" => Some(Membership::Variable),
            _ => None,
        }
    }
}

static DATA: LazyLock<FallbackData> = LazyLock::new(|| {
    serde_json::from_str(FALLBACK_JSON).unwrap_or_else(|err| {
        // A malformed committed file degrades to "no fallback" rather than taking the
        // sync down; `bundled_data_is_valid` guards the shipped file at test time.
        tracing::error!(error = %err, "failed to parse fallback_sealed.json; fallback disabled");
        FallbackData::default()
    })
});

static VERSION: LazyLock<String> = LazyLock::new(|| {
    // 64 bits of a SHA-256 over the raw bytes — any edit changes it, which is all the
    // version gate needs to detect a fallback-data change.
    hex::encode(&Sha256::digest(FALLBACK_JSON.as_bytes())[..8])
});

/// The parsed fallback data (parsed once, on first use).
pub fn data() -> &'static FallbackData {
    &DATA
}

/// A stable content hash of the bundled fallback file. The ingest stores it next to
/// MTGJSON's ETag so a fallback-only edit still forces a rebuild on the next sync.
pub fn version() -> &'static str {
    &VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped file parses, every entry is well-formed, and every membership string
    /// maps to a real bucket — so a typo in the committed data fails CI, not silently at
    /// runtime.
    #[test]
    fn bundled_data_is_valid() {
        let data = data();
        assert!(!data.products.is_empty(), "fallback file has products");
        for product in &data.products {
            assert!(
                !product.tcgplayer_product_id.trim().is_empty(),
                "product {} has a tcgplayer id",
                product.name
            );
            assert!(
                !product.contents.is_empty(),
                "product {} lists contents",
                product.name
            );
            for card in &product.contents {
                assert!(!card.set.trim().is_empty(), "card {} has a set", card.name);
                assert!(
                    !card.number.trim().is_empty(),
                    "card {} has a number",
                    card.name
                );
                assert!(
                    card.parsed_membership().is_some(),
                    "card {} ({}) has a valid membership, got {:?}",
                    card.name,
                    card.number,
                    card.membership
                );
            }
        }
    }

    /// Pin the reported cards: the Avatar Commander's Bundle covers the borderless
    /// Commander staples, with the guaranteed three as `contains` and the randomised pool
    /// (incl. Deflecting Swat) as `variable`.
    #[test]
    fn covers_avatar_commander_bundle() {
        let bundle = data()
            .products
            .iter()
            .find(|p| p.tcgplayer_product_id == "648686")
            .expect("Commander's Bundle is present");
        let find = |num: &str| bundle.contents.iter().find(|c| c.number == num);
        assert_eq!(
            find("316").map(|c| c.membership.as_str()),
            Some("contains"),
            "Sol Ring (tle #316) is a guaranteed inclusion"
        );
        assert_eq!(
            find("311").map(|c| c.membership.as_str()),
            Some("variable"),
            "Deflecting Swat (tle #311) is a may-be-in inclusion"
        );
    }

    /// The version hash is non-empty and stable across calls (drives the ingest gate).
    #[test]
    fn version_is_stable() {
        assert!(!version().is_empty());
        assert_eq!(version(), version());
    }
}
