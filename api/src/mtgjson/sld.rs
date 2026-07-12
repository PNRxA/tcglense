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
    // Confetti Foil siblings: their "(Confetti Foil)" clause is a finish clause, so
    // [`strip_finish`] removes it and the residual title collides with the base drop's key
    // — name matching can only ever reach the base drop. Pin them by id to their own drop
    // (distinct collector numbers + a curated MSRP override that would otherwise be dead).
    ("656357", "furby-doo-ay-noo-lah-confetti-foil"), // Furby: Doo-ay Noo-lah (Confetti Foil)
    ("656361", "furbys-the-gathering-confetti-foil"), // Furbys: The Gathering (Confetti Foil)
    ("656371", "furby-the-oddbodies-confetti-foil"),  // Furby: The OddBodies (Confetti Foil)
];

/// A curated **random bonus-card pool** shared by the drops of a superdrop. Some Secret Lair
/// superdrops ship **one unpredictable bonus card** with every drop, drawn from a pool that
/// MTGJSON's `AllPrintings` doesn't surface on the affected products — it marks them
/// `other: "Bonus card unknown"`, omits the axis entirely, or authors only a *partial* per-drop
/// `variable` that misses the shared pool. Left alone the pooled cards surface on *no* product;
/// this spells the pool out so it shows as a "may be included" section ([`Membership::Variable`]).
///
/// A pool card is only *possible* (the buyer gets **one** of the pool per drop, e.g. Avatar's
/// Command Tower **or** Fellwar Stone — not both), so the derivation records it `variable`. The
/// ingest is add-only and self-retires **per card**: a card MTGJSON already authored as `variable`
/// for the product is deduplicated away by the row set, so upstream wins for every card it names
/// while a genuinely-additive pool it omits (e.g. FINAL FANTASY's shared Evoke rares) still
/// surfaces — see [`super::ingest::merge::merge_sld_bonus_cards`].
///
/// [`Membership::Variable`]: crate::entities::sealed_content::Membership::Variable
struct RandomBonusPool {
    /// The drops (by slug, as in `sld_drops.json`) whose products each draw **one random** card
    /// from `pool`. Several drops of a superdrop usually share a pool (so they group here).
    drop_slugs: &'static [&'static str],
    /// The pool's possible bonus cards, by collector number in the `sld` set. Recorded at the
    /// resolving product's own foilness (a foil edition ships a foil bonus, a non-foil a non-foil),
    /// mirroring how a drop's own cards are recorded. A number absent from the catalog is skipped.
    pool: &'static [&'static str],
}

