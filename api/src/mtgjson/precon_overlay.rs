//! Preconstructed decks MTGJSON has published a *product* for but no card list.
//!
//! MTGJSON files a precon's decklist under its set's `decks[]`, and
//! [`super::precons`] derives every precon row from exactly that. When upstream ships the
//! sealed product ahead of the list, the reference dangles: `SLD`'s
//! `Secret Lair Commander Deck Hatsune Miku` declares
//! `contents.deck = [{ name: "Hatsune Miku", set: "sld" }]` and `cardCount: 100`, but no deck
//! by that name exists in `decks[]`. The product is browsable, its Secret Lair printings are
//! in the catalog, and the deck itself is simply absent from `/decks/mtg/precons/sets/sld`.
//!
//! This module is the stopgap: a committed file of decks to derive *as if* upstream had
//! listed them, merged after the real walk in [`super::precons::precons_from`].
//!
//! Three properties make it safe to leave in place:
//!
//! * **It retires itself.** An entry stands down the moment upstream publishes the deck —
//!   see [`covered_upstream`] — so the file shrinks by deletion at leisure rather than
//!   needing a removal commit racing the next sync. Nothing here ever shadows real data.
//! * **It is data, not a second derivation.** An entry becomes an ordinary
//!   [`RawPrecon`] and travels the one ingest path, so facets, slugging, pricing and the
//!   copy-to-deck seam treat it exactly like an upstream row.
//! * **It rides the version gate.** [`version()`] hashes the file into the ingest's
//!   `DERIVATION_VERSION`, so editing it forces the one rebuild that makes the edit visible
//!   — the sync is otherwise ETag-gated and a pure data edit would take effect no other way.
//!
//! **Adding an entry is transcription, and the printings have to be real.** A `deck_cards`
//! row addresses a *printing*, so every card needs a Scryfall id, not a name. Take the card
//! list from the publisher, and where the source names a set but not a collector number,
//! resolve it to that set's plain printing — black-bordered, non-promo, no frame effects —
//! which is what a precon ships; the fancy treatments are the collector-booster variants.
//! Record what each id was in the entry's `card` label so the next reader can audit it
//! without re-deriving the list.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::precons::{RawPrecon, RawPreconCard};
use crate::entities::precon_deck_card::PreconBoard;

const OVERLAY_JSON: &str = include_str!("precon_overlay.json");

#[derive(Debug, Default, Deserialize)]
pub(super) struct OverlayData {
    #[serde(default)]
    pub(super) decks: Vec<OverlayDeck>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OverlayDeck {
    /// The name upstream is expected to use, so the slug this mints is the slug the deck
    /// keeps once MTGJSON catches up and a shared URL survives the handover. For the Miku
    /// deck that is the dangling reference's own `"Hatsune Miku"`, not the product's longer
    /// retail name.
    pub(super) name: String,
    pub(super) set_code: String,
    pub(super) deck_type: String,
    #[serde(default)]
    pub(super) released_at: Option<String>,
    /// The sealed product this deck comes in, as a TCGplayer id — the identity half of
    /// [`covered_upstream`], and the same link an upstream deck gets from its
    /// `sealedProductUuids`.
    pub(super) product_tcgplayer_id: String,
    /// Where the card list came from, and anything the transcription had to decide. Never
    /// read at runtime — it exists so the next reader can re-check the entry against its
    /// source instead of trusting it.
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) source_url: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) source_note: Option<String>,
    pub(super) cards: Vec<OverlayCard>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OverlayCard {
    /// One of [`PreconBoard`]'s string values.
    pub(super) board: String,
    pub(super) scryfall_id: String,
    pub(super) quantity: i32,
    #[serde(default)]
    pub(super) foil: bool,
    pub(super) position: i32,
    /// `Name [SET number]`, for auditing the id above. Never read at runtime.
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) card: Option<String>,
}

