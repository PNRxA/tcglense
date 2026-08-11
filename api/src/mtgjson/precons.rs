//! **Preconstructed decks**: the published decklists that ship inside a set's products —
//! Commander decks, Planeswalker / Challenger / Starter decks, Jumpstart themes, Secret
//! Lair drops, and the rest.
//!
//! This is the *pure* half (no DB, no network): given a parsed `AllPrintings`, walk every
//! set's `decks[]` and resolve each board's `uuid` references into per-card rows keyed by
//! Scryfall id, exactly as [`super::model::build_memberships`] resolves a product's
//! contents. [`super::ingest::precons`] maps those onto our catalog and rebuilds the
//! `precon_decks` / `precon_deck_cards` tables.
//!
//! **No new fetch.** The decklists are already in the one `AllPrintings.json` the sealed
//! sync downloads, and this pass borrows the same parsed document *and* the same
//! [`Indexes`](super::model) the other two passes use — a fourth dataset that took its own
//! copy of either would double a 600 MB parse for data that arrived with the first one.
//!
//! Two invariants the rest of the feature leans on:
//!
//! * **The slug is the identity, not the id.** The tables are rebuilt wholesale on every
//!   sync, so primary keys are re-minted; `slug` (`turtle-power-tmc`) is what a URL, a
//!   bookmark and a copy address. It is therefore derived deterministically — sets are
//!   walked in sorted code order and decks in upstream's own order — so the same document
//!   always produces the same slugs, including the `-2` suffix a collision takes.
//! * **A board is upstream's, not a section name.** `main` / `commander` / `side` are what
//!   MTGJSON states; turning them into deck *sections* is the copy endpoint's job
//!   ([`crate::handlers::precons::copy`]), and that mapping is what makes a copied precon
//!   read correctly to the legality and analytics modules.

use std::collections::HashMap;

use crate::entities::precon_deck_card::PreconBoard;

use super::model::{AllPrintings, Deck, DeckCard, Indexes};

/// Longest slug we mint. Deck names are short; the cap only guards a pathological upstream
/// name against the column, and truncation happens *before* the collision suffix so two
/// long names that share a prefix still get distinct slugs.
const MAX_SLUG: usize = 120;

/// Fallback category for a deck upstream didn't type. Reads as a facet value in the UI
/// rather than an empty chip.
const UNTYPED: &str = "Preconstructed Deck";

/// One resolved precon deck: its metadata plus every board's cards, keyed by **external**
/// ids (Scryfall id for cards, TCGplayer product id for the sealed products that ship it) so
/// the DB layer resolves both sides to internal ids — the same shape
/// [`RawMembership`](super::model::RawMembership) travels in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPrecon {
    /// URL identity, unique within the returned batch.
    pub slug: String,
    pub name: String,
    /// Lowercased set code, matching `cards.set_code`.
    pub set_code: String,
    pub deck_type: String,
    pub released_at: Option<String>,
    /// TCGplayer product ids of the sealed products that ship this deck (first one wins as
    /// the stored link; the rest are simply not represented).
    pub product_ids: Vec<String>,
    /// Every board's cards, in board order (commander, main, side) and upstream's order
    /// within each board.
    pub cards: Vec<RawPreconCard>,
}

/// One card on one board of a precon: `quantity` copies of a Scryfall printing, in one
/// finish. Aggregated per `(board, scryfall_id, foil)` so the row set matches the unique key
/// the table carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPreconCard {
    pub scryfall_id: String,
    /// One of [`PreconBoard`]'s string values.
    pub board: &'static str,
    pub quantity: i32,
    pub foil: bool,
    /// Upstream's listing order within the board.
    pub position: i32,
}

/// Resolve every set's `decks[]` into [`RawPrecon`]s over a prebuilt index.
///
/// A deck with no name, or whose cards all fail to resolve to a Scryfall id, contributes
/// nothing — an empty shell would list as a deck with no cards. Everything else is kept,
/// including decks whose cards our catalog may not hold: that's the ingest's call to make
/// once it has tried to resolve them, not this pass's.
pub(super) fn precons_from(all: &AllPrintings, idx: &Indexes) -> Vec<RawPrecon> {
    // Sets are keyed in a HashMap, so walk them in sorted code order: slug de-duplication
    // below depends on a deterministic visit order, or a rebuild could hand the same two
    // colliding decks each other's URL.
    let mut set_codes: Vec<&String> = all.data.keys().collect();
    set_codes.sort_unstable();

    let mut out: Vec<RawPrecon> = Vec::new();
    let mut used_slugs: HashMap<String, u32> = HashMap::new();
    for set_code in set_codes {
        let Some(data) = all.data.get(set_code) else {
            continue;
        };
        for deck in &data.decks {
            let Some(precon) = build_one(deck, set_code, idx, &mut used_slugs) else {
                continue;
            };
            out.push(precon);
        }
    }
    out
}

