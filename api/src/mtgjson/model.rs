//! Trimmed serde shapes for MTGJSON's `AllPrintings.json`, plus the **pure** resolution
//! of a sealed product's contents into per-card membership rows.
//!
//! Only the handful of fields the sealed-contents feature needs are deserialized (serde
//! ignores the rest), so parsing the ~600 MB document retains only ~a few hundred MB of
//! structs rather than the whole tree. The shapes were verified against a live
//! `AllPrintings.json` build; see [`super`] for the field-by-field mapping notes.
//!
//! [`build_memberships`] is the testable heart of the ingest: given a parsed
//! `AllPrintings`, it walks every set's `sealedProduct[]` and resolves each product's
//! `contents` down to individual cards, tagged with the membership bucket
//! (`contains` / `booster` / `variable`). It is a pure function — no DB, no network — so
//! the mapping is unit-tested against a synthetic fixture.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::entities::sealed_component::ComponentKind;
use crate::entities::sealed_content::Membership;

// ---------- Serde shapes (trimmed to what we consume) ----------

/// The top-level `AllPrintings.json`: `{ "meta": …, "data": { "<SET>": {…} } }`. Set
/// codes are keyed **uppercase** here; content references use lowercase, so all
/// resolution lowercases before matching.
#[derive(Debug, Deserialize)]
pub struct AllPrintings {
    #[serde(default)]
    pub data: HashMap<String, SetData>,
}

/// One set's data. We keep its cards (for the `uuid`/`(set,number)` -> Scryfall bridge),
/// its sealed products, booster configs, and precon decks.
#[derive(Debug, Default, Deserialize)]
pub struct SetData {
    #[serde(default)]
    pub cards: Vec<CardEntry>,
    #[serde(default, rename = "sealedProduct")]
    pub sealed_product: Vec<SealedProduct>,
    #[serde(default)]
    pub booster: HashMap<String, BoosterConfig>,
    #[serde(default)]
    pub decks: Vec<Deck>,
}

