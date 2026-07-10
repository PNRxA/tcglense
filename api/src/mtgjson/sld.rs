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

/// A shared "bonus cards" drop plus the base drops that ship it. Some Secret Lair
/// *superdrops* include a fixed set of bonus cards with **every** drop in the superdrop —
/// e.g. the Avatar: The Last Airbender superdrop bundled Command Tower + Fellwar Stone
/// (their own gallery drop) with each of its drops. Those bonus cards live in a drop no
/// sealed product is named after, so name-matching alone never attaches them; this folds
/// the bonus drop's cards into every product that resolves to one of `base_slugs`.
struct BonusAttachment {
    /// The gallery drop holding the shared bonus cards (matched by slug — no product is
    /// named after it).
    bonus_slug: &'static str,
    /// The drops each product of which also receives the bonus cards, at the product's
    /// own foilness (a foil drop ships foil bonus cards, a non-foil drop non-foil).
    base_slugs: &'static [&'static str],
}

/// Curated superdrop bonus-card attachments. Kept intentionally tiny, like
/// [`PRODUCT_DROP_OVERRIDES`]: a superdrop that shares bonus cards across its drops is a
/// genuine oddball name-matching can't express, so it's spelled out here.
const BONUS_CARD_ATTACHMENTS: &[BonusAttachment] = &[BonusAttachment {
    // Avatar: The Last Airbender superdrop — Command Tower (7063) + Fellwar Stone (7062)
    // came with each drop (issue #331).
    bonus_slug: "avatar-the-last-airbender-bonus-cards",
    base_slugs: &[
        "avatar-the-last-airbender-one-with-the-elements",
        "avatar-the-last-airbender-the-ember-island-players",
        "avatar-the-last-airbender-a-lot-to-learn",
        "avatar-the-last-airbender-everything-changed",
        "avatar-the-last-airbender-my-cabbages",
    ],
}];

/// A Secret Lair sealed product resolved to the drop whose cards it contains, plus whether
/// the product is a foil edition (so its cards are recorded foil / non-foil accordingly).
pub struct ProductDrop<'a> {
    pub drop: &'a Drop,
    pub foil: bool,
    /// Shared "bonus card" drops this product also contains (see [`BonusAttachment`]),
    /// recorded at the same foilness as the main drop.
    pub bonus_drops: Vec<&'a Drop>,
}

impl<'a> ProductDrop<'a> {
    /// Every collector number whose card this product contains: the main drop's cards
    /// followed by any attached bonus-card drops'. All resolve within the `sld` set.
    pub fn collector_numbers(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.drop
            .collector_numbers
            .iter()
            .chain(self.bonus_drops.iter().flat_map(|d| d.collector_numbers.iter()))
            .map(String::as_str)
    }
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
    let (base, foil) = strip_finish(name);
    if let Some(slug) = override_slug(external_id) {
        if let Some(drop) = table.drop_by_slug(slug) {
            let bonus_drops = bonus_drops_for(table, &drop.slug);
            return Some(ProductDrop { drop, foil, bonus_drops });
        }
    }
    // Exact title match on the finish-stripped, prefix-stripped core, so a product never
    // resolves to the *wrong* drop — worst case is no match at all.
    let core = strip_prefixes(&base);
    let drop = table.drop_by_title(&drops::normalize_title(&core))?;
    let bonus_drops = bonus_drops_for(table, &drop.slug);
    Some(ProductDrop { drop, foil, bonus_drops })
}

/// The shared bonus-card drops attached to a base drop (by slug), resolved against the
/// table. Empty unless the base drop is listed in [`BONUS_CARD_ATTACHMENTS`]; a bonus slug
/// the snapshot doesn't carry is skipped (degrades to no bonus, never a wrong card).
fn bonus_drops_for<'a>(table: &'a DropTable, base_slug: &str) -> Vec<&'a Drop> {
    BONUS_CARD_ATTACHMENTS
        .iter()
        .filter(|a| a.base_slugs.contains(&base_slug))
        .filter_map(|a| table.drop_by_slug(a.bonus_slug))
        .collect()
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