/// Curated random bonus-card pools for the handful of Secret Lair superdrops whose shared bonus
/// MTGJSON's `AllPrintings` doesn't already deliver as a `variable` membership. Everything MTGJSON
/// *does* enumerate (the bulk of SLD bonus pools, e.g. A Box of Rocks, Brain Dead, the Marvel
/// singles) is deliberately **left to upstream** — the ingest surfaces those directly and a curated
/// duplicate would only add maintenance surface — so this table stays intentionally tiny, like
/// [`PRODUCT_DROP_OVERRIDES`].
///
/// Every pool below was verified card-for-card against Scryfall's `sld` set (`promo_types`
/// includes `sldbonus`) and the drops' published bonus mechanics:
///  - **Avatar: The Last Airbender** (`7062`/`7063`, Fellwar Stone / Command Tower) — a separate
///    "Bonus Cards" gallery drop no product is named after; MTGJSON marks the drop products
///    `other: "Bonus card unknown"`.
///  - **FINAL FANTASY** (`7004`–`7008`, the five shared Evoke-Elemental rares
///    Solitude/Subtlety/Grief/Fury/Endurance) — any FF drop can contain one at random. MTGJSON
///    authors only each drop's *own* per-drop card (`7001`/`7002`/`7003`) as `variable`, so this
///    shared pool is genuinely additive; the per-card dedup keeps the upstream per-drop card intact
///    alongside it.
///  - **Marvel's Spider-Man** (`7013`–`7021`) — a separate "Bonus Cards" gallery drop; MTGJSON
///    leaves the axis off the drop products.
///  - **TMNT "Totally TubuLair"** (`7077`/`7078`/`7083`, the Slime Against Humanity chase) —
///    MTGJSON marks the drop products `other: "Bonus card unknown"`.
///
/// A number already in **every** listed drop's own gallery (`sld_drops.json`) is excluded: the drop
/// derivation records those `contains`, and the read path collapses a card to its strongest
/// membership, so a `variable` row for them would be shadowed and inert.
/// `random_bonus_pools_have_no_shadowed_numbers` guards this. A drop absent here (or from the drop
/// snapshot) simply shows no bonus pool — never a wrong one.
const RANDOM_BONUS_POOLS: &[RandomBonusPool] = &[
    // Avatar: The Last Airbender superdrop — each drop draws one of Command Tower (7063) /
    // Fellwar Stone (7062), the "Avatar: ...: Bonus Cards" gallery drop no product is named after.
    RandomBonusPool {
        drop_slugs: &[
            "avatar-the-last-airbender-one-with-the-elements",
            "avatar-the-last-airbender-the-ember-island-players",
            "avatar-the-last-airbender-a-lot-to-learn",
            "avatar-the-last-airbender-everything-changed",
            "avatar-the-last-airbender-my-cabbages",
        ],
        pool: &["7062", "7063"],
    },
    // FINAL FANTASY superdrop — any drop can contain one of the five shared Evoke-Elemental rares
    // (7004–7008). MTGJSON authors only each drop's own per-drop card (7001/7002/7003), so this
    // shared pool is added on top; per-card dedup keeps the upstream card too.
    RandomBonusPool {
        drop_slugs: &["final-fantasy-game-over", "final-fantasy-grimoire", "final-fantasy-weapons"],
        pool: &["7004", "7005", "7006", "7007", "7008"],
    },
    // Marvel's Spider-Man superdrop — shared "Bonus Cards" gallery (7013–7021) no product is
    // named after.
    RandomBonusPool {
        drop_slugs: &[
            "marvel-s-spider-man-daily-bugle-breaking-news",
            "marvel-s-spider-man-heroic-deeds",
            "marvel-s-spider-man-mana-symbiote",
            "marvel-s-spider-man-venom-unleashed-colors",
            "marvel-s-spider-man-venom-unleashed-inks",
            "marvel-s-spider-man-villainous-plots",
        ],
        pool: &["7013", "7014", "7015", "7016", "7017", "7018", "7019", "7020", "7021"],
    },
    // TMNT "Totally TubuLair" superdrop — the shared Slime Against Humanity chase (7077/7078/7083)
    // can appear in any drop; MTGJSON marks these products `other: "Bonus card unknown"`.
    RandomBonusPool {
        drop_slugs: &[
            "teenage-mutant-ninja-turtles-vhs-villains",
            "teenage-mutant-ninja-turtles-the-might-mutanimals",
            "teenage-mutant-ninja-turtles-the-last-ronin",
            "featuring-kevin-eastman-colors",
            "featuring-kevin-eastman-inks",
            "featuring-stan-sakai",
        ],
        pool: &["7077", "7078", "7083"],
    },
];

/// The random bonus-card pool for a drop (by slug): the `sld` collector number of every card the
/// drop's unknown bonus card could be. Empty unless the drop is listed in [`RANDOM_BONUS_POOLS`].
pub fn random_bonus_pool(slug: &str) -> Vec<&'static str> {
    RANDOM_BONUS_POOLS
        .iter()
        .filter(|p| p.drop_slugs.contains(&slug))
        .flat_map(|p| p.pool.iter().copied())
        .collect()
}

/// A Secret Lair sealed product resolved to the drop whose cards it contains, plus whether
/// the product is a foil edition (so its cards are recorded foil / non-foil accordingly).
pub struct ProductDrop<'a> {
    pub drop: &'a Drop,
    pub foil: bool,
}

impl<'a> ProductDrop<'a> {
    /// Every collector number whose card this product contains — the drop's own cards. (The shared
    /// random bonus pool is a separate axis, see [`random_bonus_pool`].) All in the `sld` set.
    pub fn collector_numbers(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.drop.collector_numbers.iter().map(String::as_str)
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
            return Some(ProductDrop { drop, foil });
        }
    }
    // Exact title match on the finish-stripped, prefix-stripped core, so a product never
    // resolves to the *wrong* drop — worst case is no match at all.
    let core = strip_prefixes(&base);
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