/// A card row — its MTGJSON `uuid` plus the Scryfall id and `(setCode, number)` used to
/// resolve booster-sheet / deck references (which reference cards by `uuid` only) onto
/// our catalog.
#[derive(Debug, Deserialize)]
pub struct CardEntry {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub identifiers: CardIdentifiers,
    #[serde(default, rename = "setCode")]
    pub set_code: Option<String>,
    #[serde(default)]
    pub number: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CardIdentifiers {
    #[serde(default, rename = "scryfallId")]
    pub scryfall_id: Option<String>,
}

/// A sealed product with its TCGplayer id (the join onto our `products` table) and its
/// `contents` breakdown. `uuid` lets a parent product's `sealed` reference find it.
#[derive(Debug, Deserialize)]
pub struct SealedProduct {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub identifiers: SealedIdentifiers,
    #[serde(default)]
    pub contents: Option<Contents>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SealedIdentifiers {
    #[serde(default, rename = "tcgplayerProductId")]
    pub tcgplayer_product_id: Option<String>,
}

/// The six content categories. Card resolution ([`build_memberships`]) consumes five and
/// ignores `other` (non-card); the composition builder ([`build_compositions`]) consumes
/// `sealed` / `deck` / `card` / `other` (the physical "what's in the box" line items).
#[derive(Debug, Default, Deserialize)]
pub struct Contents {
    #[serde(default)]
    pub card: Vec<ContentCard>,
    #[serde(default)]
    pub deck: Vec<ContentDeck>,
    #[serde(default)]
    pub pack: Vec<ContentPack>,
    #[serde(default)]
    pub sealed: Vec<ContentSealed>,
    #[serde(default)]
    pub variable: Vec<ContentVariable>,
    /// Non-card physical extras — a spindown die, a storage box, a basic-land pack, …
    /// (`{ "name": … }`). Ignored by card resolution; surfaced by the composition builder.
    #[serde(default)]
    pub other: Vec<ContentOther>,
}

/// A directly-named card (usually a fixed promo): resolvable by `uuid` or `(set, number)`.
#[derive(Debug, Deserialize)]
pub struct ContentCard {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub foil: bool,
    /// Display name (composition line item); resolution ignores it.
    #[serde(default)]
    pub name: Option<String>,
}

/// A precon-deck reference, resolved against the set's `decks[]` by `name`.
#[derive(Debug, Deserialize)]
pub struct ContentDeck {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
}

/// A booster-pack reference, resolved against `set.booster[code].sheets`.
#[derive(Debug, Deserialize)]
pub struct ContentPack {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
}

/// A nested sealed product (e.g. a box of packs), resolved by `uuid` and recursed into.
/// `count` + `name` drive the composition line item ("9× Play Booster Pack"); `uuid` is
/// both the recursion key (card resolution) and the child-product link (composition).
#[derive(Debug, Deserialize)]
pub struct ContentSealed {
    #[serde(default)]
    pub uuid: Option<String>,
    /// How many of the sub-product the parent bundles (composition quantity). Absent = 1.
    #[serde(default)]
    pub count: Option<i32>,
    /// Display name of the sub-product (composition fallback when the link doesn't resolve).
    #[serde(default)]
    pub name: Option<String>,
}

/// A non-card physical extra (`{ "name": "…Spindown" }`). Surfaced verbatim as a
/// composition line item — never resolved to a card or product.
#[derive(Debug, Deserialize)]
pub struct ContentOther {
    #[serde(default)]
    pub name: Option<String>,
}

/// A randomized ("one of these") configuration; every option is surfaced as `may be in`.
#[derive(Debug, Deserialize)]
pub struct ContentVariable {
    #[serde(default)]
    pub configs: Vec<VariableConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub struct VariableConfig {
    #[serde(default)]
    pub card: Vec<ContentCard>,
    #[serde(default)]
    pub deck: Vec<ContentDeck>,
    #[serde(default)]
    pub pack: Vec<ContentPack>,
}

/// A booster configuration: the named sheets a pack draws from (`cards` is
/// `uuid -> weight`; we only need the uuids). `foil` is per-sheet.
#[derive(Debug, Deserialize)]
pub struct BoosterConfig {
    #[serde(default)]
    pub sheets: HashMap<String, Sheet>,
}

#[derive(Debug, Deserialize)]
pub struct Sheet {
    #[serde(default)]
    pub foil: bool,
    /// `uuid -> weight`; we keep only the keys (membership, not odds), so the weight is
    /// deserialized straight into `IgnoredAny` (zero-sized) rather than retained.
    #[serde(default)]
    pub cards: HashMap<String, serde::de::IgnoredAny>,
}

/// A precon decklist: its main board + commander cards (each by `uuid`, with a foil flag).
#[derive(Debug, Deserialize)]
pub struct Deck {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "mainBoard")]
    pub main_board: Vec<DeckCard>,
    #[serde(default)]
    pub commander: Vec<DeckCard>,
}

#[derive(Debug, Deserialize)]
pub struct DeckCard {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default, rename = "isFoil")]
    pub is_foil: bool,
}

// ---------- Pure resolution: contents -> per-card membership rows ----------

/// One resolved membership: a sealed product (by TCGplayer product id) contains / can
/// yield / may yield a card (by Scryfall id), in a given finish. External-id-keyed so
/// the DB layer resolves both sides to internal ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawMembership {
    pub tcgplayer_product_id: String,
    pub scryfall_id: String,
    pub membership: &'static str,
    pub foil: bool,
}

/// Guards a runaway `sealed` recursion (a box of boxes of …). Real chains are 2–3 deep.
const MAX_SEALED_DEPTH: usize = 8;