/// Whether a stripped finish clause denotes a foil printing: an explicit "non-foil" wins
/// over a bare "foil" (a clause often names both, e.g. "Rainbow Foil"); `None` when the
/// clause says nothing about the finish.
fn clause_foil(clause: &str) -> Option<bool> {
    let l = clause.to_lowercase();
    if l.contains("non-foil") || l.contains("nonfoil") || l.contains("non foil") {
        Some(false)
    } else if l.contains("foil") {
        Some(true)
    } else {
        None
    }
}

/// Whether a trailing clause is a finish / edition / language *wrapper* (so it should come
/// off the drop title) rather than part of the title itself. Deliberately **content-agnostic
/// on the finish name** — it keys off "foil"/"edition"/"version"/language words, not a fixed
/// finish list — so a *new* finish (Surge Foil, Halo Foil, Confetti Foil, …) still strips.
fn is_finish_clause(clause: &str) -> bool {
    let l = clause.to_lowercase();
    l.contains("foil")
        || l.contains("edition")
        || l.contains("version")
        || l.contains("japanese")
        || l.contains("english")
}

/// Strip the trailing finish/edition clause(s) from a product name and report its foilness.
/// Handles both the `" - <finish> Edition"` and parenthetical `" (<finish>)"` forms and any
/// finish name (not a fixed list). Foilness is read from the **stripped clause**, so a drop
/// whose *title* contains "foil" (e.g. "Foil-Jumpstart Lands") doesn't mislabel a non-foil
/// product. Returns `(name_without_finish, foil)`.
fn strip_finish(name: &str) -> (String, bool) {
    // Normalise unicode dashes to ASCII so " - " separators and "Non-Foil" match uniformly.
    let mut base: String = name
        .chars()
        .map(|c| match c {
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            other => other,
        })
        .collect();
    base = base.trim().to_string();
    let mut foil: Option<bool> = None;

    // Trailing parenthetical finish/language clauses, e.g. "(SP Rainbow Foil)", "(Japanese)".
    while base.ends_with(')') {
        let Some(open) = base.rfind('(') else { break };
        let inner = base[open + 1..base.len() - 1].to_string();
        if !is_finish_clause(&inner) {
            break;
        }
        foil = foil.or_else(|| clause_foil(&inner));
        base = base[..open].trim_end().to_string();
    }
    // The last " - <clause>" segment, if the clause names a finish (leaving any " - " inside
    // the real title intact).
    if let Some(idx) = base.rfind(" - ") {
        let clause = &base[idx + 3..];
        if is_finish_clause(clause) {
            foil = foil.or_else(|| clause_foil(clause));
            base = base[..idx].trim_end().to_string();
        }
    }
    (base, foil.unwrap_or(false))
}

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

