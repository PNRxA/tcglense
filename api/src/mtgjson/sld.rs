//! Secret Lair Drop sealed-contents **derivation**.
//!
//! MTGJSON ships many `SLD` (Secret Lair Drop) sealed products with `contents: null`, so
//! their pages show no "Cards in this product" — and unlike ordinary packaging (which
//! [`super::fallback`] hand-authors), a Secret Lair drop's meaningful contents is the
//! specific *list of cards* in that drop. Hand-typing ~30 card lists (and every future
//! drop) would rot, so we **derive** them instead: the app already tracks Secret Lair
//! drops ([`crate::scryfall::drops`], from `sld_drops.json`), each a curated title +
//! its cards' collector numbers. The missing link is *which drop a given SLD sealed
//! product is*, and product names carry it: they're named `Secret Lair Drop: <drop> -
//! <Foil|Non-Foil> Edition`. This module maps a product name to its drop, so the ingest
//! can populate the drop's cards as the product's `contains` contents (foil per edition).
//!
//! It's a **derivation, not a hand-authored list**: a drop that syncs in later auto-maps
//! by name with no code change. A tiny curated [`PRODUCT_DROP_OVERRIDES`] covers the
//! handful whose storefront name differs from Scryfall's gallery title (localised drops).
//! The ingest applies this **only to products MTGJSON (and the fallback) left empty**,
//! mirroring the fallback gate — so upstream stays authoritative and a derived entry
//! self-retires the moment MTGJSON authors the product.

use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::scryfall::drops::{self, Drop, DropTable};

/// The Secret Lair Drop set code (lowercased, as products/cards store it). The only set
/// this derivation applies to — it's the one `sld_drops.json` groups.
pub const SET_CODE: &str = "sld";

/// Curated `TCGplayer product id -> drop slug` overrides for Secret Lair sealed products
/// whose storefront name doesn't match any Scryfall gallery title (e.g. a localised drop
/// sold as "… x NALAC Drop: Nuestra Magia" but listed on the gallery as "Secret Lair
/// Presents: Nuestra Magia"). Kept intentionally tiny — name matching handles the rest;
/// this is the escape hatch the issue anticipated for the genuine oddballs.
const PRODUCT_DROP_OVERRIDES: &[(&str, &str)] = &[
    ("638088", "secret-lair-presents-nuestra-magia"), // Nuestra Magia (SP Non-Foil)
    ("638089", "secret-lair-presents-nuestra-magia"), // Nuestra Magia (SP Rainbow Foil)
];

/// A Secret Lair sealed product resolved to the drop whose cards it contains, plus whether
/// the product is a foil edition (so its cards are recorded foil / non-foil accordingly).
pub struct ProductDrop<'a> {
    pub drop: &'a Drop,
    pub foil: bool,
}

/// Resolve a Secret Lair sealed product to its drop: the curated override wins (by
/// TCGplayer id), else the product name's core title is matched against the drop titles.
/// `None` when neither resolves — the product simply keeps showing no contents (no worse
/// than the `contents: null` status quo), never a wrong drop.
pub fn resolve_product_drop<'a>(
    table: &'a DropTable,
    external_id: &str,
    name: &str,
) -> Option<ProductDrop<'a>> {
    let foil = parse_foil(name).unwrap_or(false);
    if let Some(slug) = override_slug(external_id) {
        if let Some(drop) = table.drop_by_slug(slug) {
            return Some(ProductDrop { drop, foil });
        }
    }
    let core = extract_core_title(name);
    let drop = table.drop_by_title(&drops::normalize_title(&core))?;
    Some(ProductDrop { drop, foil })
}

/// The drop-table for the Secret Lair set, or `None` if the snapshot doesn't cover it.
pub fn table() -> Option<&'static DropTable> {
    drops::table(super::GAME, SET_CODE)
}

fn override_slug(external_id: &str) -> Option<&'static str> {
    PRODUCT_DROP_OVERRIDES
        .iter()
        .find(|(id, _)| *id == external_id)
        .map(|(_, slug)| *slug)
}

