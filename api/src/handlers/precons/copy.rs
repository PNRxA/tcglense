//! Copy a preconstructed deck into the caller's own decks.
//!
//! The bridge from the catalog surface back to the user's: "I bought this precon / I want to
//! tinker with this list" becomes a real deck they own, private and loose, which they can
//! then edit, analyse, share, or diff against their collection like any other.
//!
//! Structurally this is [`decks::copy`](crate::handlers::decks)'s twin — the source rows
//! already carry internal `cards.id`, so there is no resolution to do — and it writes through
//! that module's `insert_deck_with_cards` seam, inheriting its deck cap, its single
//! transaction and its chunked insert. The one thing that is this module's own is the
//! **board -> section mapping**, and it is load-bearing:
//!
//! * the command zone becomes a section named exactly `Commander` and the sideboard exactly
//!   `Sideboard`, because those spellings are what `decks::analysis::rules` reads a deck's
//!   zones off — file a commander anywhere else and the copy comes back "no commander";
//! * the mainboard is filed into the preset type buckets through
//!   `deck_import::categorize::preset_section`, the same table a deck import uses, so a
//!   copied precon arrives sorted into Creatures / Lands / Ramp rather than as one pile;
//! * a precon's per-row single finish folds back into the deck card's regular/foil pair — and
//!   because a board may list one printing in **both** finishes (two rows by design: the ingest
//!   keys on `(card, finish)`), the two rows must fold into **one** deck card. `deck_cards`
//!   carries a unique `(deck_id, card_id, section_id)`, so emitting them separately is not a
//!   duplicate tile — it's a failed insert, i.e. a 500 on every Jumpstart theme and bundle
//!   land pack. `push_folded` is what makes that impossible.
//!
//! Sections are created in a fixed display order (command zone, the type buckets in the
//! deck's own preset order, then the sideboard) and an empty one is never created, so a
//! 60-card precon with no commander gets no empty `Commander` section.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::auth::extractor::WritableUser;
use crate::auth::username::handle_of;
use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, PreconDeckCard};
use crate::entities::{card, precon_deck_card};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::decks::{
    DEFAULT_SECTIONS, DeckDetail, NewDeck, NewDeckCard, NewDeckSection, deck_detail,
    insert_deck_with_cards,
};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::load_precon;

/// Section a copied commander lands in — the spelling `rules::deck_zone` recognises.
const COMMANDER_SECTION: &str = "Commander";
/// Section a copied sideboard lands in — likewise the spelling the zone split reads.
const SIDEBOARD_SECTION: &str = "Sideboard";
/// Where mainboard cards go when their type line says nothing usable (or the card row is
/// gone). Named like an import's generic board rather than invented.
const MAINBOARD_SECTION: &str = "Mainboard";

/// Copy a preconstructed deck
///
/// `POST /api/decks/{game}/precons/{slug}/copy` -> duplicate a published preconstructed
/// decklist into the caller's own decks, returning the new deck's full detail. The copy is
/// private and loose (no folder), named after the precon, with its command zone, mainboard
/// (filed into the preset type sections) and sideboard as sections. `404` when the game or
/// precon is unknown; `422` when the caller is already at their per-game deck cap.
#[utoipa::path(
    post,
    path = "/api/decks/{game}/precons/{slug}/copy",
    tag = "Preconstructed decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
    ),
    responses(
        (status = 200, description = "The newly created deck's full detail (owned by the caller).", body = DeckDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game or preconstructed deck."),
        (status = 422, description = "The caller is already at their per-game deck cap."),
    ),
)]
pub async fn copy_precon_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, slug)): Path<(String, String)>,
) -> Result<Json<DeckDetail>, AppError> {
    require_game(&game)?;
    let precon = load_precon(&state, &game, &slug).await?;

    // The decklist joined to its cards: the type line is what files each mainboard card.
    let rows: Vec<(precon_deck_card::Model, Option<card::Model>)> = PreconDeckCard::find()
        .find_also_related(Card)
        .filter(precon_deck_card::Column::PreconDeckId.eq(precon.id))
        .order_by_asc(precon_deck_card::Column::Position)
        .order_by_asc(precon_deck_card::Column::Id)
        .all(&state.db)
        .await?;

    // A card whose catalog row is gone can't be put in a deck, so it's dropped here — the
    // same LEFT-join-then-skip tolerance the detail read applies. What the planner needs of
    // a card is only its type line (which bucket it files into).
    let planned: Vec<PlannedCard> = rows
        .iter()
        .filter_map(|(item, card)| {
            card.as_ref().map(|card| PlannedCard {
                row: item,
                type_line: card.type_line.as_deref(),
            })
        })
        .collect();
    let sections = plan_sections(&planned);
    if sections.is_empty() {
        // Every card of the precon has left the catalog: copying it would create an empty
        // deck rather than the list the page showed.
        return Err(AppError::Validation(
            "this preconstructed deck has no cards in the catalog to copy".to_string(),
        ));
    }

    let deck = insert_deck_with_cards(
        &state,
        user.id,
        NewDeck {
            game: precon.game.clone(),
            name: precon.name.clone(),
            description: Some(copy_description(&precon)),
            format: precon_format(&precon.deck_type),
        },
        sections,
    )
    .await?;

    Ok(Json(deck_detail(&state, &deck, handle_of(&user)).await?))
}