/// [`precons_from`] building its own index — the entry point the unit tests use. The sync
/// itself always passes the index it already built for the membership + composition passes.
#[cfg(test)]
pub fn build_precons(all: &AllPrintings) -> Vec<RawPrecon> {
    precons_from(all, &Indexes::build(all))
}

/// Resolve one deck, or `None` when it has no name or no card that resolves.
fn build_one(
    deck: &Deck,
    fallback_set: &str,
    idx: &Indexes,
    used_slugs: &mut HashMap<String, u32>,
) -> Option<RawPrecon> {
    let name = deck
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())?;
    // The deck states its own set; fall back to the set it's filed under.
    let set_code = deck
        .code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(fallback_set)
        .to_lowercase();

    // Boards in reading order: the command zone leads, then the deck, then the sideboard.
    let mut cards: Vec<RawPreconCard> = Vec::new();
    resolve_board(&deck.commander, PreconBoard::Commander, idx, &mut cards);
    resolve_board(&deck.main_board, PreconBoard::Main, idx, &mut cards);
    resolve_board(&deck.side_board, PreconBoard::Side, idx, &mut cards);
    if cards.is_empty() {
        return None;
    }

    let product_ids = deck
        .sealed_product_uuids
        .iter()
        .filter_map(|uuid| idx.product_tcg_id(uuid).map(str::to_string))
        .collect();

    Some(RawPrecon {
        slug: unique_slug(name, &set_code, used_slugs),
        name: name.to_string(),
        set_code,
        deck_type: deck
            .deck_type
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(UNTYPED)
            .to_string(),
        released_at: deck
            .release_date
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        product_ids,
        cards,
    })
}

/// Resolve one board's `uuid` references, aggregating copies per `(card, finish)` and
/// appending them to `out` in upstream's order.
///
/// Aggregation is what lets the table carry a unique `(deck, board, card, finish)` key: two
/// entries can land on the same printing (two `uuid`s that share a Scryfall id, or a list
/// that repeats a card instead of counting it), and a precon is read-only, so summing them
/// is the only reading that keeps the copy count right.
fn resolve_board(
    board: &[DeckCard],
    kind: PreconBoard,
    idx: &Indexes,
    out: &mut Vec<RawPreconCard>,
) {
    let start = out.len();
    let mut seen: HashMap<(String, bool), usize> = HashMap::new();
    for entry in board {
        let Some(uuid) = entry.uuid.as_deref() else {
            continue;
        };
        let Some(scryfall) = idx.scryfall_by_uuid(uuid) else {
            continue;
        };
        // A missing count reads as one copy; a negative one as none.
        let quantity = entry.count.unwrap_or(1).max(0);
        if quantity == 0 {
            continue;
        }
        match seen.get(&(scryfall.to_string(), entry.is_foil)) {
            Some(&index) => out[index].quantity += quantity,
            None => {
                seen.insert((scryfall.to_string(), entry.is_foil), out.len());
                let position = (out.len() - start) as i32;
                out.push(RawPreconCard {
                    scryfall_id: scryfall.to_string(),
                    board: kind.as_str(),
                    quantity,
                    foil: entry.is_foil,
                    position,
                });
            }
        }
    }
}

/// Mint a slug for a deck, suffixing `-2`, `-3`, … when the name + set already produced one.
///
/// Collisions are real: a Secret Lair drop and its "Foil Edition" slug apart, but two decks
/// in one set *can* share a name after punctuation is stripped. The counter is per resolved
/// slug and the walk order is deterministic, so the same document always assigns the same
/// suffix to the same deck.
fn unique_slug(name: &str, set_code: &str, used: &mut HashMap<String, u32>) -> String {
    let base = match slugify(name) {
        // A name that slugifies to nothing (all punctuation / non-ASCII) still needs a URL.
        text if text.is_empty() => slugify(set_code),
        text => format!("{text}-{}", slugify(set_code)),
    };
    let base = if base.is_empty() {
        "deck".to_string()
    } else {
        base
    };
    let count = used.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}-{count}")
    }
}

