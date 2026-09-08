//! **Booster odds**: the weighted print sheets a pack draws from, the slot configurations
//! it rolls, and which of them each sealed product opens — the data MTGJSON states and the
//! sealed-contents walk discards once it has the flat *set* of cards a pack can yield
//! (issue #682).
//!
//! This is the *pure* half (no DB, no network), like [`super::precons`]: given a parsed
//! `AllPrintings` and the shared [`Indexes`], resolve
//!
//! * [`packs_from`] — every `(product, set, booster code)` link with the number of packs
//!   **one copy** of the product opens, flattening nested box → pack references, and
//! * [`configs_from`] — the configurations those links name, with their variants and
//!   sheets, cards resolved from MTGJSON `uuid`s to Scryfall ids.
//!
//! [`super::ingest::boosters`] maps those onto our catalog and rebuilds the
//! `booster_configs` / `booster_sheets` / `sealed_packs` tables. **No new fetch**: both
//! passes borrow the one `AllPrintings.json` the sealed sync already streams and the one
//! `Indexes` the membership, composition and precon passes share.
//!
//! Three decisions live here because the read cannot make them:
//!
//! * **Only referenced configurations are built.** The read is product-keyed, so a
//!   configuration no catalog product opens is of no use to it — [`configs_from`] takes the
//!   `(set, code)` set [`packs_from`] produced and builds exactly those.
//! * **A `variable` pack is not a pack we can price.** `contents.variable` is upstream's
//!   "one of these, at random", so a product whose boosters are a randomised choice gets no
//!   link at all and its expected value stays `null` — better than an average over a
//!   configuration the buyer may not receive.
//! * **A card our catalog doesn't hold is dropped, but its weight is not.** A sheet's
//!   `total_weight` counts every parsed entry, so `total_weight - Σ stored weights` is the
//!   share the read can't price — which it reports, rather than silently re-normalising
//!   over what happens to be present.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::model::{AllPrintings, BoosterConfig, Contents, Indexes, MAX_SEALED_DEPTH};

/// One resolved "one copy of this product opens `quantity` of that booster" link, keyed by
/// **external** ids (TCGplayer product id) so the DB layer resolves them — the same shape
/// [`RawMembership`](super::model::RawMembership) travels in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPack {
    pub tcgplayer_product_id: String,
    /// Lowercased set code the booster belongs to, matching `cards.set_code`.
    pub set_code: String,
    /// MTGJSON's booster key (`play`, `collector`, `draft`, …).
    pub booster_code: String,
    /// How many of that booster one copy of the product opens (`>= 1`).
    pub quantity: u32,
}

/// One resolved booster configuration: its variants and its sheets, cards keyed by
/// Scryfall id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBoosterConfig {
    /// Lowercased set code.
    pub set_code: String,
    /// MTGJSON's booster key — with `set_code`, the configuration's identity.
    pub code: String,
    /// Upstream's display name (`Play Booster`), when it states one.
    pub name: Option<String>,
    /// The pack variants, in upstream order, weight-0 ones dropped.
    pub variants: Vec<RawVariant>,
    /// The configuration's sheets, sorted by name (upstream states a JSON object).
    pub sheets: Vec<RawSheet>,
}

/// One pack variant: with probability `weight / Σ weights` a pack draws these slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawVariant {
    pub weight: u64,
    /// `(sheet name, cards drawn from it)`, sorted by sheet name — upstream states them as
    /// a JSON object, so there is no order to preserve and sorting makes the stored column
    /// deterministic across rebuilds. A slot naming a sheet the configuration doesn't
    /// define is **kept**: the read tolerates it (it draws nothing), and dropping it here
    /// would silently shrink the pack.
    pub slots: Vec<(String, u32)>,
}

/// One print sheet: its flags, its denominator, and its cards by Scryfall id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSheet {
    pub name: String,
    pub foil: bool,
    pub balance_colors: bool,
    pub allow_duplicates: bool,
    pub fixed: bool,
    /// Upstream's `totalWeight` when stated, else Σ of every parsed weight — computed
    /// **before** dropping cards that didn't resolve (see the module note).
    pub total_weight: u64,
    /// `(scryfall id, weight)` in **upstream order**: a `fixed` sheet's slot takes its
    /// first `count` cards, so the order is load-bearing, not cosmetic — which is why a
    /// card that didn't resolve is kept there as an **empty** scryfall id (a position we
    /// can't name) rather than compacted out, shifting every card after it. On any other
    /// sheet, drawn by weight where position means nothing, it is dropped.
    pub cards: Vec<(String, u32)>,
}

