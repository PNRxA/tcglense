//! What a deck *is* at a glance: the colour identity it plays in, and the card(s) leading
//! it — the two facets a deck list shows beside a name so a shelf of decks is scannable
//! without opening each one.
//!
//! Derived, never stored: a deck has no colour column, and its commander is simply
//! whatever sits in the command zone. Both are computed here for a **whole page of decks
//! at once** (three bounded queries for any number of decks), because the deck list is the
//! only caller that needs them in bulk and it must stay as cheap as the grouped
//! `card_count` aggregate beside it.
//!
//! Three couplings worth keeping straight — all of them "don't answer a question the
//! analysis modules already answer, differently":
//!
//! * The zone split is [`rules::deck_zone`](super::analysis::rules::deck_zone)'s, **not** a
//!   second list of section names — the same reason the goldfish shuffle derives its library
//!   from it. A list that called a section the command zone while the legality verdict called
//!   it the 99 would name a deck after a card the deck page says is just a creature.
//! * Whether that zone *leads* the deck is the format's call
//!   ([`rules::format_leads_with_command_zone`](super::analysis::rules::format_leads_with_command_zone)),
//!   because **every** new deck is seeded with a `Commander` section — a Modern deck with a
//!   card parked in it is a 60-card deck with a creature, exactly as `evaluate_deck_rules`
//!   treats it.
//! * Maybeboards are excluded by their **column** (issue #570), through the same
//!   [`maybeboard_section_ids`] sub-select `card_counts_by_deck` filters on.

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use crate::entities::prelude::{Card, DeckCard, DeckSection};
use crate::entities::{card, deck, deck_card, deck_section};
use crate::error::AppError;
use crate::handlers::shared::dto::split_csv;

use super::analysis::rules::{COLOUR_ORDER, DeckZone, deck_zone, format_leads_with_command_zone};
use super::maybeboard_section_ids;

/// How many command-zone cards a deck header will name. A real command zone holds one card,
/// or two (partners, or an Oathbreaker and its signature spell) — anything past that is a
/// deck the legality verdict is already calling illegal. The cap exists because these two
/// list endpoints are **not paginated**: without it, one deck with its whole 100 filed under
/// `Commander` would put 100 card entries in a response that also carries 999 other decks.
/// Colours are still folded from the *whole* command zone, so truncating names can't change
/// the pips.
const MAX_LIST_COMMANDERS: usize = 4;

/// How many same-zone sections of one deck the card scans below will look in. A deck has one
/// command zone and one sideboard; this is what keeps their **section ids out of an unbounded
/// `IN (…)` list**, since the per-`(user, game)` caps allow 1,000 decks x 200 sections and
/// 200,000 bind parameters exceed what SQLite (32,766) and Postgres (65,535) accept — the
/// whole list would 500 rather than degrade. Sections are taken in display order, so the
/// seeded `Commander` (position 0) is always among them.
const MAX_ZONE_SECTIONS_PER_DECK: usize = 4;

/// One card in a deck's command zone, as the deck list names it. The **external** card id
/// travels (like every card id on the wire), so a client can link to the printing; the deck
/// page is what serves the card in full.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "DeckCommander"))]
pub struct DeckCommanderResponse {
    /// The provider **external** card id (a Scryfall UUID for MTG).
    pub card_id: String,
    pub name: String,
}

/// A deck's derived facets, as [`deck_facets_by_deck`] computes them. `Default` is the
/// honest answer for a deck with no cards: no colours, no commander.
#[derive(Debug, Default)]
pub(crate) struct DeckFacets {
    /// Distinct colour-identity letters in WUBRG order, or `None` when there was nothing to
    /// judge. The two are **not** the same answer: `Some([])` is "this deck plays no colour",
    /// `None` is "this deck has no cards to read a colour off" — and the reader can't infer
    /// the difference from `card_count`, which counts the sideboard the colour fold ignores.
    pub color_identity: Option<Vec<String>>,
    /// The command zone's cards, by name (at most [`MAX_LIST_COMMANDERS`]); empty when the
    /// deck has no command zone or nothing in it — every non-Commander format, and a
    /// Commander deck still being built.
    pub commanders: Vec<DeckCommanderResponse>,
}

