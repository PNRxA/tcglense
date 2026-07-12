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
/// *superdrops* include a fixed set of bonus cards with **every individual drop** in the
/// superdrop — e.g. the Avatar: The Last Airbender superdrop bundled Command Tower +
/// Fellwar Stone (their own gallery drop) into each of its individual drop products. Those
/// bonus cards live in a drop no sealed product is named after, so name-matching alone never
/// attaches them; this folds the bonus drop's cards into every product that resolves to one
/// of `base_slugs`.
struct BonusAttachment {
    /// The gallery drop holding the shared bonus cards (matched by slug — no product is
    /// named after it).
    bonus_slug: &'static str,
    /// The individual drops each product of which also receives the bonus cards, at the
    /// product's own foilness (a foil drop ships foil bonus cards, a non-foil drop non-foil).
    base_slugs: &'static [&'static str],
}

/// A curated **random bonus-card pool** shared by the drops of a superdrop. MTGJSON marks the
/// affected products `other: "Bonus card unknown"`: each ships **one unpredictable bonus card**
/// drawn from a pool MTGJSON never enumerates, so the bonus card surfaces on *no* product. This
/// spells the pool out so it shows as a "may be included" section ([`Membership::Variable`]).
///
/// Unlike [`BonusAttachment`] — a *fixed* set of bonus cards guaranteed with every drop, recorded
/// `contains` — a pool card is only *possible*, so the derivation records it `variable`. Like the
/// other SLD derivations it is a **stopgap**: the ingest skips any product MTGJSON already gave a
/// `variable` row (upstream enumerated the real pool), so an entry self-retires with no code edit.
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