/// One resolved composition line item: a sealed product (by TCGplayer product id) holds
/// `quantity` of a component. A `sealed` component may link to a sub-product
/// (`child_tcgplayer_product_id`); a `card` component may link to a card
/// (`child_scryfall_id`); `deck` / `other` are textual. External-id-keyed so the DB layer
/// resolves the links to internal ids. Emitted in **display order** per product (the Vec's
/// order); the ingest assigns the stored `position` from the resolved internal product id,
/// so it stays collision-free even if two `sealedProduct` entries share a TCGplayer id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawComponent {
    pub tcgplayer_product_id: String,
    /// One of [`ComponentKind`]'s string values.
    pub kind: &'static str,
    pub name: String,
    pub quantity: i32,
    pub child_tcgplayer_product_id: Option<String>,
    pub child_scryfall_id: Option<String>,
}

/// Resolve every sealed product's contents into deduplicated per-card membership rows.
///
/// - `contents.card` and precon `contents.deck` cards -> `contains`.
/// - `contents.pack` booster-sheet cards -> `booster` (a probabilistic pull).
/// - `contents.variable` options (whatever their inner type) -> `variable` (may be in).
/// - `contents.sealed` -> recurse into the referenced sub-product, attributing its
///   leaves to the *outer* product (so a booster box's packs count as the box's cards).
///
/// Cards are resolved to a Scryfall id via a `uuid -> scryfallId` map (booster/deck
/// refs, which name cards by `uuid`) or a `(set, number) -> scryfallId` map (direct card
/// refs that carry only set + collector number). A product with no
/// `tcgplayerProductId`, or a card that resolves to no Scryfall id, is skipped.
pub fn build_memberships(all: &AllPrintings) -> Vec<RawMembership> {
    memberships_from(all, &Indexes::build(all))
}

/// Resolve every sealed product's **structural composition** into ordered component rows —
/// the "what's in the box" line items ([`RawComponent`]). Unlike [`build_memberships`]
/// (which recurses the whole tree down to cards), this lists each product's *direct*
/// components in a stable display order: nested packs/boxes (`sealed`) first, then precon
/// decks, then fixed promo cards, then physical extras (`other`) — `contents.pack` and
/// `contents.variable` are left to card resolution.
///
/// A `sealed` component's `uuid` resolves to the sub-product's `tcgplayerProductId` (the
/// link target); a `card` component resolves to a Scryfall id the same way card resolution
/// does. A product with no `tcgplayerProductId` or no `contents` contributes nothing; an
/// unresolved link is kept as text (the line item still carries name + quantity).
pub fn build_compositions(all: &AllPrintings) -> Vec<RawComponent> {
    compositions_from(all, &Indexes::build(all))
}

/// The membership walk over a prebuilt [`Indexes`] (see [`build_memberships`]).
fn memberships_from(all: &AllPrintings, idx: &Indexes) -> Vec<RawMembership> {
    let resolver = Resolver { idx };
    let mut out: HashSet<RawMembership> = HashSet::new();
    for data in all.data.values() {
        for product in &data.sealed_product {
            let Some(tcg_id) = product.identifiers.tcgplayer_product_id.as_deref() else {
                continue;
            };
            if let Some(contents) = &product.contents {
                let mut visited: HashSet<String> = HashSet::new();
                resolver.walk_inner(tcg_id, contents, None, &mut visited, 0, &mut out);
            }
        }
    }
    out.into_iter().collect()
}

/// The composition walk over a prebuilt [`Indexes`] (see [`build_compositions`]).
fn compositions_from(all: &AllPrintings, idx: &Indexes) -> Vec<RawComponent> {
    let mut out: Vec<RawComponent> = Vec::new();
    for data in all.data.values() {
        for product in &data.sealed_product {
            let Some(tcg_id) = product.identifiers.tcgplayer_product_id.as_deref() else {
                continue;
            };
            let Some(contents) = &product.contents else {
                continue;
            };
            // Nested packs/boxes: the headline items, linked to the sub-product by uuid.
            for sr in &contents.sealed {
                let child = sr.uuid.as_deref().and_then(|uuid| {
                    idx.product_by_uuid
                        .get(uuid)
                        .and_then(|p| p.identifiers.tcgplayer_product_id.clone())
                });
                let name = sr.name.clone().unwrap_or_default();
                push_component(
                    &mut out,
                    tcg_id,
                    ComponentKind::Sealed,
                    name,
                    sr.count.unwrap_or(1),
                    child,
                    None,
                );
            }
            // Preconstructed decks (textual — the deck's cards are in `sealed_contents`).
            for dr in &contents.deck {
                if let Some(name) = dr.name.clone() {
                    push_component(&mut out, tcg_id, ComponentKind::Deck, name, 1, None, None);
                }
            }
            // Fixed promo cards, linked to the card when it resolves.
            for cr in &contents.card {
                let child_card = idx.card_ref(cr).map(str::to_string);
                push_component(
                    &mut out,
                    tcg_id,
                    ComponentKind::Card,
                    cr.name.clone().unwrap_or_default(),
                    1,
                    None,
                    child_card,
                );
            }
            // Physical extras (spindown, storage box, land packs, …) — textual.
            for or in &contents.other {
                if let Some(name) = or.name.clone() {
                    push_component(&mut out, tcg_id, ComponentKind::Other, name, 1, None, None);
                }
            }
        }
    }
    out
}