/// Resolve every sealed product's booster links over a prebuilt index.
///
/// A product's own `contents.pack` references count once each; a `contents.sealed`
/// reference recurses into the sub-product with the running quantity multiplied by its
/// `count` (absent reads as one), so a box of 36 packs yields 36 and a case of six such
/// boxes 216. A reference naming a configuration the set doesn't define is dropped —
/// there would be nothing to open.
///
/// The cycle guard is a **path stack** (entered on the way down, released on the way back
/// up), not the membership walk's branch-global `visited` set: two sibling references to
/// the same pack are two real packs and must both count, where for a *membership* the
/// second adds nothing.
pub(super) fn packs_from(all: &AllPrintings, idx: &Indexes) -> Vec<RawPack> {
    // `(tcgplayer id, set, code) -> quantity`, in a BTreeMap so the output order is
    // deterministic (the document keys its sets in a HashMap).
    let mut folded: BTreeMap<(String, String, String), u32> = BTreeMap::new();
    for data in all.data.values() {
        for product in &data.sealed_product {
            let Some(tcg_id) = product.identifiers.tcgplayer_product_id.as_deref() else {
                continue;
            };
            let Some(contents) = &product.contents else {
                continue;
            };
            let mut counts: HashMap<(String, String), u32> = HashMap::new();
            // The product's own uuid seeds the path stack: nothing contains itself, so a
            // reference back to the root is a cycle like any other.
            let mut visiting: HashSet<String> = product.uuid.iter().cloned().collect();
            walk_packs(idx, contents, 1, 0, &mut visiting, &mut counts);
            for (key, quantity) in counts {
                // Two `sealedProduct` entries can share one TCGplayer id (the composition
                // walk guards the same case): they describe the *same* physical product
                // twice, so take the larger count rather than summing — a doubled box would
                // read as 72 packs. Within one entry the walk has already summed.
                let slot = folded
                    .entry((tcg_id.to_string(), key.0, key.1))
                    .or_insert(0);
                *slot = (*slot).max(quantity);
            }
        }
    }
    folded
        .into_iter()
        .map(
            |((tcgplayer_product_id, set_code, booster_code), quantity)| RawPack {
                tcgplayer_product_id,
                set_code,
                booster_code,
                quantity,
            },
        )
        .collect()
}

/// Accumulate a product's booster references, `multiplier` copies of whatever this level
/// holds. `visiting` is the path stack of sealed-product uuids currently being expanded.
fn walk_packs(
    idx: &Indexes,
    contents: &Contents,
    multiplier: u32,
    depth: usize,
    visiting: &mut HashSet<String>,
    counts: &mut HashMap<(String, String), u32>,
) {
    for pr in &contents.pack {
        let (Some(set), Some(code)) = (&pr.set, &pr.code) else {
            continue;
        };
        let set_code = set.to_lowercase();
        // A reference to a configuration the set doesn't define opens nothing.
        let defined = idx
            .set(&set_code)
            .is_some_and(|data| data.booster.contains_key(code.as_str()));
        if !defined {
            continue;
        }
        let slot = counts.entry((set_code, code.clone())).or_insert(0);
        *slot = slot.saturating_add(multiplier);
    }
    // `contents.variable` is deliberately not walked: a randomised choice of packs has no
    // defined expected value (see the module note).
    if depth >= MAX_SEALED_DEPTH {
        return;
    }
    for sr in &contents.sealed {
        let Some(uuid) = sr.uuid.as_deref() else {
            continue;
        };
        if !visiting.insert(uuid.to_string()) {
            continue; // already on this path — a cycle
        }
        if let Some(sub) = idx.product(uuid)
            && let Some(sub_contents) = &sub.contents
        {
            // A missing / zero / negative count reads as one, as everywhere else.
            let count = u32::try_from(sr.count.unwrap_or(1)).unwrap_or(1).max(1);
            walk_packs(
                idx,
                sub_contents,
                multiplier.saturating_mul(count),
                depth + 1,
                visiting,
                counts,
            );
        }
        visiting.remove(uuid);
    }
}