/// Whether a Secret Lair product is a foil edition, from its name: an explicit
/// "Non-Foil" wins over a bare "Foil" (many products name both, e.g. "Rainbow Foil"),
/// and `None` when the name says nothing (the caller defaults to non-foil).
fn parse_foil(name: &str) -> Option<bool> {
    let lc = name.to_lowercase();
    if lc.contains("non-foil") || lc.contains("nonfoil") || lc.contains("non foil") {
        Some(false)
    } else if lc.contains("foil") {
        Some(true)
    } else {
        None
    }
}

/// A trailing foil/edition clause: `" - Traditional Foil Edition"`, `" - JP Non-Foil
/// Edition"`, `" (SP Rainbow Foil)"`, `" - Foil Etched Edition"`, … Stripped repeatedly so
/// stacked clauses (`"… (Japanese)"` after a foil clause) all come off.
static FOIL_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\s*[-–(]\s*(jp |sp |eu |japanese )?(traditional foil|rainbow foil|gilded foil|galaxy foil|textured foil|pool party foil|neon ink|foil etched|non-foil|nonfoil|non foil|foil)( edition| version)?\s*\)?\s*(\(japanese\))?\s*$",
    )
    .expect("valid foil-suffix regex")
});

/// A bare trailing `" Edition"` / `" Version"` with no foil word (e.g. "… Compleat
/// Edition"), stripped once after the foil clauses.
static TRAILING_EDITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s+(edition|version)\s*$").expect("valid edition regex"));

/// The leading storefront prefixes, stripped in order (each anchored at the start), to
/// leave the bare drop title: `"Secret Lair Drop: "`, a superdrop segment, `"Secret Lair
/// x "`, etc. Order matters — the most specific prefix comes off first.
static PREFIX_STRIPS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)^secret lair (drop|superdrop)( series)?\s*:\s*",
        r"(?i)^secret lair\s*:\s*",
        r"(?i)^secret lair x\s+",
        r"(?i)^secret lair\s+",
        // A superdrop segment: "<Season> Superdrop[ YYYY]: " / " - " before the drop name.
        r"(?i)^[^-:–]*superdrop( \d{4})?\s*[-:–]\s*",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid prefix regex"))
    .collect()
});

/// A `"Secret Lair x "` appearing *after* the leading prefix (a partner drop nested in a
/// storefront name), removed wherever it occurs.
static INNER_SECRET_LAIR_X: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)secret lair x\s+").expect("valid inner regex"));

/// Reduce a Secret Lair sealed-product name to the bare drop title for matching: strip the
/// trailing foil/edition clause(s), then the leading storefront prefix(es). Punctuation and
/// case are handled downstream by [`drops::normalize_title`], so this only needs to remove
/// the wrapping *words*, not canonicalise them.
fn extract_core_title(name: &str) -> String {
    let mut core = strip_foil_suffix(name);
    for re in PREFIX_STRIPS.iter() {
        core = re.replace(&core, "").into_owned();
    }
    INNER_SECRET_LAIR_X.replace_all(&core, "").trim().to_string()
}

fn strip_foil_suffix(name: &str) -> String {
    let mut core = name.trim().to_string();
    loop {
        let next = FOIL_SUFFIX.replace(&core, "").trim().to_string();
        if next == core {
            break;
        }
        core = next;
    }
    TRAILING_EDITION.replace(&core, "").trim().to_string()
}