/// Fold the deck-level colour letters + commanders for a set of decks, keyed by deck id.
/// Every deck passed in gets an entry (an empty deck's is [`DeckFacets::default`]).
///
/// **A deck's colours are its command zone's when it has one, and the union over its deck
/// proper otherwise.** The two rules answer the same question for different formats: a
/// Commander deck *is* Mardu because its commander is, even in a build that plays no black
/// card yet, while a 60-card deck has no such declaration and can only be described by what's
/// in it. Reading the union in both cases would quietly re-colour a half-built Commander deck
/// as its 99 changed.
///
/// "Deck proper" here is the zone split's, not `card_count`'s: **a sideboard doesn't colour
/// the deck.** The 75 you register is not the deck you cast from, and a single off-colour
/// sideboard card turning a Boros list into a four-colour pip row would be the wrong answer
/// to "what is this deck". (`card_count` does count it — the two fields describe the deck at
/// different grains, and only this one claims to name its colours.) Maybeboards are outside
/// both. That mismatch is precisely why `color_identity` is an `Option`: a deck whose only
/// cards are in its sideboard has a non-zero `card_count` and *nothing to say* about colour,
/// which a reader could not otherwise tell from a deck that is genuinely colourless.
///
/// Cost is three queries regardless of how many decks are listed: the decks' non-maybeboard
/// sections, the command-zone cards, and — only for the decks that turned out to have none —
/// one `DISTINCT` colour-identity scan. The card scans select the two or three columns they
/// actually read and `DISTINCT` them, so a 100-card deck contributes at most a handful of
/// rows rather than a hundred, and both the section ids they bind
/// ([`MAX_ZONE_SECTIONS_PER_DECK`]) and the commanders they return
/// ([`MAX_LIST_COMMANDERS`]) are bounded per deck — nothing here scales with a *count* a
/// caller chose.
pub(crate) async fn deck_facets_by_deck(
    db: &DatabaseConnection,
    decks: &[deck::Model],
) -> Result<HashMap<i32, DeckFacets>, AppError> {
    let mut facets: HashMap<i32, DeckFacets> = HashMap::new();
    if decks.is_empty() {
        return Ok(facets);
    }
    let deck_ids: Vec<i32> = decks.iter().map(|d| d.id).collect();

    // ---- 1. Classify every section ----
    // The zone split is name-based (a section has no zone column), so it happens in Rust
    // through `deck_zone` rather than as a second copy of the name table in SQL. A section
    // flagged maybeboard is out of the deck entirely, whatever it's named.
    let led_by_command_zone: HashSet<i32> = decks
        .iter()
        .filter(|d| format_leads_with_command_zone(d.format.as_deref()))
        .map(|d| d.id)
        .collect();
    // Maybeboards are dropped in SQL (the column, as everywhere else); the zone split itself
    // has to come back to Rust, and display order makes the per-deck cap below deterministic.
    let sections: Vec<(i32, i32, String)> = DeckSection::find()
        .select_only()
        .column(deck_section::Column::Id)
        .column(deck_section::Column::DeckId)
        .column(deck_section::Column::Name)
        .filter(deck_section::Column::DeckId.is_in(deck_ids.iter().copied()))
        .filter(deck_section::Column::IsMaybeboard.eq(false))
        .order_by_asc(deck_section::Column::DeckId)
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .into_tuple()
        .all(db)
        .await?;
    let mut command_section_ids: Vec<i32> = Vec::new();
    let mut sideboard_section_ids: Vec<i32> = Vec::new();
    let mut per_deck: HashMap<i32, (usize, usize)> = HashMap::new();
    for (id, deck_id, name) in &sections {
        let seen = per_deck.entry(*deck_id).or_default();
        match deck_zone(name) {
            // In a format with no command zone, a `Commander` section is just part of the
            // deck — so it's neither named here nor excluded from the union below.
            DeckZone::Command
                if led_by_command_zone.contains(deck_id) && seen.0 < MAX_ZONE_SECTIONS_PER_DECK =>
            {
                seen.0 += 1;
                command_section_ids.push(*id);
            }
            DeckZone::Sideboard if seen.1 < MAX_ZONE_SECTIONS_PER_DECK => {
                seen.1 += 1;
                sideboard_section_ids.push(*id);
            }
            _ => {}
        }
    }

    let mut commanders: HashMap<i32, Vec<DeckCommanderResponse>> = HashMap::new();
    let mut letters: HashMap<i32, BTreeSet<String>> = HashMap::new();

    // ---- 2. The command-zone cards ----
    if !command_section_ids.is_empty() {
        let rows: Vec<(i32, String, String, Option<String>)> = DeckCard::find()
            .select_only()
            .column(deck_card::Column::DeckId)
            .column(card::Column::ExternalId)
            .column(card::Column::Name)
            .column(card::Column::ColorIdentity)
            .distinct()
            .inner_join(Card)
            .filter(deck_card::Column::DeckId.is_in(deck_ids.iter().copied()))
            .filter(deck_card::Column::SectionId.is_in(command_section_ids))
            .order_by_asc(card::Column::Name)
            .order_by_asc(card::Column::ExternalId)
            .into_tuple()
            .all(db)
            .await?;
        for (deck_id, card_id, name, identity) in rows {
            // Every command-zone card colours the deck, including the ones past the cap and
            // the second printing of one that's already named.
            letters
                .entry(deck_id)
                .or_default()
                .extend(split_csv(identity));
            let named = commanders.entry(deck_id).or_default();
            // By **name**, the way the colour-identity rule folds commanders: a second
            // printing of the same legend in the zone is a copy-limit matter for the legality
            // verdict, not a second commander to list.
            if named.len() < MAX_LIST_COMMANDERS && !named.iter().any(|c| c.name == name) {
                named.push(DeckCommanderResponse { card_id, name });
            }
        }
    }

    // ---- 3. The union fallback, for the decks with an empty command zone ----
    let commanderless: Vec<i32> = deck_ids
        .iter()
        .copied()
        .filter(|id| !commanders.contains_key(id))
        .collect();
    if !commanderless.is_empty() {
        let rows: Vec<(i32, Option<String>)> = DeckCard::find()
            .select_only()
            .column(deck_card::Column::DeckId)
            .column(card::Column::ColorIdentity)
            .distinct()
            .inner_join(Card)
            .filter(deck_card::Column::DeckId.is_in(commanderless.clone()))
            .filter(
                deck_card::Column::SectionId
                    .not_in_subquery(maybeboard_section_ids(commanderless.clone())),
            )
            .filter(deck_card::Column::SectionId.is_not_in(sideboard_section_ids))
            .into_tuple()
            .all(db)
            .await?;
        for (deck_id, identity) in rows {
            letters
                .entry(deck_id)
                .or_default()
                .extend(split_csv(identity));
        }
    }

    for deck_id in deck_ids {
        facets.insert(
            deck_id,
            DeckFacets {
                // A `letters` entry exists iff some card was read for this deck — even a card
                // with no colour at all — so its absence is exactly "nothing to judge".
                color_identity: letters.remove(&deck_id).as_ref().map(order_wubrg),
                commanders: commanders.remove(&deck_id).unwrap_or_default(),
            },
        );
    }
    Ok(facets)
}

