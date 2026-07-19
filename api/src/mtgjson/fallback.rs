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
//! no migration. The exception is an entry flagged [`FallbackProduct::supplement`]: its
//! `contents` are merged **even when** upstream describes the product, adding an axis
//! upstream is missing. Supplements are additive by default; one can also set
//! [`FallbackProduct::override_memberships`] when upstream resolves the right cards but
//! assigns them the wrong bucket. That removes contradictory upstream memberships only for
//! the curated cards before inserting their fallback rows — except `booster` rows, an
//! independent pullable-from-boosters axis the override never touches. Cards are keyed by
//! `(set, collector_number)` so entries are human-authorable and reviewable; the ingest
//! resolves them to internal ids the same way it resolves MTGJSON rows.
//!
//! [`version`] is a content hash of the embedded file. The ingest folds it into its
//! version gate alongside MTGJSON's ETag, so editing this file re-runs the merge on the
//! next sync even when `AllPrintings.json` itself is unchanged.

use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::entities::sealed_component::ComponentKind;
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
/// `products.external_id`; `name` is documentation only. `contents` are the per-card
/// memberships (the "found in / may be in" cards); `components` are the structural
/// composition ("what's in the box" line items) — either or both may be authored.
#[derive(Debug, Deserialize)]
pub struct FallbackProduct {
    pub tcgplayer_product_id: String,
    #[serde(default)]
    pub name: String,
    /// Merge this entry's `contents` even when MTGJSON describes the product, instead of
    /// only filling a zero-row gap — for a product whose upstream description is missing
    /// an axis the curated rows carry (e.g. the Avatar Commander's Bundle: a `deck`
    /// reference that originally resolved to no deck data, so its guaranteed/pool cards
    /// vanished, and land packs upstream keeps textual; issue #352). Additive unless
    /// [`Self::override_memberships`] is set: a row upstream also emits dedups away, and
    /// `components` are unaffected — the composition keeps the usual
    /// only-when-upstream-empty gate.
    #[serde(default)]
    pub supplement: bool,
    /// For a [`Self::supplement`] entry, make the fallback membership authoritative for
    /// each card it names: contradictory upstream memberships for the same product/card
    /// are removed before the curated row is inserted — except `booster` rows, which are
    /// an independent axis (a curated guaranteed card can still be pullable from the
    /// product's boosters) and always survive. This is narrower than replacing a
    /// product's contents — upstream-only cards remain untouched — and is intended for a
    /// source that resolves a randomized pool as guaranteed cards (the Avatar Commander's
    /// Bundle). It cannot correct a wrong upstream `booster` row. Has no effect unless
    /// `supplement` is also true.
    #[serde(default)]
    pub override_memberships: bool,
    #[serde(default)]
    pub contents: Vec<FallbackCard>,
    #[serde(default)]
    pub components: Vec<FallbackComponent>,
}

/// A curated composition line item — the fallback analogue of
/// [`crate::mtgjson::model::RawComponent`], for products MTGJSON ships without contents.
/// `kind` / `name` / `quantity` render the line; a `sealed` component optionally links a
/// sub-product by `child_tcgplayer_product_id`, a `card` component a card by
/// `(child_set, child_number)`.
#[derive(Debug, Deserialize)]
pub struct FallbackComponent {
    /// `"sealed"` | `"deck"` | `"card"` | `"other"` — see [`ComponentKind`].
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// How many of the component the product holds; defaults to 1.
    #[serde(default = "default_quantity")]
    pub quantity: i32,
    /// For a `sealed` component: the sub-product to link (its TCGplayer product id).
    #[serde(default)]
    pub child_tcgplayer_product_id: Option<String>,
    /// For a `card` component: the card to link, keyed by `(set, number)` matching
    /// `cards.set_code` / `cards.collector_number`.
    #[serde(default)]
    pub child_set: Option<String>,
    #[serde(default)]
    pub child_number: Option<String>,
}

/// serde default for [`FallbackComponent::quantity`] (a missing count reads as one).
fn default_quantity() -> i32 {
    1
}