static DATA: LazyLock<OverlayData> = LazyLock::new(|| {
    serde_json::from_str(OVERLAY_JSON).unwrap_or_else(|err| {
        // A malformed committed file degrades to "no overlay" rather than taking the sync
        // down — the same stance `fallback` takes. `bundled_overlay_is_valid` guards the
        // shipped file at test time, so this only ever fires on an unreviewed edit.
        tracing::error!(error = %err, "failed to parse precon_overlay.json; overlay disabled");
        OverlayData::default()
    })
});

/// A stable content hash of the bundled overlay. The ingest folds it into the stored
/// derivation tag so an overlay-only edit still forces a rebuild on the next sync.
pub fn version() -> &'static str {
    static VERSION: LazyLock<String> =
        LazyLock::new(|| hex::encode(&Sha256::digest(OVERLAY_JSON.as_bytes())[..8]));
    &VERSION
}

fn data() -> &'static OverlayData {
    &DATA
}

/// Whether upstream already lists `deck`, in which case the overlay entry must not add a
/// second row for it.
///
/// Two independent tests, because either one alone has a way to miss. Matching on the
/// **product** is the real identity — that is what the deck *is*, and it survives upstream
/// naming the deck something we didn't predict — but MTGJSON can publish a decklist without
/// linking it to a `sealedProduct`, and then only the **name** catches it. Both are scoped
/// to the set, so a same-named deck in another set is untouched.
fn covered_upstream(derived: &[RawPrecon], deck: &OverlayDeck) -> bool {
    let set_code = deck.set_code.trim().to_lowercase();
    let name = deck.name.trim().to_lowercase();
    derived.iter().any(|existing| {
        existing.set_code == set_code
            && (existing
                .product_ids
                .iter()
                .any(|id| id.trim() == deck.product_tcgplayer_id.trim())
                || existing.name.trim().to_lowercase() == name)
    })
}

/// Append every overlay deck upstream hasn't published to `derived`, claiming slugs from the
/// same counter the real walk used.
///
/// Called after that walk, never before: the upstream row must win the base slug whenever
/// both exist, or the day MTGJSON catches up a live URL would silently move to `-2`.
///
/// `product_present(set_code, tcgplayer_id)` answers whether the document being derived
/// actually holds the sealed product the entry stands in for. That is the precise condition
/// an entry claims — *upstream shipped this product without a card list* — so gating on it
/// keeps the overlay strictly additive: a document that never mentions the product (a test
/// fixture, a trimmed mirror, a future MTGJSON that withdrew it) derives exactly as it did
/// before this file existed, rather than growing a deck out of nowhere.
pub(super) fn merge_into(
    derived: &mut Vec<RawPrecon>,
    used_slugs: &mut HashMap<String, u32>,
    product_present: impl Fn(&str, &str) -> bool,
) {
    for deck in &data().decks {
        let set_code = deck.set_code.trim().to_lowercase();
        if !product_present(&set_code, deck.product_tcgplayer_id.trim()) {
            continue;
        }
        if covered_upstream(derived, deck) {
            continue;
        }
        let Some(precon) = build(deck, used_slugs) else {
            continue;
        };
        derived.push(precon);
    }
}