/// Append one composition line item (in emission = display order). `quantity` is clamped to
/// `>= 1` (a missing/zero count reads as one).
fn push_component(
    out: &mut Vec<RawComponent>,
    tcg_id: &str,
    kind: ComponentKind,
    name: String,
    quantity: i32,
    child_product: Option<String>,
    child_card: Option<String>,
) {
    out.push(RawComponent {
        tcgplayer_product_id: tcg_id.to_string(),
        kind: kind.as_str(),
        name,
        quantity: quantity.max(1),
        child_tcgplayer_product_id: child_product,
        child_scryfall_id: child_card,
    });
}

/// Lookups built once over the parsed document, shared by the card-membership walk and the
/// composition builder: sets by lowercased code, `uuid` / `(set, number)` -> Scryfall id,
/// and sealed-product `uuid` -> the product it names.
struct Indexes<'a> {
    sets: HashMap<String, &'a SetData>,
    uuid_to_scryfall: HashMap<&'a str, &'a str>,
    setnum_to_scryfall: HashMap<(String, String), &'a str>,
    product_by_uuid: HashMap<&'a str, &'a SealedProduct>,
}

impl<'a> Indexes<'a> {
    fn build(all: &'a AllPrintings) -> Self {
        // Index sets by lowercased code (data keys are uppercase; refs are lowercase).
        let sets: HashMap<String, &SetData> = all
            .data
            .iter()
            .map(|(code, data)| (code.to_lowercase(), data))
            .collect();

        // uuid -> scryfallId and (set_lower, number) -> scryfallId, over every card.
        let mut uuid_to_scryfall: HashMap<&str, &str> = HashMap::new();
        let mut setnum_to_scryfall: HashMap<(String, String), &str> = HashMap::new();
        for data in all.data.values() {
            for card in &data.cards {
                let Some(scryfall) = card.identifiers.scryfall_id.as_deref() else {
                    continue;
                };
                if let Some(uuid) = card.uuid.as_deref() {
                    uuid_to_scryfall.insert(uuid, scryfall);
                }
                if let (Some(set), Some(number)) = (&card.set_code, &card.number) {
                    setnum_to_scryfall.insert((set.to_lowercase(), number.clone()), scryfall);
                }
            }
        }

        // uuid -> the sealed product it names, for `sealed` recursion / child links.
        let mut product_by_uuid: HashMap<&str, &SealedProduct> = HashMap::new();
        for data in all.data.values() {
            for product in &data.sealed_product {
                if let Some(uuid) = product.uuid.as_deref() {
                    product_by_uuid.insert(uuid, product);
                }
            }
        }

        Indexes {
            sets,
            uuid_to_scryfall,
            setnum_to_scryfall,
            product_by_uuid,
        }
    }