impl FallbackComponent {
    /// Parse the `kind` string into the enum, `None` for an unrecognised value (the ingest
    /// skips such a component and logs it; [`bundled_data_is_valid`] guards the shipped file).
    pub fn parsed_kind(&self) -> Option<ComponentKind> {
        match self.kind.as_str() {
            "sealed" => Some(ComponentKind::Sealed),
            "deck" => Some(ComponentKind::Deck),
            "card" => Some(ComponentKind::Card),
            "other" => Some(ComponentKind::Other),
            _ => None,
        }
    }
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
                !product.contents.is_empty() || !product.components.is_empty(),
                "product {} lists contents and/or components",
                product.name
            );
            assert!(
                !product.override_memberships || product.supplement,
                "product {} only overrides memberships as a supplement",
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
            for component in &product.components {
                assert!(
                    component.parsed_kind().is_some(),
                    "component {} has a valid kind, got {:?}",
                    component.name,
                    component.kind
                );
                assert!(
                    !component.name.trim().is_empty(),
                    "component of {} has a name",
                    product.name
                );
                assert!(
                    component.quantity >= 1,
                    "component {} has quantity >= 1",
                    component.name
                );
            }
        }
    }

    /// Pin the reported cards: the Avatar Commander's Bundle covers the borderless
    /// Commander staples, with the guaranteed three as `contains` and the randomised pool
    /// (incl. Deflecting Swat) as `variable` — plus the land packs' printings (5 full-art
    /// Appa + 5 default basics, both finishes each) as `contains`. The entry is a
    /// `supplement`: upstream describes the product but its land packs stay textual, and its
    /// deck now resolves the random pool incorrectly as guaranteed, so these rows merge and
    /// override contradictory memberships (#352).
    #[test]
    fn covers_avatar_commander_bundle() {
        let bundle = data()
            .products
            .iter()
            .find(|p| p.tcgplayer_product_id == "648686")
            .expect("Commander's Bundle is present");
        assert!(
            bundle.supplement,
            "the entry merges despite upstream's incomplete deck description"
        );
        assert!(
            bundle.override_memberships,
            "the curated pool overrides upstream's incorrect guaranteed classification"
        );
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
        let staples: Vec<&FallbackCard> = bundle
            .contents
            .iter()
            .filter(|card| card.set == "tle")
            .collect();
        assert_eq!(
            staples
                .iter()
                .filter(|card| card.membership == "contains")
                .count(),
            3,
            "only Arcane Signet, Sol Ring, and Swiftfoot Boots are guaranteed"
        );
        assert_eq!(
            staples
                .iter()
                .filter(|card| card.membership == "variable")
                .count(),
            10,
            "the remaining ten Commander staples are randomized"
        );
        // The land packs: tla 282-291 in both finishes (20 rows), and none of the
        // Avatar's Journey cycle (292-296), which Wizards excludes from this bundle.
        let lands: Vec<&FallbackCard> = bundle.contents.iter().filter(|c| c.set == "tla").collect();
        assert_eq!(lands.len(), 20, "10 land printings x both finishes");
        for num in 282..=291 {
            let num = num.to_string();
            for foil in [false, true] {
                assert!(
                    lands
                        .iter()
                        .any(|c| c.number == num && c.foil == foil && c.membership == "contains"),
                    "land tla #{num} (foil: {foil}) is a guaranteed inclusion"
                );
            }
        }
        assert!(
            !bundle.contents.iter().any(|c| {
                c.set == "tla" && (292..=296).contains(&c.number.parse::<i32>().unwrap_or(0))
            }),
            "the Avatar's Journey cycle is not in the packs"
        );
    }

    /// Pin the Commander's Bundle composition kept as the safety net: upstream authors a
    /// composition today (so these `components` are gated out and its "1× Commander's
    /// Bundle" deck line + booster links render), but if it ever regresses to
    /// `contents: null` this authored box (9 Play Boosters + 1 Collector Booster, both
    /// linked, + extras) fills the gap again.
    #[test]
    fn covers_avatar_commander_bundle_components() {
        let bundle = data()
            .products
            .iter()
            .find(|p| p.tcgplayer_product_id == "648686")
            .expect("Commander's Bundle is present");
        let play = bundle
            .components
            .iter()
            .find(|c| c.child_tcgplayer_product_id.as_deref() == Some("648640"))
            .expect("links the Play Booster sub-product");
        assert_eq!(play.parsed_kind(), Some(ComponentKind::Sealed));
        assert_eq!(play.quantity, 9, "9 play boosters");
        let collector = bundle
            .components
            .iter()
            .find(|c| c.child_tcgplayer_product_id.as_deref() == Some("648646"))
            .expect("links the Collector Booster sub-product");
        assert_eq!(collector.quantity, 1, "1 collector booster");
        // The physical extras are textual `other` line items (no child link).
        assert!(
            bundle
                .components
                .iter()
                .any(|c| c.kind == "other" && c.child_tcgplayer_product_id.is_none())
        );
        // The three guaranteed staples are `card` components linking the borderless printing
        // (tle 315-317), one each; the two randomised staples are one textual `card` line
        // (quantity 2, no card link) — so "what's in the box" lists the guaranteed cards and
        // the 2-of-10 pool alongside the boosters.
        let guaranteed: Vec<(&str, i32)> = bundle
            .components
            .iter()
            .filter(|c| c.kind == "card" && c.child_set.as_deref() == Some("tle"))
            .map(|c| (c.child_number.as_deref().unwrap_or_default(), c.quantity))
            .collect();
        assert_eq!(
            guaranteed,
            vec![("315", 1), ("316", 1), ("317", 1)],
            "the three guaranteed borderless staples are linked, one each"
        );
        let pool = bundle
            .components
            .iter()
            .find(|c| c.kind == "card" && c.child_number.is_none())
            .expect("a textual card line for the randomised pool");
        assert_eq!(pool.quantity, 2, "2 random cards drawn from the pool of 10");
    }

    /// Pin the Commander's Bundle **Case** (issue #507): a 6-bundle case whose cards cascade
    /// from the Commander's Bundle via MTGJSON's sealed recursion — carrying the same wrong
    /// certainty (the random pool arriving as guaranteed), so it needs the identical override.
    /// Its curated `contents` must mirror the bundle exactly, it must be a `supplement` that
    /// `override_memberships`, and its composition links six of the bundle (tcg 648686).
    #[test]
    fn covers_avatar_commander_bundle_case() {
        let find = |tcg: &str| {
            data()
                .products
                .iter()
                .find(|p| p.tcgplayer_product_id == tcg)
                .unwrap_or_else(|| panic!("product {tcg} present"))
        };
        let bundle = find("648686");
        let case = find("648694");

        assert!(
            case.supplement && case.override_memberships,
            "the case merges despite upstream coverage and overrides the pool's certainty"
        );

        // The case's cards are exactly the bundle's cards, membership + finish for membership —
        // a card found in the bundle is found in the case, with the same pool demoted to
        // `variable`. Comparing the full sets keeps the two entries in lock-step: a future
        // edit to the bundle that isn't mirrored onto the case fails here.
        let key = |c: &FallbackCard| {
            (
                c.set.clone(),
                c.number.clone(),
                c.membership.clone(),
                c.foil,
            )
        };
        let bundle_cards: std::collections::HashSet<_> = bundle.contents.iter().map(key).collect();
        let case_cards: std::collections::HashSet<_> = case.contents.iter().map(key).collect();
        assert_eq!(
            case_cards, bundle_cards,
            "the case mirrors the bundle's contents exactly"
        );

        // The random pool is `variable` ('may be in'), the guaranteed three stay `contains`.
        let find_card = |num: &str| case.contents.iter().find(|c| c.number == num);
        assert_eq!(
            find_card("316").map(|c| c.membership.as_str()),
            Some("contains"),
            "Sol Ring (tle #316) is a guaranteed inclusion"
        );
        assert_eq!(
            find_card("311").map(|c| c.membership.as_str()),
            Some("variable"),
            "Deflecting Swat (tle #311) is a may-be-in inclusion"
        );

        // The composition is a safety net: six of the Commander's Bundle sub-product.
        assert_eq!(case.components.len(), 1);
        assert_eq!(case.components[0].quantity, 6);
        assert_eq!(
            case.components[0].child_tcgplayer_product_id.as_deref(),
            Some("648686")
        );
    }

    /// Pin the newly-authored Avatar `contents:null` products — the case/multipack link
    /// counts and children their composition renders (so a bad tcgid/count fails CI).
    #[test]
    fn covers_avatar_null_content_products() {
        let find = |tcg: &str| {
            data()
                .products
                .iter()
                .find(|p| p.tcgplayer_product_id == tcg)
                .unwrap_or_else(|| panic!("product {tcg} present"))
        };

        // The Beginner Box Case is 3x Beginner Box (tcg 648682).
        let case = find("662272");
        assert_eq!(case.components.len(), 1);
        assert_eq!(case.components[0].quantity, 3);
        assert_eq!(
            case.components[0].child_tcgplayer_product_id.as_deref(),
            Some("648682")
        );

        // The Prerelease Packs Set of 5 links one of each of the five character packs.
        let set5 = find("648724");
        let mut children: Vec<&str> = set5
            .components
            .iter()
            .filter_map(|c| c.child_tcgplayer_product_id.as_deref())
            .collect();
        children.sort();
        assert_eq!(
            children,
            vec!["648719", "648720", "648721", "648722", "648723"]
        );
        assert!(set5.components.iter().all(|c| c.quantity == 1));

        // The Scene Box Case is 2 of each Scene Box (a 4-box case).
        let scene_case = find("648718");
        assert_eq!(scene_case.components.len(), 2);
        assert!(scene_case.components.iter().all(|c| c.quantity == 2));

        // A prerelease pack lists 5 Play Boosters (tcg 648640).
        let aang = find("648719");
        let boosters = aang
            .components
            .iter()
            .find(|c| c.child_tcgplayer_product_id.as_deref() == Some("648640"))
            .expect("links the play booster");
        assert_eq!(boosters.quantity, 5);
    }

    /// Spot-check a few of the bulk-authored non-Avatar products — the multipack link
    /// structure and standard booster counts (so a typo'd child id or count fails CI).
    #[test]
    fn covers_non_avatar_products() {
        let find = |tcg: &str| {
            data()
                .products
                .iter()
                .find(|p| p.tcgplayer_product_id == tcg)
                .unwrap_or_else(|| panic!("product {tcg} present"))
        };

        // Secrets of Strixhaven Commander Deck Set of 5 -> one of each of 5 college decks.
        let soc = find("675572");
        let soc_children: std::collections::HashSet<&str> = soc
            .components
            .iter()
            .filter_map(|c| c.child_tcgplayer_product_id.as_deref())
            .collect();
        assert_eq!(soc_children.len(), 5, "links 5 sibling decks");
        assert!(soc.components.iter().all(|c| c.quantity == 1));

        // Tarkir Dragonstorm Prerelease Packs Set of 5 -> 5 clan packs.
        let tdm = find("620244");
        assert_eq!(
            tdm.components.iter().filter(|c| c.kind == "sealed").count(),
            5
        );

        // Modern Horizons 3 Prerelease Pack -> 6 Play Boosters (tcg 541163).
        let mh3 = find("541159");
        let mh3_boosters = mh3
            .components
            .iter()
            .find(|c| c.child_tcgplayer_product_id.as_deref() == Some("541163"))
            .expect("links the play booster pack");
        assert_eq!(mh3_boosters.quantity, 6);
    }

    /// Every `sealed` component carries a non-empty child link (a linkable sub-product);
    /// every non-`sealed` link field stays absent (textual). Guards the shipped file.
    #[test]
    fn sealed_components_link_and_others_are_textual() {
        for product in &data().products {
            for c in &product.components {
                match c.kind.as_str() {
                    "sealed" => {
                        // Most sealed components link a sub-product; a cross-set booster (a
                        // deluxe-kit's from-another-set pack) legitimately can't, and stays
                        // textual — so this only asserts a *present* link is non-empty.
                        if let Some(id) = &c.child_tcgplayer_product_id {
                            assert!(!id.trim().is_empty(), "{} sealed link non-empty", c.name);
                        }
                    }
                    _ => assert!(
                        c.child_tcgplayer_product_id.is_none(),
                        "non-sealed component {} carries no product link",
                        c.name
                    ),
                }
            }
        }
    }

    /// The version hash is non-empty and stable across calls (drives the ingest gate).
    #[test]
    fn version_is_stable() {
        assert!(!version().is_empty());
        assert_eq!(version(), version());
    }
}