/// Add a card to a section, merging into the copy already there when the section holds that
/// printing — which happens whenever a board lists it in both finishes (a Jumpstart theme's
/// foil rare beside its non-foil copy, a bundle land pack's foil basics).
///
/// This is not cosmetic de-duplication: `deck_cards` is unique on
/// `(deck_id, card_id, section_id)`, so two entries for one printing make the whole copy's
/// `insert_many` fail — a 500, and no deck. Folding here is also what makes the deck a copy
/// *of* the precon: 3 regular + 1 foil is one card the deck holds four of.
fn push_folded(section: &mut Vec<NewDeckCard>, entry: NewDeckCard) {
    match section
        .iter_mut()
        .find(|card| card.card_id == entry.card_id)
    {
        Some(existing) => {
            existing.quantity += entry.quantity;
            existing.foil_quantity += entry.foil_quantity;
        }
        None => section.push(entry),
    }
}

/// One precon row about to be planned: the row itself plus the only thing about its card the
/// plan depends on. Narrow on purpose — it keeps the pure planner testable without
/// fabricating a whole catalog row.
struct PlannedCard<'a> {
    row: &'a precon_deck_card::Model,
    type_line: Option<&'a str>,
}

/// Group a precon's rows into the sections the copy is written with, in display order.
///
/// Mainboard buckets follow [`DEFAULT_SECTIONS`]' own order rather than first-seen order, so
/// two copies of different precons are laid out the same way and match a freshly created
/// deck. A bucket with no cards is omitted entirely.
fn plan_sections(rows: &[PlannedCard<'_>]) -> Vec<NewDeckSection> {
    let mut commander: Vec<NewDeckCard> = Vec::new();
    let mut sideboard: Vec<NewDeckCard> = Vec::new();
    // Mainboard buckets, keyed by section name and kept in `DEFAULT_SECTIONS` order below.
    let mut main: std::collections::HashMap<&'static str, Vec<NewDeckCard>> =
        std::collections::HashMap::new();

    for PlannedCard { row, type_line } in rows {
        // A precon row is one finish; a deck card carries both counts.
        let entry = NewDeckCard {
            card_id: row.card_id,
            quantity: if row.foil { 0 } else { row.quantity },
            foil_quantity: if row.foil { row.quantity } else { 0 },
        };
        match row.board.as_str() {
            b if b == PreconBoard::Commander.as_str() => push_folded(&mut commander, entry),
            b if b == PreconBoard::Side.as_str() => push_folded(&mut sideboard, entry),
            _ => {
                let bucket = crate::deck_import::categorize::preset_section(*type_line)
                    .unwrap_or(MAINBOARD_SECTION);
                push_folded(main.entry(bucket).or_default(), entry);
            }
        }
    }

    let mut sections = Vec::new();
    if !commander.is_empty() {
        sections.push(NewDeckSection {
            name: COMMANDER_SECTION.to_string(),
            is_maybeboard: false,
            cards: commander,
        });
    }
    for (name, _) in DEFAULT_SECTIONS {
        // The command zone is a default section too, and it was already emitted above from
        // the precon's own commander board — never from a type bucket.
        if *name == COMMANDER_SECTION {
            continue;
        }
        if let Some(cards) = main.remove(name) {
            sections.push(NewDeckSection {
                name: (*name).to_string(),
                is_maybeboard: false,
                cards,
            });
        }
    }
    // Anything filed into a bucket that isn't a preset (today only `Mainboard`), name-sorted
    // so the layout stays deterministic if the bucket table ever grows one.
    let mut leftovers: Vec<(&'static str, Vec<NewDeckCard>)> = main.into_iter().collect();
    leftovers.sort_by_key(|(name, _)| *name);
    for (name, cards) in leftovers {
        sections.push(NewDeckSection {
            name: name.to_string(),
            is_maybeboard: false,
            cards,
        });
    }
    if !sideboard.is_empty() {
        sections.push(NewDeckSection {
            name: SIDEBOARD_SECTION.to_string(),
            is_maybeboard: false,
            cards: sideboard,
        });
    }
    sections
}

/// The copy's description: what it is and where it came from, so a shelf of decks says which
/// ones were bought rather than built.
fn copy_description(precon: &crate::entities::precon_deck::Model) -> String {
    match &precon.released_at {
        Some(date) => format!(
            "{} from {} ({})",
            precon.deck_type,
            precon.set_code.to_uppercase(),
            date
        ),
        None => format!(
            "{} from {}",
            precon.deck_type,
            precon.set_code.to_uppercase()
        ),
    }
}

/// Guess the copy's format from upstream's deck type — only where the type *states* it.
///
/// Shared with the analysis mirror ([`super::analysis`]) rather than re-derived there: the
/// legality verdict and the bracket a precon *page* reports must be the ones the deck you copy
/// from it would report, and two derivations of "which format is this" is exactly how that
/// guarantee rots.
///
/// A "Commander Deck" is a Commander deck; a "Theme Deck" or a "Secret Lair Drop" says
/// nothing about a format, and a wrong guess is worse than none (the deck page would judge a
/// 30-card drop against Commander's rules and call it illegal). So everything else copies
/// with no format, exactly as a blank deck starts.
pub(super) fn precon_format(deck_type: &str) -> Option<String> {
    let lowered = deck_type.to_ascii_lowercase();
    if lowered.contains("commander") {
        Some("commander".to_string())
    } else if lowered.contains("brawl") {
        Some("brawl".to_string())
    } else if lowered.contains("oathbreaker") {
        Some("oathbreaker".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(board: PreconBoard, card_id: i32, quantity: i32, foil: bool) -> precon_deck_card::Model {
        precon_deck_card::Model {
            id: card_id,
            precon_deck_id: 1,
            card_id,
            board: board.as_str().to_string(),
            quantity,
            foil,
            position: 0,
        }
    }

    fn planned<'a>(rows: &'a [(precon_deck_card::Model, Option<&'a str>)]) -> Vec<PlannedCard<'a>> {
        rows.iter()
            .map(|(row, type_line)| PlannedCard {
                row,
                type_line: *type_line,
            })
            .collect()
    }

    /// The command zone and the sideboard take their exact rules-visible names, and the
    /// mainboard is filed into the preset buckets in `DEFAULT_SECTIONS` order.
    #[test]
    fn boards_map_to_the_rules_visible_sections() {
        let rows = vec![
            (
                row(PreconBoard::Commander, 1, 1, true),
                Some("Legendary Creature — Turtle"),
            ),
            (
                row(PreconBoard::Main, 2, 20, false),
                Some("Basic Land — Island"),
            ),
            (row(PreconBoard::Main, 3, 1, false), Some("Creature — Rat")),
            (row(PreconBoard::Side, 4, 2, false), Some("Instant")),
        ];
        let sections = plan_sections(&planned(&rows));
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Commander", "Creatures", "Lands", "Sideboard"]);
        assert!(
            sections.iter().all(|s| !s.is_maybeboard),
            "a precon's sideboard is a real sideboard, not a maybeboard"
        );
    }

    /// A precon row states one finish; the deck card it becomes carries the pair.
    #[test]
    fn a_single_finish_row_folds_into_the_deck_cards_two_counts() {
        let rows = vec![
            (
                row(PreconBoard::Commander, 1, 1, true),
                Some("Legendary Creature"),
            ),
            (row(PreconBoard::Main, 2, 3, false), Some("Creature")),
        ];
        let sections = plan_sections(&planned(&rows));
        let commander = &sections[0].cards[0];
        assert_eq!((commander.quantity, commander.foil_quantity), (0, 1));
        let creature = &sections[1].cards[0];
        assert_eq!((creature.quantity, creature.foil_quantity), (3, 0));
    }

    /// A printing listed in **both** finishes on one board is two precon rows by design — and
    /// must fold into ONE deck card, or the copy's insert violates `deck_cards`' unique
    /// `(deck, card, section)` key and the whole request 500s.
    #[test]
    fn a_printing_in_both_finishes_folds_into_one_deck_card() {
        let rows = vec![
            (row(PreconBoard::Main, 1, 3, false), Some("Creature")),
            // Same printing, foil — MTGJSON lists these as separate entries.
            (row(PreconBoard::Main, 1, 1, true), Some("Creature")),
            (row(PreconBoard::Main, 2, 1, false), Some("Creature")),
        ];
        let sections = plan_sections(&planned(&rows));
        assert_eq!(sections.len(), 1);
        let cards = &sections[0].cards;
        assert_eq!(cards.len(), 2, "one entry per printing, not per finish");
        let folded = cards
            .iter()
            .find(|c| c.card_id == 1)
            .expect("the mixed-finish printing");
        assert_eq!((folded.quantity, folded.foil_quantity), (3, 1));
    }

    /// The same fold applies to the command zone and the sideboard, which don't go through the
    /// type-bucket map at all.
    #[test]
    fn both_finishes_fold_on_every_board() {
        for board in [PreconBoard::Commander, PreconBoard::Side] {
            let rows = vec![
                (row(board, 7, 1, false), Some("Legendary Creature")),
                (row(board, 7, 1, true), Some("Legendary Creature")),
            ];
            let sections = plan_sections(&planned(&rows));
            assert_eq!(sections[0].cards.len(), 1, "{board:?}");
            assert_eq!(
                (
                    sections[0].cards[0].quantity,
                    sections[0].cards[0].foil_quantity
                ),
                (1, 1),
                "{board:?}"
            );
        }
    }

    /// A card with no usable type line still lands somewhere, and a precon with no command
    /// zone gets no empty `Commander` section.
    #[test]
    fn untyped_cards_fall_back_and_empty_sections_are_omitted() {
        let rows = vec![(row(PreconBoard::Main, 1, 1, false), None)];
        let sections = plan_sections(&planned(&rows));
        assert_eq!(
            sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["Mainboard"]
        );
    }

    /// Only a type that *states* a format sets one — a guess would have the deck page judge
    /// a Secret Lair drop against Commander's construction rules.
    #[test]
    fn only_format_stating_types_set_a_format() {
        assert_eq!(
            precon_format("Commander Deck").as_deref(),
            Some("commander")
        );
        assert_eq!(precon_format("Brawl Deck").as_deref(), Some("brawl"));
        assert_eq!(precon_format("Secret Lair Drop"), None);
        assert_eq!(precon_format("Theme Deck"), None);
        assert_eq!(precon_format("Jumpstart"), None);
    }

    /// The description says what the deck is and where it came from, dated when upstream
    /// dated it.
    #[test]
    fn copy_description_names_the_type_and_set() {
        let now = chrono::Utc::now();
        let mut precon = crate::entities::precon_deck::Model {
            id: 1,
            game: "mtg".to_string(),
            slug: "turtle-power-tmc".to_string(),
            name: "Turtle Power!".to_string(),
            set_code: "tmc".to_string(),
            deck_type: "Commander Deck".to_string(),
            released_at: Some("2026-03-06".to_string()),
            color_identity: Some("WUB".to_string()),
            card_count: 100,
            sideboard_count: 0,
            face_card_id: None,
            product_id: None,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(
            copy_description(&precon),
            "Commander Deck from TMC (2026-03-06)"
        );
        precon.released_at = None;
        assert_eq!(copy_description(&precon), "Commander Deck from TMC");
    }
}