/// Resolve the booster configurations named by `referenced` (the `(set code, booster code)`
/// pairs [`packs_from`] produced) over a prebuilt index.
///
/// Sets are walked in sorted key order and each set's configurations in sorted code order,
/// so the same document always produces the same rows in the same order. A referenced pair
/// the document doesn't define simply yields nothing.
pub(super) fn configs_from(
    all: &AllPrintings,
    idx: &Indexes,
    referenced: &HashSet<(String, String)>,
) -> Vec<RawBoosterConfig> {
    let mut set_keys: Vec<&String> = all.data.keys().collect();
    set_keys.sort_unstable();

    let mut out: Vec<RawBoosterConfig> = Vec::new();
    for set_key in set_keys {
        let Some(data) = all.data.get(set_key) else {
            continue;
        };
        let set_code = set_key.to_lowercase();
        let mut codes: Vec<&String> = data.booster.keys().collect();
        codes.sort_unstable();
        for code in codes {
            if !referenced.contains(&(set_code.clone(), code.clone())) {
                continue;
            }
            let Some(config) = data.booster.get(code) else {
                continue;
            };
            out.push(build_config(&set_code, code, config, idx));
        }
    }
    out
}

/// Resolve one configuration: its variants (weight-0 ones dropped, slots sorted by sheet
/// name) and its sheets (sorted by name, cards resolved `uuid` -> Scryfall id; an
/// unresolvable card is dropped, or kept as an empty-id placeholder on a `fixed` sheet —
/// see [`RawSheet::cards`]).
fn build_config(
    set_code: &str,
    code: &str,
    config: &BoosterConfig,
    idx: &Indexes,
) -> RawBoosterConfig {
    let variants: Vec<RawVariant> = config
        .boosters
        .iter()
        .filter(|variant| variant.weight > 0)
        .map(|variant| {
            let mut slots: Vec<(String, u32)> = variant
                .contents
                .iter()
                .map(|(sheet, count)| (sheet.to_string(), *count))
                .collect();
            slots.sort_by(|a, b| a.0.cmp(&b.0));
            RawVariant {
                weight: variant.weight,
                slots,
            }
        })
        .collect();

    let mut sheet_names: Vec<&String> = config.sheets.keys().collect();
    sheet_names.sort_unstable();
    let sheets: Vec<RawSheet> = sheet_names
        .into_iter()
        .filter_map(|name| {
            let sheet = config.sheets.get(name)?;
            // Summed before the resolution filter below, so the stated total keeps
            // covering the cards our catalog doesn't hold.
            let parsed_total: u64 = sheet.cards.iter().map(|&(_, w)| u64::from(w)).sum();
            let cards: Vec<(String, u32)> = sheet
                .cards
                .iter()
                .filter_map(|(uuid, weight)| match idx.scryfall_by_uuid(uuid) {
                    Some(scryfall) => Some((scryfall.to_string(), *weight)),
                    // A fixed sheet is read by *position*, so a card we can't name has to
                    // keep its place — an empty id, which resolves to nothing downstream
                    // and is stored as `booster_sheet::UNRESOLVED_CARD_ID`. Dropping it
                    // would shift every later card and hand a bundle's land pack the wrong
                    // printings.
                    None if sheet.fixed => Some((String::new(), *weight)),
                    None => None,
                })
                .collect();
            Some(RawSheet {
                name: name.clone(),
                foil: sheet.foil,
                balance_colors: sheet.balance_colors,
                allow_duplicates: sheet.allow_duplicates,
                fixed: sheet.fixed,
                total_weight: sheet.total_weight.unwrap_or(parsed_total),
                cards,
            })
        })
        .collect();

    RawBoosterConfig {
        set_code: set_code.to_string(),
        code: code.to_string(),
        name: config.name.clone(),
        variants,
        sheets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic `AllPrintings` covering every shape the two passes have to handle: a
    /// direct pack, a box, a case of boxes, two sibling references to one pack, a cycle, a
    /// `variable` product, a reference to an undefined configuration, and a product with no
    /// TCGplayer id.
    ///
    /// Parsed from a **string** rather than `serde_json::json!`: sheet cards and slot
    /// counts are read as ordered entry lists and a `fixed` sheet's order is load-bearing,
    /// while `json!` would re-key both through a sorted map.
    fn fixture() -> AllPrintings {
        serde_json::from_str(
            r#"{
              "data": {
                "SET": {
                  "cards": [
                    { "uuid": "u-a", "number": "1", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-a" } },
                    { "uuid": "u-b", "number": "2", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-b" } },
                    { "uuid": "u-rare", "number": "3", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-rare" } },
                    { "uuid": "u-mythic", "number": "4", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-mythic" } },
                    { "uuid": "u-land", "number": "5", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-land" } },
                    { "uuid": "u-land2", "number": "6", "setCode": "SET",
                      "identifiers": { "scryfallId": "sf-land2" } }
                  ],
                  "booster": {
                    "play": {
                      "name": "Play Booster",
                      "boosters": [
                        { "contents": { "rareMythic": 1, "common": 6, "land": 1 }, "weight": 3 },
                        { "contents": { "rareMythic": 1, "common": 5, "list": 1 }, "weight": 1 },
                        { "contents": { "common": 99 }, "weight": 0 }
                      ],
                      "boostersTotalWeight": 4,
                      "sheets": {
                        "common": { "balanceColors": true, "totalWeight": 12,
                                    "cards": { "u-a": 1, "u-ghost": 8, "u-b": 3 } },
                        "rareMythic": { "cards": { "u-rare": 2, "u-mythic": 1 } },
                        "land": { "fixed": true, "totalWeight": 2,
                                  "cards": { "u-land2": 1, "u-land": 1 } },
                        "wildcard": { "fixed": true, "totalWeight": 3,
                                      "cards": { "u-land2": 1, "u-ghost-wild": 1,
                                                 "u-land": 1 } }
                      }
                    },
                    "collector": {
                      "name": "Collector Booster",
                      "boosters": [ { "contents": { "foilRare": 1 }, "weight": 1 } ],
                      "sheets": {
                        "foilRare": { "foil": true, "allowDuplicates": true,
                                      "totalWeight": 3, "cards": { "u-rare": 3 } }
                      }
                    }
                  },
                  "sealedProduct": [
                    { "uuid": "p-pack", "identifiers": { "tcgplayerProductId": "1001" },
                      "contents": { "pack": [ { "code": "play", "set": "set" } ] } },
                    { "uuid": "p-box", "identifiers": { "tcgplayerProductId": "1002" },
                      "contents": { "sealed": [
                        { "uuid": "p-pack", "count": 36, "name": "Play Booster Pack" } ] } },
                    { "uuid": "p-case", "identifiers": { "tcgplayerProductId": "1003" },
                      "contents": { "sealed": [
                        { "uuid": "p-box", "count": 6, "name": "Play Booster Box" } ] } },
                    { "uuid": "p-bundle", "identifiers": { "tcgplayerProductId": "1004" },
                      "contents": { "sealed": [
                        { "uuid": "p-pack", "count": 1, "name": "Play Booster Pack" },
                        { "uuid": "p-pack", "count": 2, "name": "Play Booster Pack" } ] } },
                    { "uuid": "p-loop", "identifiers": { "tcgplayerProductId": "1005" },
                      "contents": { "sealed": [ { "uuid": "p-loop-b" } ] } },
                    { "uuid": "p-loop-b", "identifiers": { "tcgplayerProductId": "1006" },
                      "contents": {
                        "pack": [ { "code": "play", "set": "set" } ],
                        "sealed": [ { "uuid": "p-loop" } ] } },
                    { "uuid": "p-sld", "identifiers": { "tcgplayerProductId": "1007" },
                      "contents": { "variable": [ { "configs": [
                        { "pack": [ { "code": "play", "set": "set" } ] } ] } ] } },
                    { "uuid": "p-unknown", "identifiers": { "tcgplayerProductId": "1008" },
                      "contents": { "pack": [ { "code": "jumpstart", "set": "set" } ] } },
                    { "uuid": "p-collector", "identifiers": { "tcgplayerProductId": "1009" },
                      "contents": { "pack": [ { "code": "collector", "set": "SET" } ] } },
                    { "uuid": "p-noid",
                      "contents": { "pack": [ { "code": "play", "set": "set" } ] } }
                  ]
                }
              }
            }"#,
        )
        .expect("fixture parses")
    }

    fn packs() -> Vec<RawPack> {
        let all = fixture();
        let idx = Indexes::build(&all);
        packs_from(&all, &idx)
    }

    fn quantity(packs: &[RawPack], product: &str, code: &str) -> Option<u32> {
        packs
            .iter()
            .find(|p| p.tcgplayer_product_id == product && p.booster_code == code)
            .map(|p| p.quantity)
    }

    /// A product's own `contents.pack` reference is one pack, with the set code lowercased
    /// however upstream spelled it.
    #[test]
    fn a_direct_pack_reference_is_one_pack() {
        let packs = packs();
        assert_eq!(quantity(&packs, "1001", "play"), Some(1));
        let collector = packs
            .iter()
            .find(|p| p.tcgplayer_product_id == "1009")
            .expect("the collector pack product");
        assert_eq!(collector.set_code, "set", "the set code is lowercased");
        assert_eq!(collector.quantity, 1);
    }

    /// `contents.sealed` multiplies through: a box of 36 packs opens 36, and a case of six
    /// such boxes 216 — the number a page has to be able to state.
    #[test]
    fn nested_sealed_references_multiply_by_count() {
        let packs = packs();
        assert_eq!(quantity(&packs, "1002", "play"), Some(36));
        assert_eq!(quantity(&packs, "1003", "play"), Some(216));
    }

    /// Two sibling references to the *same* pack are two real packs, so they sum — which is
    /// exactly what the membership walk's branch-global `visited` set would have prevented.
    #[test]
    fn sibling_references_to_one_pack_sum() {
        assert_eq!(quantity(&packs(), "1004", "play"), Some(3));
    }

    /// A reference cycle terminates and still counts what it legitimately reached once.
    #[test]
    fn a_cycle_terminates_and_counts_once() {
        let packs = packs();
        assert_eq!(quantity(&packs, "1005", "play"), Some(1));
        assert_eq!(quantity(&packs, "1006", "play"), Some(1));
    }

    /// A `variable` ("one of these, at random") product gets no link at all — its expected
    /// value stays `null` rather than averaging a configuration the buyer may not receive.
    #[test]
    fn variable_packs_are_skipped() {
        assert!(packs().iter().all(|p| p.tcgplayer_product_id != "1007"));
    }

    /// A reference to a configuration the set doesn't define opens nothing, and a product
    /// with no TCGplayer id can't be joined to our catalog at all.
    #[test]
    fn undefined_configurations_and_id_less_products_are_dropped() {
        let packs = packs();
        assert!(packs.iter().all(|p| p.tcgplayer_product_id != "1008"));
        let products: HashSet<&str> = packs
            .iter()
            .map(|p| p.tcgplayer_product_id.as_str())
            .collect();
        assert_eq!(
            products,
            HashSet::from(["1001", "1002", "1003", "1004", "1005", "1006", "1009"])
        );
    }

    /// The output order is deterministic (the document keys sets and products in hash maps,
    /// so it must not be iteration order).
    #[test]
    fn packs_are_emitted_in_a_deterministic_order() {
        let keys: Vec<(String, String, String)> = packs()
            .into_iter()
            .map(|p| (p.tcgplayer_product_id, p.set_code, p.booster_code))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(keys, {
            let again: Vec<(String, String, String)> = packs()
                .into_iter()
                .map(|p| (p.tcgplayer_product_id, p.set_code, p.booster_code))
                .collect();
            again
        });
    }

    fn configs_for(codes: &[&str]) -> Vec<RawBoosterConfig> {
        let all = fixture();
        let idx = Indexes::build(&all);
        let referenced: HashSet<(String, String)> = codes
            .iter()
            .map(|code| ("set".to_string(), (*code).to_string()))
            .collect();
        configs_from(&all, &idx, &referenced)
    }

    /// Only the configurations some product opens are built — the read is product-keyed, so
    /// a configuration nothing references would be dead weight in the table.
    #[test]
    fn only_referenced_configurations_are_built() {
        let built: Vec<String> = configs_for(&["play"]).into_iter().map(|c| c.code).collect();
        assert_eq!(built, vec!["play".to_string()]);
        // The set derived from the pack walk names both, and both are built.
        let all = fixture();
        let idx = Indexes::build(&all);
        let referenced: HashSet<(String, String)> = packs_from(&all, &idx)
            .into_iter()
            .map(|p| (p.set_code, p.booster_code))
            .collect();
        let mut codes: Vec<String> = configs_from(&all, &idx, &referenced)
            .into_iter()
            .map(|c| c.code)
            .collect();
        codes.sort();
        assert_eq!(codes, vec!["collector".to_string(), "play".to_string()]);
        // A referenced pair the document doesn't define yields nothing.
        assert!(configs_for(&["jumpstart"]).is_empty());
    }

    /// Variants keep upstream's order and weights, a weight-0 variant is dropped (it can
    /// never be rolled), and each variant's slots are sorted by sheet name so the stored
    /// column is stable across rebuilds. A slot naming a sheet the configuration doesn't
    /// define (`list`) is kept — the read tolerates it, dropping it would shrink the pack.
    #[test]
    fn variants_keep_their_weights_and_sort_their_slots() {
        let configs = configs_for(&["play"]);
        let play = configs.first().expect("the play configuration");
        assert_eq!(play.set_code, "set");
        assert_eq!(play.name.as_deref(), Some("Play Booster"));
        assert_eq!(play.variants.len(), 2, "the weight-0 variant is dropped");
        assert_eq!(play.variants[0].weight, 3);
        assert_eq!(
            play.variants[0].slots,
            vec![
                ("common".to_string(), 6),
                ("land".to_string(), 1),
                ("rareMythic".to_string(), 1),
            ]
        );
        assert_eq!(play.variants[1].weight, 1);
        assert_eq!(
            play.variants[1].slots,
            vec![
                ("common".to_string(), 5),
                ("list".to_string(), 1),
                ("rareMythic".to_string(), 1),
            ]
        );
    }

    /// Sheets carry their flags, keep upstream's card order (a `fixed` slot takes its first
    /// `count` cards, so the order is load-bearing), and are themselves sorted by name.
    #[test]
    fn sheets_keep_their_flags_and_upstream_card_order() {
        let configs = configs_for(&["play", "collector"]);
        let play = configs
            .iter()
            .find(|c| c.code == "play")
            .expect("the play configuration");
        let names: Vec<&str> = play.sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["common", "land", "rareMythic", "wildcard"]);

        let land = &play.sheets[1];
        assert!(land.fixed && !land.foil && !land.allow_duplicates);
        assert_eq!(
            land.cards,
            vec![("sf-land2".to_string(), 1), ("sf-land".to_string(), 1)],
            "a fixed sheet keeps upstream's order, not a sorted one"
        );
        assert!(play.sheets[0].balance_colors, "the common sheet balances");

        let collector = configs
            .iter()
            .find(|c| c.code == "collector")
            .expect("the collector configuration");
        let foil = &collector.sheets[0];
        assert!(foil.foil && foil.allow_duplicates);
        assert_eq!(foil.cards, vec![("sf-rare".to_string(), 3)]);
    }

    /// A card our catalog can't name is dropped from the sheet, but its weight stays in
    /// `total_weight`: the difference is exactly the share the read can't price, and
    /// re-normalising over the survivors would quietly overstate every remaining card.
    #[test]
    fn an_unresolved_card_is_dropped_but_keeps_its_weight_in_the_total() {
        let configs = configs_for(&["play"]);
        let common = &configs[0].sheets[0];
        assert_eq!(common.name, "common");
        assert_eq!(
            common.cards,
            vec![("sf-a".to_string(), 1), ("sf-b".to_string(), 3)],
            "u-ghost names no card in the document"
        );
        assert_eq!(common.total_weight, 12, "upstream's total is kept verbatim");
        let stored: u64 = common.cards.iter().map(|&(_, w)| u64::from(w)).sum();
        assert_eq!(common.total_weight - stored, 8, "the unaccounted share");
    }

    /// A **fixed** sheet is read by *position*, so a card the document can't name keeps its
    /// place as an empty scryfall id rather than letting every card after it slide up one.
    /// Compacting it out is what would hand a slot taking two cards the third printing.
    #[test]
    fn a_fixed_sheets_unresolved_card_holds_its_position() {
        let configs = configs_for(&["play"]);
        let wildcard = &configs[0].sheets[3];
        assert_eq!(wildcard.name, "wildcard");
        assert!(wildcard.fixed);
        assert_eq!(
            wildcard.cards,
            vec![
                ("sf-land2".to_string(), 1),
                (String::new(), 1),
                ("sf-land".to_string(), 1),
            ],
            "`u-ghost-wild` names no card, but `sf-land` must stay third"
        );
        assert_eq!(
            wildcard.total_weight, 3,
            "the placeholder's weight is still unaccounted for, as a dropped card's is"
        );

        // The weighted sheet in the same configuration still compacts: there is no position
        // to protect when the sheet is drawn by weight.
        assert_eq!(
            configs[0].sheets[0].cards,
            vec![("sf-a".to_string(), 1), ("sf-b".to_string(), 3)]
        );
    }

    /// A sheet upstream states no `totalWeight` for sums its parsed weights instead — the
    /// same denominator, computed before the resolution filter.
    #[test]
    fn a_sheet_without_a_stated_total_sums_its_weights() {
        let configs = configs_for(&["play"]);
        let rare_mythic = &configs[0].sheets[2];
        assert_eq!(rare_mythic.name, "rareMythic");
        assert_eq!(rare_mythic.total_weight, 3);
        assert_eq!(
            rare_mythic.cards,
            vec![("sf-rare".to_string(), 2), ("sf-mythic".to_string(), 1)]
        );
    }
}