    /// Resolve a direct card reference to a Scryfall id (by `uuid`, else `(set, number)`).
    fn card_ref(&self, cr: &ContentCard) -> Option<&str> {
        if let Some(uuid) = cr.uuid.as_deref()
            && let Some(&scryfall) = self.uuid_to_scryfall.get(uuid)
        {
            return Some(scryfall);
        }
        if let (Some(set), Some(number)) = (&cr.set, &cr.number)
            && let Some(&scryfall) = self
                .setnum_to_scryfall
                .get(&(set.to_lowercase(), number.clone()))
        {
            return Some(scryfall);
        }
        None
    }
}

/// Borrows the shared [`Indexes`] for the recursive card-membership walk.
struct Resolver<'a> {
    idx: &'a Indexes<'a>,
}

impl Resolver<'_> {
    /// Push a membership row (deduped by the output set).
    fn push(
        &self,
        tcg_id: &str,
        scryfall: &str,
        membership: Membership,
        foil: bool,
        out: &mut HashSet<RawMembership>,
    ) {
        out.insert(RawMembership {
            tcgplayer_product_id: tcg_id.to_string(),
            scryfall_id: scryfall.to_string(),
            membership: membership.as_str(),
            foil,
        });
    }

    /// Resolve a deck reference to its main-board + commander cards as
    /// `(card_uuid, is_foil)`.
    fn deck_cards(&self, dr: &ContentDeck) -> Vec<(&str, bool)> {
        let (Some(set_code), Some(name)) = (&dr.set, &dr.name) else {
            return Vec::new();
        };
        let Some(set) = self.idx.sets.get(&set_code.to_lowercase()) else {
            return Vec::new();
        };
        let Some(deck) = set
            .decks
            .iter()
            .find(|d| d.name.as_deref() == Some(name.as_str()))
        else {
            return Vec::new();
        };
        deck.main_board
            .iter()
            .chain(deck.commander.iter())
            .filter_map(|dc| dc.uuid.as_deref().map(|uuid| (uuid, dc.is_foil)))
            .collect()
    }

    /// Resolve a booster-pack reference to `(card_uuid, sheet_is_foil)` for every card on
    /// any sheet the pack draws from.
    fn pack_cards(&self, pr: &ContentPack) -> Vec<(&str, bool)> {
        let (Some(set_code), Some(code)) = (&pr.set, &pr.code) else {
            return Vec::new();
        };
        let Some(set) = self.idx.sets.get(&set_code.to_lowercase()) else {
            return Vec::new();
        };
        let Some(config) = set.booster.get(code) else {
            return Vec::new();
        };
        let mut cards = Vec::new();
        for sheet in config.sheets.values() {
            for uuid in sheet.cards.keys() {
                cards.push((uuid.as_str(), sheet.foil));
            }
        }
        cards
    }

    /// Walk a product's contents, emitting membership rows. `membership_override` (set
    /// inside a `variable` config) forces every emitted row to `variable`. `visited`
    /// holds the sealed-product uuids already expanded on this branch (cycle guard).
    fn walk_inner(
        &self,
        tcg_id: &str,
        contents: &Contents,
        membership_override: Option<Membership>,
        visited: &mut HashSet<String>,
        depth: usize,
        out: &mut HashSet<RawMembership>,
    ) {
        // Directly-named cards: definitely in (unless inside a variable option).
        for cr in &contents.card {
            if let Some(scryfall) = self.idx.card_ref(cr) {
                let membership = membership_override.unwrap_or(Membership::Contains);
                self.push(tcg_id, scryfall, membership, cr.foil, out);
            }
        }
        // Precon decks: every deck card is definitely in.
        for dr in &contents.deck {
            for (uuid, is_foil) in self.deck_cards(dr) {
                if let Some(&scryfall) = self.idx.uuid_to_scryfall.get(uuid) {
                    let membership = membership_override.unwrap_or(Membership::Contains);
                    self.push(tcg_id, scryfall, membership, is_foil, out);
                }
            }
        }
        // Booster packs: every sheet card can be pulled.
        for pr in &contents.pack {
            for (uuid, foil) in self.pack_cards(pr) {
                if let Some(&scryfall) = self.idx.uuid_to_scryfall.get(uuid) {
                    let membership = membership_override.unwrap_or(Membership::Booster);
                    self.push(tcg_id, scryfall, membership, foil, out);
                }
            }
        }
        // Variable configs: everything inside is only *maybe* in the product. Build an
        // owned Contents view over the config's members (their vecs are tiny) and walk it
        // with the `variable` override.
        for vr in &contents.variable {
            for config in &vr.configs {
                let view = Contents {
                    card: config.card.iter().map(clone_content_card).collect(),
                    deck: config.deck.iter().map(clone_content_deck).collect(),
                    pack: config.pack.iter().map(clone_content_pack).collect(),
                    sealed: Vec::new(),
                    variable: Vec::new(),
                    other: Vec::new(),
                };
                self.walk_inner(tcg_id, &view, Some(Membership::Variable), visited, depth, out);
            }
        }
        // Nested sealed products: recurse, attributing leaves to the outer product.
        if depth < MAX_SEALED_DEPTH {
            for sr in &contents.sealed {
                let Some(uuid) = sr.uuid.as_deref() else {
                    continue;
                };
                if !visited.insert(uuid.to_string()) {
                    continue; // cycle guard
                }
                if let Some(sub) = self.idx.product_by_uuid.get(uuid)
                    && let Some(sub_contents) = &sub.contents
                {
                    self.walk_inner(
                        tcg_id,
                        sub_contents,
                        membership_override,
                        visited,
                        depth + 1,
                        out,
                    );
                }
            }
        }
    }
}