/// Resolve one overlay entry into a [`RawPrecon`], or `None` when it carries no usable card.
///
/// An entry with no name, or whose every row names a board we don't have, would list as a
/// deck with no cards — the same shell [`super::precons`] refuses upstream.
fn build(deck: &OverlayDeck, used_slugs: &mut HashMap<String, u32>) -> Option<RawPrecon> {
    let name = deck.name.trim();
    if name.is_empty() {
        return None;
    }
    let set_code = deck.set_code.trim().to_lowercase();
    let cards: Vec<RawPreconCard> = deck
        .cards
        .iter()
        .filter_map(|card| {
            let scryfall_id = card.scryfall_id.trim();
            // A quantity of zero or less is not a copy; a board we can't name would be
            // stored as a string no reader splits on.
            if scryfall_id.is_empty() || card.quantity <= 0 {
                return None;
            }
            Some(RawPreconCard {
                scryfall_id: scryfall_id.to_string(),
                board: board_of(&card.board)?,
                quantity: card.quantity,
                foil: card.foil,
                position: card.position,
            })
        })
        .collect();
    if cards.is_empty() {
        return None;
    }
    Some(RawPrecon {
        slug: super::precons::unique_slug(name, &set_code, used_slugs),
        name: name.to_string(),
        set_code,
        deck_type: deck.deck_type.trim().to_string(),
        released_at: deck
            .released_at
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        product_ids: vec![deck.product_tcgplayer_id.trim().to_string()],
        cards,
    })
}