/// Curated random bonus-card pools for Secret Lair drops. MTGJSON marks the affected products
/// `other: "Bonus card unknown"` — a real bonus card the buyer receives, drawn at random from a
/// pool MTGJSON doesn't enumerate on those products. This table names that pool so it surfaces as
/// "may be included".
///
/// Provenance: two corroborating sources. (1) The **authoritative** `mtgjson/mtg-sealed-content`
/// `data/contents/SLD.yaml` `variable` sections — the pools MTGJSON *does* enumerate for sibling
/// products of the same drop. (2) Scryfall's own `sld` **"Bonus Cards" gallery drops** already in
/// the snapshot (`sld_drops.json`), which are *superdrop-wide* pools no single product is named
/// after — e.g. Marvel's Spider-Man `7013`–`7021`, FINAL FANTASY `7004`–`7008` (the five shared
/// Evoke-Elemental rares), the TMNT Slime Against Humanity chase `7077`/`7078`/`7083` — so those
/// attach to *every* drop of the superdrop (web-research-confirmed as shared, not per-drop).
/// Only pure-numeric `sld` printings are included; booster-pack pool options, unverifiable
/// printings, and the fixed (guaranteed) attachments in [`BONUS_CARD_ATTACHMENTS`] (e.g. Avatar's
/// Command Tower + Fellwar Stone, which come with every Avatar drop) are deliberately excluded.
/// Kept curated like [`PRODUCT_DROP_OVERRIDES`]; a drop absent here (or from the drop snapshot)
/// simply shows no bonus pool — never a wrong one.
const RANDOM_BONUS_POOLS: &[RandomBonusPool] = &[
    RandomBonusPool { drop_slugs: &["a-box-of-rocks"], pool: &["507", "511", "512", "523", "535"] },
    RandomBonusPool { drop_slugs: &["absolute-annihilation"], pool: &["645", "642", "629", "686"] },
    RandomBonusPool {
        drop_slugs: &["alien-auroras", "featuring-deathburger", "magiccon-the-gathering"],
        pool: &["818"],
    },
    RandomBonusPool { drop_slugs: &["artist-series-johannes-voss"], pool: &["588"] },
    RandomBonusPool { drop_slugs: &["artist-series-mark-poole"], pool: &["582"] },
    RandomBonusPool {
        drop_slugs: &["bitterblossom-dreams"],
        pool: &["503", "520", "521", "523", "524", "530"],
    },
    RandomBonusPool { drop_slugs: &["black-is-magic"], pool: &["519", "526", "531", "535"] },
    RandomBonusPool {
        drop_slugs: &["brain-dead-creatures", "brain-dead-lands", "brain-dead-staples"],
        pool: &["821", "822", "823", "824"],
    },
    RandomBonusPool {
        drop_slugs: &["brain-dead-new-earth-mentality"],
        pool: &["7107", "7105", "7106", "7108"],
    },
    RandomBonusPool { drop_slugs: &["buggin-out"], pool: &["641", "621", "622"] },
    RandomBonusPool { drop_slugs: &["calling-all-hydra-heads"], pool: &["653", "624", "622"] },
    RandomBonusPool { drop_slugs: &["city-styles"], pool: &["615", "645", "640", "681"] },
    RandomBonusPool { drop_slugs: &["dwarf-fortress-create-new-world"], pool: &["7162", "7161"] },
    RandomBonusPool { drop_slugs: &["eldraine-wonderland"], pool: &["503", "504", "505"] },
    RandomBonusPool { drop_slugs: &["faerie-faerie-faerie-rad"], pool: &["512", "529", "534"] },
    RandomBonusPool {
        drop_slugs: &["featuring-imiri-sakabashira"],
        pool: &["7023", "7024", "7025", "7026"],
    },
    // Each FINAL FANTASY drop's fixed bonus (7001/7002/7003) plus the five shared rare Evoke
    // Elemental reprints (7004–7008, the "FINAL FANTASY: Bonus Cards" gallery drop) found in *any*
    // FF drop — a superdrop-wide pool, so every drop lists all five.
    RandomBonusPool {
        drop_slugs: &["final-fantasy-game-over"],
        pool: &["7001", "7004", "7005", "7006", "7007", "7008"],
    },
    RandomBonusPool {
        drop_slugs: &["final-fantasy-grimoire"],
        pool: &["7003", "7004", "7005", "7006", "7007", "7008"],
    },
    RandomBonusPool {
        drop_slugs: &["final-fantasy-weapons"],
        pool: &["7002", "7004", "7005", "7006", "7007", "7008"],
    },
    RandomBonusPool { drop_slugs: &["flower-power"], pool: &["819"] },
    RandomBonusPool {
        drop_slugs: &["just-some-totally-normal-guys"],
        pool: &["618", "650", "652"],
    },
    RandomBonusPool {
        drop_slugs: &["kaleidoscope-killers"],
        pool: &["520", "522", "523", "525", "526"],
    },
    RandomBonusPool { drop_slugs: &["kamigawa-ink"], pool: &["553"] },
    RandomBonusPool { drop_slugs: &["marvel-s-black-panther"], pool: &["867", "870"] },
    RandomBonusPool { drop_slugs: &["marvel-s-captain-america"], pool: &["863", "870"] },
    RandomBonusPool {
        drop_slugs: &["marvel-s-deadpool-i-fixed-it-you-re-welcome"],
        pool: &["7126", "7127"],
    },
    RandomBonusPool { drop_slugs: &["marvel-s-iron-man"], pool: &["864", "870"] },
    RandomBonusPool { drop_slugs: &["marvel-s-storm"], pool: &["866", "870"] },
    RandomBonusPool { drop_slugs: &["marvel-s-wolverine"], pool: &["865", "870"] },
    // Marvel's Spider-Man superdrop: nine shared bonus cards (the "Marvel's Spider-Man: Bonus
    // Cards" gallery drop, 7013–7021) come with every drop of the superdrop.
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
    RandomBonusPool { drop_slugs: &["math-is-for-blockers"], pool: &["584"] },
    RandomBonusPool { drop_slugs: &["mother-s-day-2021"], pool: &["552", "556", "571"] },
    RandomBonusPool { drop_slugs: &["mschf"], pool: &["670", "669"] },
    RandomBonusPool { drop_slugs: &["ornithological-studies"], pool: &["502", "511", "522"] },
    RandomBonusPool { drop_slugs: &["our-show-is-on-friday-can-you-make-it"], pool: &["516"] },
    RandomBonusPool { drop_slugs: &["pride-across-the-multiverse"], pool: &["530", "534"] },
    RandomBonusPool { drop_slugs: &["restless-in-peace"], pool: &["506", "524", "525", "528"] },
    RandomBonusPool { drop_slugs: &["showcase-kaldheim-part-1"], pool: &["555", "573"] },
    RandomBonusPool { drop_slugs: &["showcase-kaldheim-part-2"], pool: &["557", "566"] },
    RandomBonusPool { drop_slugs: &["showcase-zendikar-revisited"], pool: &["518", "532", "533"] },
    RandomBonusPool { drop_slugs: &["special-guest-fiona-staples"], pool: &["513", "514"] },
    RandomBonusPool { drop_slugs: &["special-guest-matt-jukes"], pool: &["662", "665", "667"] },
    RandomBonusPool {
        drop_slugs: &[
            "spongebob-squarepants-internet-sensation",
            "spongebob-squarepants-lands-under-the-sea",
            "spongebob-squarepants-legends-of-bikini-bottom",
        ],
        pool: &["7012", "7009", "7010", "7011"],
    },
    RandomBonusPool { drop_slugs: &["thalia-beyond-the-helvault"], pool: &["529", "507"] },
    RandomBonusPool {
        drop_slugs: &["the-office-dwight-s-destiny"],
        pool: &["7041", "7042", "7043", "7044"],
    },
    RandomBonusPool { drop_slugs: &["the-path-not-traveled"], pool: &["520", "521", "525", "536"] },
    // Teenage Mutant Ninja Turtles "Totally TubuLair" superdrop: the shared ultra-rare Slime
    // Against Humanity chase (the "Slimes Against Humanity" gallery drop, 7077/7078/7083) can turn
    // up in any of the superdrop's drops. Each drop's own themed reprint is already its `contains`.
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
    RandomBonusPool { drop_slugs: &["twisted-toons"], pool: &["881", "882", "883", "884", "885"] },
    RandomBonusPool { drop_slugs: &["year-of-the-rat"], pool: &["504", "514", "516", "523"] },
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
    fn random_bonus_pool_resolves_by_slug_and_shares_across_a_superdrop() {
        // A shared-pool superdrop: each Brain Dead drop draws from the same four-card pool.
        let creatures = random_bonus_pool("brain-dead-creatures");
        assert_eq!(creatures, ["821", "822", "823", "824"]);
        assert_eq!(random_bonus_pool("brain-dead-staples"), creatures);
        // A drop with no curated pool resolves to nothing (folds to no bonus, never a wrong one).
        assert!(random_bonus_pool("cats-of-chaos").is_empty());
        assert!(random_bonus_pool("no-such-drop").is_empty());
    }
}
