//! Curated **MSRP** (manufacturer's suggested retail price) for sealed products.
//!
//! No feed we ingest carries sealed-product MSRP — TCGCSV ships market prices only, and
//! MTGJSON's `sealedProduct` has no price field — yet WotC publishes MSRP on its product
//! announcements. So the values here are hand-curated and embedded at compile time
//! (`msrp.json`, `include_str!`-ed like [`crate::mtgjson::fallback`]'s `fallback_sealed.json`
//! and [`crate::scryfall::drops`]'s `sld_drops.json`), keyed by **TCGplayer product id**
//! (`products.external_id`). The daily TCGCSV product sweep sets each product's `msrp`
//! column from this map (see [`super::ingest::build_group_products`]); a product not listed
//! here gets `NULL` and the SPA hides its MSRP line.
//!
//! [`version`] is a content hash of the embedded file, folded into the products sync's
//! version gate (see [`super::ingest`]) so editing this file re-applies MSRP on the next
//! sync even when TCGCSV itself is unchanged — the same coupling
//! [`crate::mtgjson::fallback`] uses for its curated snapshot.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The committed MSRP snapshot, embedded at compile time.
const MSRP_JSON: &str = include_str!("msrp.json");

/// One curated MSRP entry. `msrp` is the only load-bearing field (a decimal string like
/// the market prices, e.g. `"179.99"`); `name` and `source` are documentation only — a
/// human label and the citation (a WotC product page / announcement URL) so each entry is
/// self-explanatory and auditable.
#[derive(Debug, Deserialize)]
struct MsrpEntry {
    msrp: String,
    #[serde(default)]
    #[allow(dead_code)] // documentation only (self-describing entries); not read at runtime
    name: String,
    #[serde(default)]
    #[allow(dead_code)] // documentation only (citation URL); not read at runtime
    source: Option<String>,
}

/// The parsed file: `{ "<tcgplayerProductId>": { "msrp": "…", "name": "…", "source": "…" } }`.
/// A raw map (not a wrapper) so entries can't collide on product id and lookups are direct.
/// A malformed committed file degrades to "no MSRP" rather than taking the sync down
/// ([`bundled_data_is_valid`] guards the shipped file at test time).
static DATA: LazyLock<HashMap<String, MsrpEntry>> = LazyLock::new(|| {
    serde_json::from_str(MSRP_JSON).unwrap_or_else(|err| {
        tracing::error!(error = %err, "failed to parse msrp.json; MSRP disabled");
        HashMap::new()
    })
});

/// The ingest-facing lookup: `tcgplayer product id -> msrp string`, built once from
/// [`DATA`]. Keys that don't parse as an `i64` are dropped (guarded by
/// [`bundled_data_is_valid`], so this never silently loses a real entry).
static PRICE_MAP: LazyLock<HashMap<i64, String>> = LazyLock::new(|| {
    DATA.iter()
        .filter_map(|(id, entry)| id.parse::<i64>().ok().map(|id| (id, entry.msrp.clone())))
        .collect()
});

/// 64 bits of a SHA-256 over the raw file — any edit changes it, which is all the products
/// sync's version gate needs to detect an MSRP-data change.
static VERSION: LazyLock<String> =
    LazyLock::new(|| hex::encode(&Sha256::digest(MSRP_JSON.as_bytes())[..8]));

/// The curated MSRP map keyed by TCGplayer product id (built once, on first use).
pub fn price_map() -> &'static HashMap<i64, String> {
    &PRICE_MAP
}

/// A stable content hash of the bundled MSRP file. The products ingest folds it into its
/// version gate so an MSRP-only edit still re-applies on the next sync.
pub fn version() -> &'static str {
    &VERSION
}

/// Whether `s` is a well-formed USD price string: a positive amount with exactly two
/// decimal places (e.g. `"179.99"`, `"5.99"`), matching how market prices are stored. Kept
/// strict so a typo in the committed file (`"17999"`, `"$5.99"`, `"5.9"`) fails CI.
#[cfg(test)]
fn is_valid_price(s: &str) -> bool {
    let Some((whole, frac)) = s.split_once('.') else {
        return false;
    };
    if frac.len() != 2
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    s.parse::<f64>().is_ok_and(|v| v > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped file parses, is non-empty, and every entry is well-formed — every key is
    /// a positive TCGplayer product id and every `msrp` a valid 2-dp price string — so a typo
    /// in the committed data fails CI, not silently at runtime.
    #[test]
    fn bundled_data_is_valid() {
        let data = &*DATA;
        assert!(!data.is_empty(), "msrp.json has entries");
        for (id, entry) in data {
            let parsed = id.parse::<i64>();
            assert!(
                parsed.is_ok_and(|n| n > 0),
                "product id {id:?} is a positive TCGplayer id"
            );
            assert!(
                is_valid_price(&entry.msrp),
                "product {id} ({}) has a valid 2-dp price, got {:?}",
                entry.name,
                entry.msrp
            );
        }
        // Every valid entry survives into the ingest-facing map — no key dropped, and no
        // two distinct id strings collide on the same i64 (e.g. `"100"` vs `"0100"`, which
        // would shrink the map below `data.len()`).
        assert_eq!(price_map().len(), data.len());
        // No product id is *duplicated* in the file. A plain `HashMap` deserialize silently
        // keeps the last value for a repeated key, so counting the entries **as authored**
        // (duplicates included) and comparing to the deduped map makes a copy-paste repeat —
        // which would ship the wrong MSRP — fail CI instead.
        assert_eq!(
            authored_entry_count(),
            data.len(),
            "duplicate product id in msrp.json"
        );
    }

    /// The number of top-level entries in `msrp.json` **as authored**, counting a repeated
    /// product-id key more than once (unlike the deduped [`DATA`] map). Walks the raw JSON
    /// via a `MapAccess` visitor rather than string-matching, so it's robust to formatting.
    fn authored_entry_count() -> usize {
        use serde::de::{Deserializer as _, MapAccess, Visitor};

        struct CountEntries;
        impl<'de> Visitor<'de> for CountEntries {
            type Value = usize;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("the msrp.json object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<usize, A::Error> {
                let mut n = 0;
                while map.next_entry::<String, MsrpEntry>()?.is_some() {
                    n += 1;
                }
                Ok(n)
            }
        }

        serde_json::Deserializer::from_str(MSRP_JSON)
            .deserialize_map(CountEntries)
            .expect("msrp.json parses")
    }

    /// The version hash is non-empty and stable across calls (drives the ingest gate).
    #[test]
    fn version_is_stable() {
        assert!(!version().is_empty());
        assert_eq!(version(), version());
    }

    #[test]
    fn is_valid_price_accepts_two_dp_positive_only() {
        assert!(is_valid_price("179.99"));
        assert!(is_valid_price("5.99"));
        assert!(is_valid_price("1000.00"));
        assert!(!is_valid_price("17999")); // no decimal
        assert!(!is_valid_price("5.9")); // one dp
        assert!(!is_valid_price("5.999")); // three dp
        assert!(!is_valid_price("$5.99")); // currency sign
        assert!(!is_valid_price("0.00")); // non-positive
        assert!(!is_valid_price(".99")); // empty whole part
        assert!(!is_valid_price("abc")); // not a number
    }
}