/// A stable content hash of everything this derivation reads — the embedded drop snapshot
/// plus the curated overrides — 64 bits of SHA-256. The ingest folds it into its version
/// gate alongside MTGJSON's ETag and the fallback hash, so regenerating `sld_drops.json`
/// (or editing an override) re-runs the derivation on the next sync even when
/// `AllPrintings.json` is byte-identical.
pub fn derivation_version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        let mut hasher = Sha256::new();
        hasher.update(drops::snapshot_json().as_bytes());
        for (id, slug) in PRODUCT_DROP_OVERRIDES {
            hasher.update(id.as_bytes());
            hasher.update(b"=");
            hasher.update(slug.as_bytes());
            hasher.update(b";");
        }
        hex::encode(&hasher.finalize()[..8])
    });
    &VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core(name: &str) -> String {
        extract_core_title(name)
    }

    #[test]
    fn extracts_core_title_across_name_shapes() {
        assert_eq!(core("Secret Lair Drop: Cats of Chaos - Non-Foil Edition"), "Cats of Chaos");
        assert_eq!(core("Secret Lair Drop: Purr Majesty - Traditional Foil Edition"), "Purr Majesty");
        assert_eq!(
            core("Secret Lair Drop: Secret Lair x Garfield: Motivationally Challenged - Non-Foil Edition"),
            "Garfield: Motivationally Challenged",
        );
        assert_eq!(
            core("Secret Lair x FINAL FANTASY: Game Over - JP Non-Foil Edition"),
            "FINAL FANTASY: Game Over",
        );
        assert_eq!(core("Secret Lair Drop: Witch's Familiar - Traditional Foil Edition"), "Witch's Familiar");
        // A superdrop segment between the prefix and the drop name comes off too.
        assert_eq!(
            core("Secret Lair Drop: Summer Superdrop - Mountain, Go - Traditional Foil Edition"),
            "Mountain, Go",
        );
    }

    #[test]
    fn parses_foil_from_edition() {
        assert_eq!(parse_foil("Cats of Chaos - Non-Foil Edition"), Some(false));
        assert_eq!(parse_foil("Cats of Chaos - Traditional Foil Edition"), Some(true));
        assert_eq!(parse_foil("Nuestra Magia (SP Rainbow Foil)"), Some(true));
        // "Non-Foil" wins even when "foil" is a substring of it.
        assert_eq!(parse_foil("Those Non-Foils Just Won't Let Up"), Some(false));
        assert_eq!(parse_foil("A Plain Name"), None);
    }

    #[test]
    fn resolves_product_to_drop_by_name() {
        let table = table().expect("sld drop table present");
        let resolved = resolve_product_drop(table, "700795", "Secret Lair Drop: Cats of Chaos - Non-Foil Edition")
            .expect("resolves");
        assert_eq!(resolved.drop.slug, "cats-of-chaos");
        assert!(!resolved.foil);
        assert_eq!(resolved.drop.collector_numbers, ["2690", "2691", "2692", "2693", "2694"]);

        let foil = resolve_product_drop(table, "700796", "Secret Lair Drop: Cats of Chaos - Traditional Foil Edition")
            .expect("resolves");
        assert!(foil.foil);
    }

    #[test]
    fn curated_override_wins_over_name() {
        let table = table().expect("sld drop table present");
        // The storefront name "… x NALAC Drop: Nuestra Magia" matches no gallery title, but
        // the override maps the TCGplayer id straight to the drop.
        let resolved =
            resolve_product_drop(table, "638088", "Secret Lair x NALAC Drop: Nuestra Magia (SP Non-Foil)")
                .expect("override resolves");
        assert_eq!(resolved.drop.slug, "secret-lair-presents-nuestra-magia");
        assert!(!resolved.foil);
    }

    #[test]
    fn unmatched_product_resolves_to_nothing() {
        let table = table().expect("sld drop table present");
        // A single-card promo that has no gallery drop -> no match (shows nothing, safely).
        assert!(
            resolve_product_drop(table, "554987", "Secret Lair Drop: Secret Lair Promo: Seedborn Muse - Rainbow Foil Edition")
                .is_none()
        );
    }

    #[test]
    fn derivation_version_is_stable_and_nonempty() {
        assert_eq!(derivation_version(), derivation_version());
        assert_eq!(derivation_version().len(), 16); // 8 bytes hex-encoded
    }
}