/// The board string as one of [`PreconBoard`]'s own spellings, so an overlay can't invent a
/// fourth board that every downstream zone reader would then ignore.
fn board_of(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "main" => Some(PreconBoard::Main.as_str()),
        "commander" => Some(PreconBoard::Commander.as_str()),
        "side" => Some(PreconBoard::Side.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, set_code: &str, product_ids: &[&str]) -> RawPrecon {
        RawPrecon {
            slug: format!("{name}-{set_code}"),
            name: name.to_string(),
            set_code: set_code.to_string(),
            deck_type: "Commander Deck".to_string(),
            released_at: None,
            product_ids: product_ids.iter().map(|id| id.to_string()).collect(),
            cards: Vec::new(),
        }
    }

    /// The shipped file parses and every entry is well-formed, so a bad transcription fails
    /// CI rather than silently listing a broken deck.
    #[test]
    fn bundled_overlay_is_valid() {
        for deck in &data().decks {
            assert!(!deck.name.trim().is_empty(), "entry has a name");
            assert!(!deck.set_code.trim().is_empty(), "{} has a set", deck.name);
            assert!(
                !deck.product_tcgplayer_id.trim().is_empty(),
                "{} names its product",
                deck.name
            );
            assert!(!deck.cards.is_empty(), "{} has cards", deck.name);
            assert!(
                deck.source_url
                    .as_deref()
                    .is_some_and(|u| u.starts_with("http")),
                "{} cites where its card list came from",
                deck.name
            );
            for card in &deck.cards {
                assert!(
                    board_of(&card.board).is_some(),
                    "{} card on a real board, got {:?}",
                    deck.name,
                    card.board
                );
                assert!(
                    !card.scryfall_id.trim().is_empty(),
                    "{} card has a scryfall id",
                    deck.name
                );
                assert!(card.quantity > 0, "{} card has copies", deck.name);
            }
            // A printing may legitimately repeat across boards, but never within one: the
            // table is unique on `(deck, board, card, finish)`, so a duplicate row here is a
            // failed insert at ingest, not a duplicate tile.
            let mut seen = std::collections::HashSet::new();
            for card in &deck.cards {
                assert!(
                    seen.insert((&card.board, &card.scryfall_id, card.foil)),
                    "{} lists {} twice on {}",
                    deck.name,
                    card.scryfall_id,
                    card.board
                );
            }
        }
    }

    /// The Miku deck is the reason this file exists; pin what it claims to be so a careless
    /// edit to the transcription trips here. 100 cards, 26 of them foil (WotC ships 12 new-art
    /// borderless foils plus 7 foil Plains and 7 foil Forests), one commander.
    #[test]
    fn miku_entry_matches_the_published_deck() {
        let deck = data()
            .decks
            .iter()
            .find(|d| d.name == "Hatsune Miku")
            .expect("the Miku entry is present");
        assert_eq!(deck.set_code, "sld");
        assert_eq!(deck.deck_type, "Commander Deck");
        assert_eq!(deck.product_tcgplayer_id, "709981");

        let total: i32 = deck.cards.iter().map(|c| c.quantity).sum();
        assert_eq!(total, 100, "a Commander deck is 100 cards");
        let foil: i32 = deck
            .cards
            .iter()
            .filter(|c| c.foil)
            .map(|c| c.quantity)
            .sum();
        assert_eq!(foil, 26, "12 new-art foils + 7 Plains + 7 Forests");
        let commanders: i32 = deck
            .cards
            .iter()
            .filter(|c| c.board == "commander")
            .map(|c| c.quantity)
            .sum();
        assert_eq!(commanders, 1, "Trostani leads alone");
    }

    /// With nothing upstream, the overlay contributes its decks and claims a slug.
    #[test]
    fn merges_when_upstream_is_silent() {
        let mut derived = Vec::new();
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |_, _| true);
        let miku = derived
            .iter()
            .find(|d| d.name == "Hatsune Miku")
            .expect("overlay contributed the deck");
        // The slug upstream itself would mint from the dangling reference's name, so the URL
        // survives the handover.
        assert_eq!(miku.slug, "hatsune-miku-sld");
        assert_eq!(miku.product_ids, vec!["709981".to_string()]);
        assert!(miku.cards.iter().any(|c| c.board == "commander"));
    }

    /// A document that doesn't hold the product an entry stands in for derives exactly as it
    /// did before this file existed — the overlay never invents a deck for a set upstream
    /// described without it.
    #[test]
    fn an_absent_product_stands_the_entry_down() {
        let mut derived = Vec::new();
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |_, _| false);
        assert!(derived.is_empty(), "nothing to stand in for");
    }

    /// The gate is asked about the entry's own set and product, lowercased set code first.
    #[test]
    fn the_gate_is_asked_about_this_entry() {
        // A `Fn` predicate on purpose — the gate is a pure question about the document —
        // so the test records through a cell rather than loosening it to `FnMut`.
        let asked = std::cell::RefCell::new(Vec::new());
        let mut derived = Vec::new();
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |set_code, product_id| {
            asked
                .borrow_mut()
                .push((set_code.to_string(), product_id.to_string()));
            false
        });
        let asked = asked.into_inner();
        assert!(
            asked.contains(&("sld".to_string(), "709981".to_string())),
            "asked about the Miku product: {asked:?}"
        );
    }

    /// Upstream publishing the deck under the predicted name stands the overlay down.
    #[test]
    fn stands_down_on_a_name_match() {
        let mut derived = vec![raw("Hatsune Miku", "sld", &[])];
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |_, _| true);
        assert_eq!(derived.len(), 1, "no second row for the same deck");
    }

    /// …and so does publishing it under a name we didn't predict, because the product link
    /// is the real identity.
    #[test]
    fn stands_down_on_a_product_match() {
        let mut derived = vec![raw(
            "Secret Lair Commander Deck: Hatsune Miku",
            "sld",
            &["709981"],
        )];
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |_, _| true);
        assert_eq!(derived.len(), 1, "matched on the TCGplayer product id");
    }

    /// A same-named deck in a *different* set is a different deck and must not stand the
    /// overlay down.
    #[test]
    fn another_set_does_not_stand_the_entry_down() {
        let mut derived = vec![raw("Hatsune Miku", "tmc", &["709981"])];
        let mut slugs = HashMap::new();
        merge_into(&mut derived, &mut slugs, |_, _| true);
        assert_eq!(derived.len(), 2, "the sld entry still lands");
    }

    /// Editing the file has to move the hash, or the ETag-gated sync would never rebuild and
    /// the edit would be invisible until an unrelated bump.
    #[test]
    fn version_is_stable_and_nonempty() {
        assert_eq!(version(), version());
        assert_eq!(version().len(), 16);
    }

    #[test]
    fn unknown_boards_are_refused() {
        assert_eq!(board_of("Main"), Some("main"));
        assert_eq!(board_of(" commander "), Some("commander"));
        assert_eq!(board_of("maybeboard"), None);
    }
}