/// The colour letters held, in canonical WUBRG order. Anything that isn't one of the five
/// (a stray letter in a stored CSV) is dropped rather than appended — the same stance
/// [`rules`](super::analysis::rules)' own identity label takes, so the two agree on what a
/// colour is.
fn order_wubrg(held: &BTreeSet<String>) -> Vec<String> {
    COLOUR_ORDER
        .iter()
        .copied()
        .filter(|colour| held.contains(*colour))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(letters: &[&str]) -> BTreeSet<String> {
        letters.iter().map(|l| (*l).to_string()).collect()
    }

    #[test]
    fn colours_come_back_in_wubrg_order_not_alphabetical() {
        assert_eq!(order_wubrg(&set(&["G", "W", "B"])), ["W", "B", "G"]);
        assert_eq!(
            order_wubrg(&set(&["R", "G", "W", "U", "B"])),
            ["W", "U", "B", "R", "G"]
        );
    }

    #[test]
    fn colourless_is_an_empty_list_and_junk_letters_are_dropped() {
        assert!(order_wubrg(&set(&[])).is_empty());
        // `C` is how a *card* row spells colourless; a deck says it by holding no colours,
        // so the letter never reaches the wire.
        assert!(order_wubrg(&set(&["C"])).is_empty());
        assert_eq!(order_wubrg(&set(&["U", "?"])), ["U"]);
    }

    /// Only a format whose rules define a command zone lets a `Commander` section name the
    /// deck — every new deck is seeded with one, so this is what keeps a Modern deck from
    /// being described by a creature someone parked there.
    #[test]
    fn only_command_zone_formats_are_led_by_their_commander_section() {
        for led in ["Commander", "commander", "EDH", "Brawl", "Oathbreaker"] {
            assert!(
                format_leads_with_command_zone(Some(led)),
                "{led} has a command zone"
            );
        }
        for flat in ["Modern", "standard", "Pauper", "Legacy", "Gladiator"] {
            assert!(
                !format_leads_with_command_zone(Some(flat)),
                "{flat} has no command zone"
            );
        }
        // No format set, or one the rules module has no profile for: it renders no verdict
        // about such a deck, so there's nothing for the list to contradict — take the owner
        // at their word.
        assert!(format_leads_with_command_zone(None));
        assert!(format_leads_with_command_zone(Some("Kitchen Table")));
    }
}