/// Whether a drop slug can ever be reached by product resolution — either it's a curated
/// id-override target, or its own gallery title round-trips back to it through the same
/// finish/prefix stripping a product name undergoes. A slug that fails both is unreachable
/// dead data: the confetti-foil trap where [`strip_finish`] removes the "(Confetti Foil)"
/// clause that distinguishes a drop from its base sibling, so name matching only ever hits
/// the base. Test-only guard against curated MSRP overrides (or bonus attachments) keyed on
/// a slug no product can resolve to.
#[cfg(test)]
pub fn slug_is_reachable(table: &DropTable, slug: &str) -> bool {
    if PRODUCT_DROP_OVERRIDES.iter().any(|(_, s)| *s == slug) {
        return true;
    }
    let Some(drop) = table.drop_by_slug(slug) else {
        return false;
    };
    let (base, _) = strip_finish(&drop.title);
    let core = strip_prefixes(&base);
    table.drop_by_title(&drops::normalize_title(&core)).is_some_and(|d| d.slug == slug)
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
        for pool in RANDOM_BONUS_POOLS {
            for slug in pool.drop_slugs {
                hasher.update(slug.as_bytes());
                hasher.update(b",");
            }
            hasher.update(b"<-");
            for cn in pool.pool {
                hasher.update(cn.as_bytes());
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
    fn product_contents_are_the_drops_own_cards_only() {
        let table = table().expect("sld drop table present");
        // A resolved product contains exactly its drop's own cards — the shared random bonus pool
        // is a separate axis (see `random_bonus_pool`), never folded into `collector_numbers`.
        let pd = resolve_product_drop(
            table,
            "700795",
            "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
        )
        .expect("resolves");
        let cns: Vec<&str> = pd.collector_numbers().collect();
        assert_eq!(cns, ["2690", "2691", "2692", "2693", "2694"]);
        // Even an Avatar drop resolves to just its own cards; its bonus (7062/7063) lives in the
        // random pool, not its contents.
        let avatar = resolve_product_drop(
            table,
            "0",
            "Secret Lair Drop: Avatar: The Last Airbender: My Cabbages! - Traditional Foil Edition",
        )
        .expect("resolves");
        assert_eq!(avatar.drop.slug, "avatar-the-last-airbender-my-cabbages");
        let cns: Vec<&str> = avatar.collector_numbers().collect();
        assert!(!cns.contains(&"7062") && !cns.contains(&"7063"));
    }

    #[test]
    fn avatar_bonus_is_a_shared_random_pool_across_every_drop() {
        // Each Avatar drop draws one of Command Tower (7063) / Fellwar Stone (7062) — a shared
        // `variable` pool, not two guaranteed cards.
        for slug in [
            "avatar-the-last-airbender-my-cabbages",
            "avatar-the-last-airbender-one-with-the-elements",
            "avatar-the-last-airbender-the-ember-island-players",
        ] {
            let pool = random_bonus_pool(slug);
            assert!(pool.contains(&"7062") && pool.contains(&"7063"), "avatar drop {slug} pool");
        }
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

    #[test]
    fn random_bonus_pools_reference_real_drops_and_are_nonempty() {
        let table = table().expect("sld drop table present");
        let mut slugs = std::collections::HashSet::new();
        for entry in RANDOM_BONUS_POOLS {
            assert!(!entry.pool.is_empty(), "a bonus pool is never empty");
            assert!(!entry.drop_slugs.is_empty(), "a bonus pool attaches to a drop");
            for slug in entry.drop_slugs {
                // Every attached drop must exist in the shipped snapshot, else the pool is dead
                // weight (no product can ever resolve to that slug).
                assert!(
                    table.drop_by_slug(slug).is_some(),
                    "bonus-pool drop slug {slug:?} is present in sld_drops.json"
                );
                assert!(slugs.insert(*slug), "drop slug {slug:?} appears in one pool entry only");
            }
            for cn in entry.pool {
                // Pool numbers are `sld` collector numbers; guard a stray non-numeric typo (all
                // curated entries are plain digits — the derivation would just skip others).
                assert!(cn.bytes().all(|b| b.is_ascii_digit()), "pool number {cn:?} is numeric");
            }
        }
    }

    #[test]
    fn random_bonus_pools_have_no_shadowed_numbers() {
        // A pool number that's in EVERY listed drop's own gallery is recorded `contains` by the
        // drop derivation and would shadow the `variable` row (strongest membership wins), making
        // the entry inert. Curated entries must carry only genuinely-additive bonus cards.
        let table = table().expect("sld drop table present");
        for entry in RANDOM_BONUS_POOLS {
            for cn in entry.pool {
                let in_all = entry.drop_slugs.iter().all(|slug| {
                    table
                        .drop_by_slug(slug)
                        .is_some_and(|d| d.collector_numbers.iter().any(|c| c.as_str() == *cn))
                });
                assert!(
                    !in_all,
                    "pool number {cn:?} is in every gallery of {:?}; it would be shadowed",
                    entry.drop_slugs
                );
            }
        }
    }

    #[test]
    fn random_bonus_pool_resolves_by_slug_and_shares_across_a_superdrop() {
        // A shared-pool superdrop: every FF drop draws from the same five Evoke rares (7004–7008).
        let game_over = random_bonus_pool("final-fantasy-game-over");
        assert_eq!(game_over, ["7004", "7005", "7006", "7007", "7008"]);
        assert_eq!(random_bonus_pool("final-fantasy-weapons"), game_over);
        assert_eq!(random_bonus_pool("final-fantasy-grimoire"), game_over);
        // A drop with no curated pool resolves to nothing (folds to no bonus, never a wrong one).
        assert!(random_bonus_pool("cats-of-chaos").is_empty());
        assert!(random_bonus_pool("no-such-drop").is_empty());
    }
}