// The `variable` walk needs an owned `Contents` view over a config's members; these
// clone the small ref structs into it (they carry no heavy data).
fn clone_content_card(c: &ContentCard) -> ContentCard {
    ContentCard {
        uuid: c.uuid.clone(),
        set: c.set.clone(),
        number: c.number.clone(),
        foil: c.foil,
        name: c.name.clone(),
    }
}
fn clone_content_deck(d: &ContentDeck) -> ContentDeck {
    ContentDeck {
        name: d.name.clone(),
        set: d.set.clone(),
    }
}
fn clone_content_pack(p: &ContentPack) -> ContentPack {
    ContentPack {
        code: p.code.clone(),
        set: p.set.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic AllPrintings exercising all five content categories: a direct promo
    /// card (`contains`), a precon deck (`contains`), a booster pack (`booster`), a
    /// variable option (`may`/`variable`), and a nested `sealed` box that yields its
    /// packs' cards as `booster`.
    fn fixture() -> AllPrintings {
        let json = serde_json::json!({
            "meta": { "version": "5.3.0+x", "date": "2026-07-03" },
            "data": {
                "SET": {
                    "cards": [
                        { "uuid": "u-alpha", "number": "1", "setCode": "SET",
                          "identifiers": { "scryfallId": "sf-alpha" } },
                        { "uuid": "u-beta", "number": "2", "setCode": "SET",
                          "identifiers": { "scryfallId": "sf-beta" } },
                        { "uuid": "u-promo", "number": "300", "setCode": "SET",
                          "identifiers": { "scryfallId": "sf-promo" } },
                        { "uuid": "u-rare", "number": "10", "setCode": "SET",
                          "identifiers": { "scryfallId": "sf-rare" } }
                    ],
                    "booster": {
                        "draft": {
                            "sheets": {
                                "common": { "foil": false, "cards": { "u-alpha": 1, "u-beta": 1 } },
                                "foilRare": { "foil": true, "cards": { "u-rare": 1 } }
                            }
                        }
                    },
                    "decks": [
                        { "name": "Precon A", "code": "SET",
                          "mainBoard": [ { "count": 1, "uuid": "u-alpha", "isFoil": false } ],
                          "commander": [ { "count": 1, "uuid": "u-promo", "isFoil": true } ] }
                    ],
                    "sealedProduct": [
                        { "uuid": "p-pack", "identifiers": { "tcgplayerProductId": "1001" },
                          "contents": { "pack": [ { "code": "draft", "set": "set" } ] } },
                        { "uuid": "p-box", "identifiers": { "tcgplayerProductId": "1002" },
                          "contents": { "sealed": [
                              { "uuid": "p-pack", "count": 36, "name": "Draft Booster Pack" }
                          ] } },
                        { "uuid": "p-deck", "identifiers": { "tcgplayerProductId": "1003" },
                          "contents": { "deck": [ { "name": "Precon A", "set": "set" } ] } },
                        { "uuid": "p-bundle", "identifiers": { "tcgplayerProductId": "1004" },
                          "contents": {
                              "card": [ { "set": "set", "number": "300", "foil": true, "name": "Foil Promo" } ],
                              "other": [ { "name": "Spindown" }, { "name": "Storage Box" } ]
                          } },
                        { "uuid": "p-sld", "identifiers": { "tcgplayerProductId": "1005" },
                          "contents": { "variable": [ { "configs": [
                              { "card": [ { "uuid": "u-promo", "foil": true } ] }
                          ] } ] } },
                        { "uuid": "p-noid", "contents": { "card": [ { "uuid": "u-alpha" } ] } }
                    ]
                }
            }
        });
        serde_json::from_value(json).expect("fixture parses")
    }

    fn has(rows: &[RawMembership], tcg: &str, sf: &str, m: &str, foil: bool) -> bool {
        rows.iter().any(|r| {
            r.tcgplayer_product_id == tcg
                && r.scryfall_id == sf
                && r.membership == m
                && r.foil == foil
        })
    }

    #[test]
    fn pack_yields_booster_rows_with_per_sheet_foil() {
        let rows = build_memberships(&fixture());
        // The draft pack (1001) can yield the two common (non-foil) + the rare (foil).
        assert!(has(&rows, "1001", "sf-alpha", "booster", false));
        assert!(has(&rows, "1001", "sf-beta", "booster", false));
        assert!(has(&rows, "1001", "sf-rare", "booster", true));
    }

    #[test]
    fn sealed_box_inherits_its_packs_cards() {
        let rows = build_memberships(&fixture());
        // The box (1002) contains the pack, so its cards can be pulled from the box too.
        assert!(has(&rows, "1002", "sf-alpha", "booster", false));
        assert!(has(&rows, "1002", "sf-rare", "booster", true));
    }

    #[test]
    fn deck_cards_are_contains() {
        let rows = build_memberships(&fixture());
        // Precon deck (1003): the main-board card + the foil commander are definitely in.
        assert!(has(&rows, "1003", "sf-alpha", "contains", false));
        assert!(has(&rows, "1003", "sf-promo", "contains", true));
    }

    #[test]
    fn direct_card_resolves_by_set_and_number() {
        let rows = build_memberships(&fixture());
        // The bundle (1004) names its promo by (set, number) only — resolved via the map.
        assert!(has(&rows, "1004", "sf-promo", "contains", true));
    }

    #[test]
    fn variable_options_are_may_be_in() {
        let rows = build_memberships(&fixture());
        // The Secret-Lair-style product (1005) only *may* contain the promo.
        assert!(has(&rows, "1005", "sf-promo", "variable", true));
        // …and it is not asserted as a definite `contains`.
        assert!(!has(&rows, "1005", "sf-promo", "contains", true));
    }

    #[test]
    fn product_without_tcgplayer_id_is_skipped() {
        let rows = build_memberships(&fixture());
        // p-noid has contents but no tcgplayerProductId, so it contributes nothing: the
        // only product ids in the output are the five that carry a tcgplayerProductId.
        let products: HashSet<&str> = rows.iter().map(|r| r.tcgplayer_product_id.as_str()).collect();
        assert_eq!(
            products,
            HashSet::from(["1001", "1002", "1003", "1004", "1005"]),
            "only products with a tcgplayerProductId appear"
        );
    }

    #[test]
    fn rows_are_deduplicated() {
        let rows = build_memberships(&fixture());
        let mut seen = HashSet::new();
        for r in &rows {
            assert!(
                seen.insert(r.clone()),
                "duplicate membership row: {r:?}"
            );
        }
    }

    /// AllPrintings is a ~600 MB third-party document; a single object missing an
    /// expected field must never abort the whole parse. Every reference field is
    /// optional, and a card with no `uuid` still resolves by `(set, number)`, while a
    /// sealed/deck/pack reference missing its key is skipped rather than fatal.
    #[test]
    fn tolerates_missing_optional_fields() {
        let json = serde_json::json!({
            "data": { "SET": {
                // A card with NO uuid — resolvable only by (set, number).
                "cards": [
                    { "number": "5", "setCode": "SET", "identifiers": { "scryfallId": "sf-x" } }
                ],
                "sealedProduct": [
                    { "identifiers": { "tcgplayerProductId": "2001" },
                      "contents": {
                        "card": [ { "set": "set", "number": "5" } ],
                        // References missing their uuid / code — skipped, not fatal.
                        "sealed": [ { "name": "no uuid here" } ],
                        "deck": [ { "set": "set" } ],
                        "pack": [ { "set": "set" } ]
                      } }
                ]
            } }
        });
        let all: AllPrintings = serde_json::from_value(json).expect("parses despite missing fields");
        let rows = build_memberships(&all);
        // The uuid-less card still resolves by (set, number).
        assert!(rows.iter().any(|r| {
            r.tcgplayer_product_id == "2001"
                && r.scryfall_id == "sf-x"
                && r.membership == "contains"
        }));
        // The malformed sealed/deck/pack references contributed nothing but didn't panic.
        assert_eq!(rows.len(), 1);
    }

    // ---------- Composition (build_compositions) ----------

    fn find_component<'a>(
        rows: &'a [RawComponent],
        tcg: &str,
        kind: &str,
        name: &str,
    ) -> Option<&'a RawComponent> {
        rows.iter()
            .find(|c| c.tcgplayer_product_id == tcg && c.kind == kind && c.name == name)
    }

    /// A `sealed` component keeps its count and links to the sub-product it references
    /// (by uuid -> the child's tcgplayerProductId).
    #[test]
    fn sealed_component_links_to_child_product_with_count() {
        let comps = build_compositions(&fixture());
        // The box (1002) is "36× Draft Booster Pack", linked to the pack product (1001).
        let boxed = find_component(&comps, "1002", "sealed", "Draft Booster Pack")
            .expect("box lists its packs");
        assert_eq!(boxed.quantity, 36);
        assert_eq!(boxed.child_tcgplayer_product_id.as_deref(), Some("1001"));
    }

    /// Decks, promo cards, and physical extras all surface as line items; a promo card
    /// resolves to a Scryfall id link, the extras stay textual.
    #[test]
    fn deck_card_and_other_components_are_emitted() {
        let comps = build_compositions(&fixture());
        assert!(find_component(&comps, "1003", "deck", "Precon A").is_some());
        let promo =
            find_component(&comps, "1004", "card", "Foil Promo").expect("bundle lists its promo");
        assert_eq!(promo.child_scryfall_id.as_deref(), Some("sf-promo"));
        assert!(find_component(&comps, "1004", "other", "Spindown").is_some());
        assert!(find_component(&comps, "1004", "other", "Storage Box").is_some());
    }

    /// A bare booster pack (contents is a single `pack` sheet config) and a variable-only
    /// product contribute no composition — `pack` / `variable` are card-resolution only.
    #[test]
    fn pack_only_and_variable_products_have_no_composition() {
        let comps = build_compositions(&fixture());
        assert!(comps.iter().all(|c| c.tcgplayer_product_id != "1001"));
        assert!(comps.iter().all(|c| c.tcgplayer_product_id != "1005"));
    }

    /// Components are emitted in display order (kinds grouped: sealed → deck → card →
    /// other) — the bundle has no sealed/deck, so its promo card leads, then the extras.
    #[test]
    fn components_are_emitted_in_display_order() {
        let comps = build_compositions(&fixture());
        let bundle: Vec<&str> = comps
            .iter()
            .filter(|c| c.tcgplayer_product_id == "1004")
            .map(|c| c.kind)
            .collect();
        assert_eq!(bundle, vec!["card", "other", "other"]);
    }
}