/// Lowercase ASCII slug: alphanumerics kept, every other character a separator, runs
/// collapsed, ends trimmed, truncated to [`MAX_SLUG`].
fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(MAX_SLUG));
    let mut pending_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !out.is_empty() {
                out.push('-');
            }
            pending_separator = false;
            out.push(ch.to_ascii_lowercase());
            if out.len() >= MAX_SLUG {
                break;
            }
        } else {
            pending_separator = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-set document: a Commander precon (command zone + deck + a sideboard card, one
    /// card listed twice) and two same-named Secret Lair drops that must slug apart.
    fn fixture() -> AllPrintings {
        let json = serde_json::json!({
            "data": {
                "TMC": {
                    "cards": [
                        { "uuid": "u-leo", "identifiers": { "scryfallId": "sf-leo" } },
                        { "uuid": "u-shell", "identifiers": { "scryfallId": "sf-shell" } },
                        { "uuid": "u-island", "identifiers": { "scryfallId": "sf-island" } },
                        { "uuid": "u-side", "identifiers": { "scryfallId": "sf-side" } }
                    ],
                    "sealedProduct": [
                        { "uuid": "p-deck", "identifiers": { "tcgplayerProductId": "657865" } }
                    ],
                    "decks": [{
                        "code": "TMC",
                        "name": "Turtle Power!",
                        "type": "Commander Deck",
                        "releaseDate": "2026-03-06",
                        "sealedProductUuids": ["p-deck"],
                        "commander": [ { "count": 1, "uuid": "u-shell", "isFoil": true } ],
                        "mainBoard": [
                            { "count": 1, "uuid": "u-leo", "isFoil": true },
                            { "count": 20, "uuid": "u-island", "isFoil": false },
                            // Same printing listed again: the counts aggregate onto one row.
                            { "count": 4, "uuid": "u-island", "isFoil": false },
                            // Unresolvable uuid — skipped, not fatal.
                            { "count": 1, "uuid": "u-missing", "isFoil": false }
                        ],
                        "sideBoard": [ { "count": 2, "uuid": "u-side", "isFoil": false } ]
                    }]
                },
                "SLD": {
                    "cards": [
                        { "uuid": "u-drop", "identifiers": { "scryfallId": "sf-drop" } }
                    ],
                    "decks": [
                        { "code": "SLD", "name": "A Box of Rocks", "type": "Secret Lair Drop",
                          "releaseDate": "2021-03-15",
                          "mainBoard": [ { "count": 1, "uuid": "u-drop" } ] },
                        // Punctuation-only difference: the two slugs must not be the same URL.
                        { "code": "SLD", "name": "A Box of Rocks!", "type": "Secret Lair Drop",
                          "releaseDate": "2021-03-15",
                          "mainBoard": [ { "count": 1, "uuid": "u-drop", "isFoil": true } ] },
                        // No name, and a named deck whose every card is unresolvable: both dropped.
                        { "code": "SLD", "type": "Secret Lair Drop",
                          "mainBoard": [ { "count": 1, "uuid": "u-drop" } ] },
                        { "code": "SLD", "name": "Ghost Drop", "type": "Secret Lair Drop",
                          "mainBoard": [ { "count": 1, "uuid": "u-nope" } ] }
                    ]
                }
            }
        });
        serde_json::from_value(json).expect("fixture parses")
    }

    fn find<'a>(precons: &'a [RawPrecon], slug: &str) -> &'a RawPrecon {
        precons
            .iter()
            .find(|p| p.slug == slug)
            .unwrap_or_else(|| panic!("no precon slugged {slug}: {:?}", slugs(precons)))
    }

    fn slugs(precons: &[RawPrecon]) -> Vec<&str> {
        precons.iter().map(|p| p.slug.as_str()).collect()
    }

    #[test]
    fn resolves_every_board_with_counts_and_finishes() {
        let precons = build_precons(&fixture());
        let deck = find(&precons, "turtle-power-tmc");
        assert_eq!(deck.name, "Turtle Power!");
        assert_eq!(deck.set_code, "tmc");
        assert_eq!(deck.deck_type, "Commander Deck");
        assert_eq!(deck.released_at.as_deref(), Some("2026-03-06"));
        assert_eq!(deck.product_ids, vec!["657865".to_string()]);

        let by_board = |board: &str| -> Vec<(&str, i32, bool)> {
            deck.cards
                .iter()
                .filter(|c| c.board == board)
                .map(|c| (c.scryfall_id.as_str(), c.quantity, c.foil))
                .collect()
        };
        assert_eq!(by_board("commander"), vec![("sf-shell", 1, true)]);
        // The repeated island aggregates to 24 copies on one row, and the unresolvable
        // uuid contributed nothing.
        assert_eq!(
            by_board("main"),
            vec![("sf-leo", 1, true), ("sf-island", 24, false)]
        );
        assert_eq!(by_board("side"), vec![("sf-side", 2, false)]);
    }

    #[test]
    fn positions_are_per_board_and_start_at_zero() {
        let precons = build_precons(&fixture());
        let deck = find(&precons, "turtle-power-tmc");
        let positions: Vec<(&str, i32)> =
            deck.cards.iter().map(|c| (c.board, c.position)).collect();
        assert_eq!(
            positions,
            vec![("commander", 0), ("main", 0), ("main", 1), ("side", 0)],
            "each board numbers from 0 in upstream's own order"
        );
    }

    #[test]
    fn names_differing_only_in_punctuation_get_distinct_slugs() {
        let precons = build_precons(&fixture());
        let all = slugs(&precons);
        assert!(all.contains(&"a-box-of-rocks-sld"));
        assert!(
            all.contains(&"a-box-of-rocks-sld-2"),
            "the colliding drop takes a numeric suffix: {all:?}"
        );
    }

    /// Slugs are the URL identity, so the same document must always produce the same ones —
    /// including which of two colliding decks takes the `-2`. `AllPrintings.data` is a
    /// HashMap, so this only holds because the walk sorts the set codes.
    #[test]
    fn slug_assignment_is_stable_across_runs() {
        let first = build_precons(&fixture());
        for _ in 0..5 {
            let again = build_precons(&fixture());
            assert_eq!(slugs(&first), slugs(&again));
            assert_eq!(first, again);
        }
    }

    #[test]
    fn nameless_and_unresolvable_decks_are_dropped() {
        let precons = build_precons(&fixture());
        assert_eq!(precons.len(), 3, "{:?}", slugs(&precons));
        assert!(
            !precons.iter().any(|p| p.name == "Ghost Drop"),
            "a deck whose every card is unresolvable is not a deck"
        );
    }

    /// The whole document is third-party and every field is optional: a deck missing its
    /// type / date / set / counts still resolves, with documented defaults.
    #[test]
    fn tolerates_missing_optional_fields() {
        let json = serde_json::json!({
            "data": { "ABC": {
                "cards": [ { "uuid": "u-1", "identifiers": { "scryfallId": "sf-1" } } ],
                "decks": [ { "name": "Bare Deck", "mainBoard": [ { "uuid": "u-1" } ] } ]
            } }
        });
        let all: AllPrintings = serde_json::from_value(json).expect("parses");
        let precons = build_precons(&all);
        assert_eq!(precons.len(), 1);
        let deck = &precons[0];
        // No `code` on the deck: the set it's filed under, lowercased.
        assert_eq!(deck.set_code, "abc");
        assert_eq!(deck.slug, "bare-deck-abc");
        assert_eq!(deck.deck_type, UNTYPED);
        assert_eq!(deck.released_at, None);
        assert!(deck.product_ids.is_empty());
        // A missing count is one copy, not zero.
        assert_eq!(deck.cards[0].quantity, 1);
        assert!(!deck.cards[0].foil);
    }

    #[test]
    fn slugify_collapses_runs_and_trims() {
        assert_eq!(slugify("Turtle Power!"), "turtle-power");
        assert_eq!(
            slugify("  Heads I Win, Tails You Lose  "),
            "heads-i-win-tails-you-lose"
        );
        assert_eq!(
            slugify("Artist Series: Núria Bonet"),
            "artist-series-n-ria-bonet"
        );
        assert_eq!(slugify("!!!"), "");
        assert!(slugify(&"x".repeat(500)).len() <= MAX_SLUG);
    }

    /// A name that slugifies to nothing still gets a usable, unique URL rather than an
    /// empty one (which would collide with every other such deck).
    #[test]
    fn unnameable_decks_fall_back_to_the_set_code() {
        let mut used = HashMap::new();
        assert_eq!(unique_slug("!!!", "sld", &mut used), "sld");
        assert_eq!(unique_slug("???", "sld", &mut used), "sld-2");
        assert_eq!(unique_slug("!!!", "", &mut used), "deck");
    }
}