/// Strip the leading storefront prefix(es) to leave the bare drop title. Punctuation and
/// case are handled downstream by [`drops::normalize_title`], so this only removes the
/// wrapping *words*.
fn strip_prefixes(base: &str) -> String {
    let mut core = base.to_string();
    for re in PREFIX_STRIPS.iter() {
        core = re.replace(&core, "").into_owned();
    }
    INNER_SECRET_LAIR_X.replace_all(&core, "").trim().to_string()
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
        for attachment in BONUS_CARD_ATTACHMENTS {
            hasher.update(attachment.bonus_slug.as_bytes());
            hasher.update(b"<-");
            for base in attachment.base_slugs {
                hasher.update(base.as_bytes());
                hasher.update(b",");
            }
            hasher.update(b";");
        }
        hex::encode(&hasher.finalize()[..8])
    });
    &VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bare drop title a product name reduces to (finish clause + storefront prefixes off).
    fn core(name: &str) -> String {
        strip_prefixes(&strip_finish(name).0)
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
        // Content-agnostic finish stripping: a finish NOT in any fixed list still comes off,
        // and a " - " inside the real drop title is preserved.
        assert_eq!(
            core("Secret Lair Drop: Showcase: March of the Machine Vol. 1 - Halo Foil Edition"),
            "Showcase: March of the Machine Vol. 1",
        );
        // A drop title that genuinely ends in "Edition" is NOT over-stripped.
        assert_eq!(
            core("Secret Lair Drop: Marvel's Deadpool: I Fixed It (You're Welcome) Pool Party Edition - Non-Foil Edition"),
            "Marvel's Deadpool: I Fixed It (You're Welcome) Pool Party Edition",
        );
    }

    #[test]
    fn strip_finish_reports_foil_from_the_clause_only() {
        assert_eq!(strip_finish("Cats of Chaos - Non-Foil Edition").1, false);
        assert_eq!(strip_finish("Cats of Chaos - Traditional Foil Edition").1, true);
        assert_eq!(strip_finish("Nuestra Magia (SP Rainbow Foil)").1, true);
        // Any finish name strips, even one not in a fixed list.
        assert_eq!(strip_finish("March of the Machine Vol. 1 - Surge Foil Edition"),
                   ("March of the Machine Vol. 1".to_string(), true));
        // Foil is read from the finish clause, so "foil" inside the drop *title* doesn't
        // mislabel a non-foil product.
        assert_eq!(strip_finish("Foil-Jumpstart Lands - Non-Foil Edition").1, false);
        // No finish clause -> unchanged, defaults non-foil.
        assert_eq!(strip_finish("A Plain Name"), ("A Plain Name".to_string(), false));
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
    fn resolves_non_standard_finishes_and_edition_titles() {
        let table = table().expect("sld drop table present");
        // A finish outside any fixed list ("Halo Foil") still strips, so the product matches
        // its gallery drop (whose title lacks the finish).
        let halo = resolve_product_drop(
            table,
            "493799",
            "Secret Lair Drop: Showcase: March of the Machine Vol. 1 - Halo Foil Edition",
        )
        .expect("halo-foil resolves");
        assert_eq!(halo.drop.slug, "showcase-march-of-the-machine-vol-1");
        assert!(halo.foil);
        // A drop whose gallery title genuinely ends in "Edition" is not over-stripped.
        let pool = resolve_product_drop(
            table,
            "686670",
            "Secret Lair Drop: Marvel's Deadpool: I Fixed It (You're Welcome) Pool Party Edition - Non-Foil Edition",
        )
        .expect("pool-party-edition resolves");
        assert_eq!(pool.drop.slug, "marvel-s-deadpool-i-fixed-it-you-re-welcome-pool-party-edition");
        assert!(!pool.foil);
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
    fn superdrop_bonus_cards_attach_to_every_avatar_drop_product() {
        let table = table().expect("sld drop table present");
        // Each Avatar drop product also contains the shared bonus cards (Command Tower 7063
        // + Fellwar Stone 7062), at the product's own foilness — foil drop, foil bonus.
        let foil = resolve_product_drop(
            table,
            "0",
            "Secret Lair Drop: Avatar: The Last Airbender: My Cabbages! - Traditional Foil Edition",
        )
        .expect("resolves");
        assert_eq!(foil.drop.slug, "avatar-the-last-airbender-my-cabbages");
        assert!(foil.foil);
        let cns: Vec<&str> = foil.collector_numbers().collect();
        // The main drop's cards come first, then the appended bonus cards.
        assert_eq!(cns, ["2295", "2296", "2297", "2298", "2299", "7062", "7063"]);

        // A non-foil drop resolves the same bonus cards; foilness is the product's.
        let non_foil = resolve_product_drop(
            table,
            "0",
            "Secret Lair Drop: Avatar: The Last Airbender: One With the Elements - Non-Foil Edition",
        )
        .expect("resolves");
        assert!(!non_foil.foil);
        let cns: Vec<&str> = non_foil.collector_numbers().collect();
        assert!(cns.contains(&"7062") && cns.contains(&"7063"));
    }

    #[test]
    fn non_superdrop_products_get_no_bonus_cards() {
        let table = table().expect("sld drop table present");
        // A drop not listed in the bonus attachments contains only its own cards.
        let pd = resolve_product_drop(table, "700795", "Secret Lair Drop: Cats of Chaos - Non-Foil Edition")
            .expect("resolves");
        assert!(pd.bonus_drops.is_empty());
        let cns: Vec<&str> = pd.collector_numbers().collect();
        assert_eq!(cns, ["2690", "2691", "2692", "2693", "2694"]);
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
