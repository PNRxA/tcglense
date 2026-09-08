//! **Cards you already own that this deck could play** (issue #684) — EDHREC's core loop,
//! "here's what goes in a deck with this commander", minus the external API and scoped to
//! the caller's own collection: legal in the deck's format, inside its colour identity, not
//! already in it, ranked by popularity and grouped by the role each card fills.
//!
//! **What "recommended" means here, honestly.** The only popularity signal in the catalog is
//! `cards.edhrec_rank`, Scryfall's copy of EDHREC's **global** rank — how often a card is
//! played across every Commander deck, not how well it fits *this* commander. EDHREC's
//! per-commander tables have no bulk export and would be a scrape (off the table for a
//! self-host), so this read makes the narrower claim it can stand behind: *popular cards in
//! your colours that you own*, most popular first, and every response says so in its
//! `caveats`. It is a list to browse with the deck's own role counts beside it ("you have
//! six draw spells; here are the fourteen you own"), not a synergy engine.
//!
//! **Three borrowed answers, never a fourth rule.** Which cards are the commander is
//! [`super::rules::deck_zone`]'s answer and whether that zone *leads* the deck is
//! [`super::rules::format_leads_with_command_zone`]'s — the same pair the deck list's facets
//! and the mana base borrow — so the colour identity filtered by is the command zone's when it
//! leads and holds a card, and the union over the deck proper (sideboard out, like the facets)
//! otherwise; a deck with nothing to read a colour off filters by none and says so. Format
//! legality is [`super::legality::status_of`]'s reading of the card's own legality object under
//! [`super::formats::normalize_format_key`], so a deck in an untracked format (`Cube`, a blank
//! field) applies no legality filter rather than guessing. And the role each card fills is
//! [`super::roles::roles_of`], the grammar the roles panel counts the deck with, so "you have
//! N, you own M more" compares like with like.
//!
//! **Bounded two ways, because a collection's size is the user's.** The candidate scan reads
//! the collection through one narrow query (identity, colours, legality, rank — never the
//! wide catalog row) and folds it by gameplay identity, so a card held in four printings is
//! one candidate holding four copies. Only the [`SCAN_CAP`] most popular survivors are then
//! loaded in full for the role grammar and the wire; `candidate_count` stays exact while
//! `scanned_count` says how many were classified, and every per-role list is capped like the
//! roles read's own. The whole answer is memoised in the analytics cache under the holdings
//! version, the price epoch and a fingerprint of the deck's rows (`super::read`), because a
//! 30,000-card collection is a real scan and nothing in it changes between the user's own
//! edits and the daily price capture.
//!
//! **Not mirrored publicly.** Every other analysis read has a `/api/u/{handle}/…` twin; this
//! one reads the *caller's* collection, so a public mirror would leak per-user state.

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;

use crate::entities::card;
use crate::entities::collection_item;
use crate::entities::prelude::{Card, CollectionItem};
use crate::error::AppError;
use crate::handlers::decks::DeckCommanderResponse;
use crate::handlers::decks::facets::order_wubrg;
use crate::handlers::decks::needed::identity_key;
use crate::handlers::shared::CardResponse;
use crate::handlers::shared::dto::{parse_legalities, split_csv};
use crate::state::AppState;

use super::legality::{LegalityStatus, status_of};
use super::roles::{DeckRole, ROLES, roles_of};
use super::rules::{DeckZone, deck_zone, format_leads_with_command_zone};
use super::{CardFacts, DeckAnalysisInput, fold_by_name};

/// The most popular candidates loaded in full and classified. A collection's size is the
/// user's, so the classification (which needs each card's rules text) is bounded here;
/// `candidate_count` stays exact.
pub(crate) const SCAN_CAP: usize = 500;

/// Candidates named in the overall `top` list.
const MAX_LISTED_TOP: usize = 24;

/// Candidates named per role. Every list is **ids into one shared pool** (`cards`), so a
/// card filling three roles is serialised once — a full `CardResponse` duplicated across
/// nine lists was the body that broke the analytics cache's memory bound.
const MAX_LISTED_PER_ROLE: usize = 12;

/// Commanders named on the wire, for the "in Atraxa's colours" line. A real command zone
/// holds one or two; the colours still fold over every card in it.
const MAX_LISTED_COMMANDERS: usize = 4;

/// Card ids per `WHERE id IN (…)` lookup — the pricing read's bound.
const RESOLVE_CHUNK: usize = 900;

// ---------- Wire types ----------

/// One owned card the deck could play.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckSuggestionCard {
    /// One printing the caller owns — the lowest catalog id among those held, so the same
    /// collection answers byte-identically across requests.
    pub card: CardResponse,
    /// EDHREC's global popularity rank (1 = most played). The sort key.
    pub edhrec_rank: i32,
    /// Copies owned across every printing (regular + foil).
    pub owned: i64,
    /// The roles the card fills, in the roles read's order — empty for a card the grammar
    /// can't place.
    pub roles: Vec<DeckRole>,
}

/// What the collection could add to the deck in one role, beside what the deck holds.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckSuggestionRole {
    pub role: DeckRole,
    pub label: String,
    /// What the role counts — the roles read's own wording.
    pub description: String,
    /// Distinct cards in the deck proper already filling this role (the roles read's
    /// `count`), so a client can say "you have 6" beside "you own 14 more".
    pub in_deck: i64,
    /// Scanned candidates filling this role.
    pub count: i64,
    /// Those candidates by external card id into `DeckSuggestions::cards`, most popular
    /// first, capped (`count` stays exact).
    pub card_ids: Vec<String>,
}

/// Everything `GET /api/decks/{game}/{deck_id}/suggestions` answers.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckSuggestions {
    /// The legality key the deck's format normalised to and was filtered by, or `null` when
    /// the format isn't a tracked one — then no legality filter applied.
    pub format_key: Option<String>,
    pub format_label: Option<String>,
    /// The colour identity candidates had to fit inside, WUBRG-ordered: the command zone's
    /// when it leads the deck and holds a card, else the union over the deck proper. `null`
    /// when the deck has no card to read a colour off — then no colour filter applied. An
    /// empty list is a colourless deck, which only colourless cards fit.
    pub color_identity: Option<Vec<String>>,
    /// The command-zone cards whose identity that is (at most four named — a real zone holds
    /// one or two; the colours still fold over every card in it); empty when the colours are
    /// a union.
    pub commanders: Vec<DeckCommanderResponse>,
    /// Owned cards (by gameplay identity) that passed every filter — exact.
    pub candidate_count: i64,
    /// How many of those, most popular first, were loaded and classified. Equal to
    /// `candidate_count` unless it exceeded the scan cap.
    pub scanned_count: i64,
    /// Every card `top` or a role names, most popular first, each **once** — the pool the
    /// id lists below index into, so a card filling three roles rides the wire one time.
    pub cards: Vec<DeckSuggestionCard>,
    /// The most popular candidates overall, by external card id into `cards`, capped.
    pub top: Vec<String>,
    /// Every role, in the roles read's order, whether or not any candidate fills it.
    pub roles: Vec<DeckSuggestionRole>,
    /// Scanned candidates filling no role — most creatures and every land — so the role
    /// lists are never mistaken for a partition of the candidates.
    pub unclassified_count: i64,
    /// What the ranking is and isn't. Never empty.
    pub caveats: Vec<String>,
}

// ---------- The fold ----------

/// The colours a deck plays in, by the facets' rule over a loaded deck: the command zone's
/// when the format leads with one and it holds a card, else the union over the deck proper
/// (maybeboards out by their column, the sideboard out by its name). `None` when there was
/// nothing to judge. Also names the command-zone cards, when they are what decided it.
pub(crate) fn deck_colour_identity(
    format: Option<&str>,
    input: &DeckAnalysisInput,
) -> (Option<Vec<String>>, Vec<DeckCommanderResponse>) {
    let zone_by_section: HashMap<i32, DeckZone> = input
        .sections
        .iter()
        .filter(|section| !section.is_maybeboard)
        .map(|section| (section.id, deck_zone(&section.name)))
        .collect();
    let zone_of = |entry: &super::AnalysisEntry| zone_by_section.get(&entry.section_id).copied();

    if format_leads_with_command_zone(format) {
        let commanders: Vec<&super::AnalysisEntry> = input
            .entries
            .iter()
            .filter(|entry| entry.copies() > 0 && zone_of(entry) == Some(DeckZone::Command))
            .collect();
        if !commanders.is_empty() {
            let letters: BTreeSet<String> = commanders
                .iter()
                .flat_map(|entry| entry.facts.color_identity.iter().cloned())
                .collect();
            // By name, the way the facets list them: a second printing of the same legend in
            // the zone is a copy-limit matter, not a second commander.
            let mut named: Vec<DeckCommanderResponse> = Vec::new();
            for fold in fold_by_name(&commanders) {
                if named.len() >= MAX_LISTED_COMMANDERS {
                    break;
                }
                named.push(DeckCommanderResponse {
                    card_id: fold.card_id,
                    name: fold.facts.name.clone(),
                });
            }
            named.sort_by(|a, b| a.name.cmp(&b.name));
            return (Some(order_wubrg(&letters)), named);
        }
    }

    let mut letters: BTreeSet<String> = BTreeSet::new();
    let mut any = false;
    for entry in input.entries.iter().filter(|entry| entry.copies() > 0) {
        match zone_of(entry) {
            Some(DeckZone::Main) | Some(DeckZone::Command) => {
                any = true;
                letters.extend(entry.facts.color_identity.iter().cloned());
            }
            _ => {}
        }
    }
    (any.then(|| order_wubrg(&letters)), Vec::new())
}

/// Whether a card may sit in the 99 (or the 60) of a deck in `format_key`: `legal`, or
/// `restricted` where that means "one copy" rather than Pauper Commander's "commander
/// only". A card with no legality data, or none for this format, is **not** vouched for.
fn playable_in(facts: &CardFacts, format_key: &str) -> bool {
    match status_of(facts, format_key) {
        Some(LegalityStatus::Legal) => true,
        Some(LegalityStatus::Restricted) => format_key != "paupercommander",
        _ => false,
    }
}

/// One owned gameplay card as the narrow scan sees it, before the wide row is loaded.
struct OwnedCandidate {
    /// Lowest catalog id among the held printings — the one loaded for the wire.
    card_id: i32,
    /// `None` until a ranked printing is seen; a card that never gets one is dropped.
    edhrec_rank: Option<i32>,
    owned: i64,
    color_identity: Vec<String>,
    legalities: Option<String>,
}

/// The narrow scan of the caller's collection, folded by gameplay identity, most popular
/// first. A card needs a rank on **some** held printing to be a candidate — an unranked card
/// has no popularity claim to make — but every held printing counts towards `owned`, since a
/// reprint imported before the rank column existed is still a copy of the same card. The
/// rank, colours and legality object are oracle-level facts, so the first ranked printing read
/// speaks for the card. No SQL ordering: the fold below has to re-sort after merging
/// printings, so asking the database to sort the whole join first was dead work.
async fn owned_candidates(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<Vec<(String, OwnedCandidate)>, AppError> {
    let rows: Vec<OwnedRow> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(card::Column::OracleId)
        .column(card::Column::Name)
        .column(card::Column::ColorIdentity)
        .column(card::Column::Legalities)
        .column(card::Column::EdhrecRank)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .inner_join(Card)
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .into_tuple()
        .all(&state.db)
        .await?;

    Ok(fold_owned_rows(rows))
}

/// One row of the narrow scan: `(card id, oracle id, name, colour identity CSV, legality
/// JSON, EDHREC rank, regular copies, foil copies)`.
type OwnedRow = (
    i32,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<i32>,
    i32,
    i32,
);

/// Fold the scan's rows by gameplay identity, most popular first (see [`owned_candidates`]).
/// Pure, so the cross-printing rules can be pinned without a database: copies sum across
/// every held printing, the rank is the lowest any printing carries, the printing kept for
/// the wire is the lowest catalog id, and an identity no held printing ranks is dropped.
fn fold_owned_rows(rows: Vec<OwnedRow>) -> Vec<(String, OwnedCandidate)> {
    let mut order: Vec<String> = Vec::new();
    let mut folded: HashMap<String, OwnedCandidate> = HashMap::new();
    for (card_id, oracle_id, name, color_identity, legalities, rank, quantity, foil) in rows {
        let copies = i64::from(quantity).saturating_add(i64::from(foil)).max(0);
        if copies == 0 {
            continue;
        }
        let key = identity_key(oracle_id.as_deref(), &name);
        match folded.get_mut(&key) {
            Some(existing) => {
                existing.owned = existing.owned.saturating_add(copies);
                existing.edhrec_rank = match (existing.edhrec_rank, rank) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                if card_id < existing.card_id {
                    existing.card_id = card_id;
                }
            }
            None => {
                order.push(key.clone());
                folded.insert(
                    key,
                    OwnedCandidate {
                        card_id,
                        edhrec_rank: rank,
                        owned: copies,
                        color_identity: split_csv(color_identity),
                        legalities,
                    },
                );
            }
        }
    }
    let mut candidates: Vec<(String, OwnedCandidate)> = order
        .into_iter()
        .filter_map(|key| folded.remove(&key).map(|candidate| (key, candidate)))
        .filter(|(_, candidate)| candidate.edhrec_rank.is_some())
        .collect();
    // Most popular first, then by id, so the window the scan cap cuts is exactly "the most
    // popular" and the same collection answers identically across requests.
    candidates.sort_by_key(|(_, c)| (c.edhrec_rank, c.card_id));
    candidates
}

/// The caveats every response carries: what the rank is, and what the filters didn't check.
fn caveats(
    format_key: Option<&str>,
    color_identity: Option<&[String]>,
    candidate_count: i64,
    scanned_count: i64,
) -> Vec<String> {
    let mut caveats = vec![
        "Ranked by EDHREC's global popularity — how often a card is played across every \
         Commander deck — not by synergy with this deck's commander or plan. Cards with no \
         rank aren't listed."
            .to_string(),
    ];
    match format_key {
        Some(_) => caveats.push(
            "Only cards the catalog marks legal in the deck's format are listed; a card whose \
             legality isn't known is left out rather than guessed."
                .to_string(),
        ),
        None => caveats.push(
            "The deck's format isn't one legality is tracked for, so no legality filter was \
             applied."
                .to_string(),
        ),
    }
    if color_identity.is_none() {
        caveats.push(
            "The deck has no card to read a colour identity off yet, so no colour filter was \
             applied."
                .to_string(),
        );
    }
    if scanned_count < candidate_count {
        caveats.push(format!(
            "Only the {scanned_count} most popular of the {candidate_count} matching cards you \
             own were classified by role."
        ));
    }
    caveats
}

/// Everything the deck contributes to the answer, computed once from the loaded deck so
/// the handler can fingerprint it for the cache and the fold can read it.
pub(crate) struct DeckSide {
    pub format_key: Option<&'static str>,
    pub color_identity: Option<Vec<String>>,
    pub commanders: Vec<DeckCommanderResponse>,
    /// Gameplay identities already in the deck, maybeboards included — a card the deck is
    /// already considering isn't a suggestion.
    pub in_deck: HashSet<String>,
    /// The deck proper's distinct cards per role — the roles read's `count`.
    pub in_deck_by_role: HashMap<DeckRole, i64>,
}

/// Read the deck's side of the question. `models` are the catalog rows behind
/// `input.entries`, in the same order (what carries the `oracle_id` the identity fold keys on).
pub(crate) fn deck_side(
    format: Option<&str>,
    input: &DeckAnalysisInput,
    models: &[card::Model],
) -> DeckSide {
    let format_key = super::formats::normalize_format_key(format);
    let (color_identity, commanders) = deck_colour_identity(format, input);
    let in_deck: HashSet<String> = input
        .entries
        .iter()
        .zip(models)
        .filter(|(entry, _)| entry.copies() > 0)
        .map(|(_, model)| identity_key(model.oracle_id.as_deref(), &model.name))
        .collect();
    let roles = super::roles::analyse_roles(input);
    let in_deck_by_role = roles
        .roles
        .iter()
        .map(|group| (group.role, group.count))
        .collect();
    DeckSide {
        format_key,
        color_identity,
        commanders,
        in_deck,
        in_deck_by_role,
    }
}

/// Answer the question for a loaded deck against the caller's collection.
pub(crate) async fn analyse_suggestions(
    state: &AppState,
    user_id: i32,
    game: &str,
    deck: DeckSide,
) -> Result<DeckSuggestions, AppError> {
    let owned = owned_candidates(state, user_id, game).await?;

    // The filters, on the narrow rows: not in the deck, inside its colours, legal in its
    // format. A candidate's legality object is parsed here rather than on the wide row so
    // that a collection full of Standard-illegal cards costs a parse each, never a load.
    let survivors: Vec<OwnedCandidate> = owned
        .into_iter()
        .filter(|(key, _)| !deck.in_deck.contains(key))
        .filter(|(_, candidate)| match &deck.color_identity {
            Some(identity) => candidate
                .color_identity
                .iter()
                .all(|colour| identity.contains(colour)),
            None => true,
        })
        .filter(|(_, candidate)| match deck.format_key {
            Some(key) => {
                // A throwaway `CardFacts` would drag every field along; the per-card check
                // only reads the legality object, so hand it that alone.
                let facts = CardFacts {
                    legalities: parse_legalities(candidate.legalities.as_deref()),
                    ..CardFacts::empty()
                };
                playable_in(&facts, key)
            }
            None => true,
        })
        .map(|(_, candidate)| candidate)
        .collect();
    let candidate_count = survivors.len() as i64;

    // The most popular survivors, loaded in full for the role grammar and the wire.
    let scanned: Vec<OwnedCandidate> = survivors.into_iter().take(SCAN_CAP).collect();
    let mut models: HashMap<i32, card::Model> = HashMap::new();
    let wanted: Vec<i32> = scanned.iter().map(|c| c.card_id).collect();
    for chunk in wanted.chunks(RESOLVE_CHUNK) {
        let found = Card::find()
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for model in found {
            models.insert(model.id, model);
        }
    }

    let mut classified: Vec<DeckSuggestionCard> = Vec::with_capacity(scanned.len());
    for candidate in scanned {
        // A row gone between the two queries (a re-import mid-request) is skipped, as every
        // deck reader skips a card whose catalog row is gone — and is not counted as scanned.
        let Some(model) = models.remove(&candidate.card_id) else {
            continue;
        };
        let Some(edhrec_rank) = candidate.edhrec_rank else {
            continue; // unreachable: the scan keeps only ranked cards
        };
        let roles = roles_of(&CardFacts::from(&model));
        classified.push(DeckSuggestionCard {
            card: CardResponse::from(model),
            edhrec_rank,
            owned: candidate.owned,
            roles,
        });
    }
    let scanned_count = classified.len() as i64;

    // The id lists, and the one pool they index into: a card named by `top` and by three
    // roles is serialised once. `classified` is rank-ordered, so every list is too.
    let top: Vec<String> = classified
        .iter()
        .take(MAX_LISTED_TOP)
        .map(|card| card.card.id.clone())
        .collect();
    let roles: Vec<DeckSuggestionRole> = ROLES
        .iter()
        .map(|(role, label, description)| {
            let matched: Vec<&DeckSuggestionCard> = classified
                .iter()
                .filter(|card| card.roles.contains(role))
                .collect();
            DeckSuggestionRole {
                role: *role,
                label: (*label).to_string(),
                description: (*description).to_string(),
                in_deck: deck.in_deck_by_role.get(role).copied().unwrap_or(0),
                count: matched.len() as i64,
                card_ids: matched
                    .iter()
                    .take(MAX_LISTED_PER_ROLE)
                    .map(|card| card.card.id.clone())
                    .collect(),
            }
        })
        .collect();
    let unclassified_count = classified
        .iter()
        .filter(|card| card.roles.is_empty())
        .count() as i64;
    let named: HashSet<&str> = top
        .iter()
        .chain(roles.iter().flat_map(|group| group.card_ids.iter()))
        .map(String::as_str)
        .collect();
    let cards: Vec<DeckSuggestionCard> = classified
        .iter()
        .filter(|card| named.contains(card.card.id.as_str()))
        .cloned()
        .collect();

    let caveats = caveats(
        deck.format_key,
        deck.color_identity.as_deref(),
        candidate_count,
        scanned_count,
    );
    Ok(DeckSuggestions {
        format_label: deck.format_key.map(super::formats::format_label),
        format_key: deck.format_key.map(str::to_string),
        color_identity: deck.color_identity,
        commanders: deck.commanders,
        candidate_count,
        scanned_count,
        cards,
        top,
        roles,
        unclassified_count,
        caveats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{deck, entry, section};

    const MAIN: i32 = 1;
    const COMMAND: i32 = 2;
    const SIDE: i32 = 3;
    const MAYBE: i32 = 4;

    fn strs(letters: &[&str]) -> Vec<String> {
        letters.iter().map(|l| (*l).to_string()).collect()
    }

    fn sections() -> Vec<crate::handlers::decks::DeckSectionResponse> {
        vec![
            section(MAIN, "Main", false),
            section(COMMAND, "Commander", false),
            section(SIDE, "Sideboard", false),
            section(MAYBE, "Maybeboard", true),
        ]
    }

    /// A Commander deck is its commander's colours, even while the 99 plays fewer of them —
    /// and the commanders are named.
    #[test]
    fn a_led_deck_takes_its_command_zones_identity() {
        let input = deck(
            sections(),
            vec![
                entry("atraxa", "Atraxa, Praetors' Voice", COMMAND, 1, 0).colors("W,U,B,G"),
                entry("bolt", "Lightning Bolt", MAIN, 1, 0).colors("R"),
                entry("sol", "Sol Ring", MAIN, 1, 0),
            ],
        );
        let (identity, commanders) = deck_colour_identity(Some("Commander"), &input);
        assert_eq!(identity, Some(strs(&["W", "U", "B", "G"])));
        assert_eq!(commanders.len(), 1);
        assert_eq!(commanders[0].name, "Atraxa, Praetors' Voice");
    }

    /// In a format with no command zone the seeded `Commander` section is just part of the
    /// deck, so the colours are the union — and the sideboard never colours a deck.
    #[test]
    fn a_flat_deck_is_the_union_over_its_deck_proper_without_the_sideboard() {
        let input = deck(
            sections(),
            vec![
                entry("bear", "Grizzly Bears", MAIN, 4, 0).colors("G"),
                entry("parked", "Parked Legend", COMMAND, 1, 0).colors("W"),
                entry("side", "Sideboard Card", SIDE, 1, 0).colors("B"),
                entry("maybe", "Maybe Card", MAYBE, 1, 0).colors("R"),
            ],
        );
        let (identity, commanders) = deck_colour_identity(Some("Modern"), &input);
        assert_eq!(identity, Some(strs(&["W", "G"])));
        assert!(commanders.is_empty(), "a union names no commander");
    }

    /// A Commander deck whose zone is still empty falls back to the union, and a deck with
    /// no card at all (or only a sideboard) has nothing to say.
    #[test]
    fn an_empty_command_zone_falls_back_and_an_empty_deck_says_nothing() {
        let input = deck(
            sections(),
            vec![entry("bolt", "Lightning Bolt", MAIN, 1, 0).colors("R")],
        );
        let (identity, commanders) = deck_colour_identity(Some("Commander"), &input);
        assert_eq!(identity, Some(strs(&["R"])));
        assert!(commanders.is_empty());

        let empty = deck(sections(), vec![]);
        assert_eq!(deck_colour_identity(Some("Commander"), &empty).0, None);
        let side_only = deck(
            sections(),
            vec![entry("side", "Side", SIDE, 1, 0).colors("B")],
        );
        assert_eq!(deck_colour_identity(Some("Modern"), &side_only).0, None);
        // A zero-count row is on its way out and reads nothing.
        let gone = deck(
            sections(),
            vec![entry("gone", "Gone", COMMAND, 0, 0).colors("B")],
        );
        assert_eq!(deck_colour_identity(Some("Commander"), &gone).0, None);
    }

    /// A colourless deck is `Some([])`, which only colourless cards fit — not "no filter".
    #[test]
    fn a_colourless_deck_is_an_empty_identity_not_none() {
        let input = deck(sections(), vec![entry("sol", "Sol Ring", MAIN, 1, 0)]);
        assert_eq!(
            deck_colour_identity(Some("Modern"), &input).0,
            Some(Vec::new())
        );
    }

    #[test]
    fn playable_reads_legal_and_one_copy_restricted_but_not_pdhs_commander_only() {
        let legal = CardFacts::empty().legal("vintage", "legal");
        assert!(playable_in(&legal, "vintage"));
        let restricted = CardFacts::empty()
            .legal("vintage", "restricted")
            .legal("paupercommander", "restricted");
        assert!(playable_in(&restricted, "vintage"));
        assert!(
            !playable_in(&restricted, "paupercommander"),
            "restricted there means commander-only"
        );
        let banned = CardFacts::empty().legal("commander", "banned");
        assert!(!playable_in(&banned, "commander"));
        assert!(
            !playable_in(&CardFacts::empty(), "commander"),
            "no data is not vouched for"
        );
        assert!(
            !playable_in(&legal, "modern"),
            "no entry for the format is not vouched for"
        );
    }

    /// The deck side: what's in the deck (maybeboards included) is out of the suggestions,
    /// keyed by oracle id when there is one, else name.
    #[test]
    fn the_deck_side_keys_what_is_in_the_deck_by_identity() {
        use crate::test_support::card_model;
        let input = deck(
            sections(),
            vec![
                entry("ext-1", "Sol Ring", MAIN, 1, 0).oracle("{T}: Add {C}{C}."),
                entry("ext-2", "Maybe Card", MAYBE, 1, 0),
                entry("ext-3", "Gone", MAIN, 0, 0),
            ],
        );
        let models = vec![
            card::Model {
                oracle_id: Some("sol-oracle".to_string()),
                name: "Sol Ring".to_string(),
                ..card_model(1)
            },
            card::Model {
                name: "Maybe Card".to_string(),
                ..card_model(2)
            },
            card::Model {
                name: "Gone".to_string(),
                ..card_model(3)
            },
        ];
        let side = deck_side(Some("EDH"), &input, &models);
        assert_eq!(side.format_key, Some("commander"));
        assert!(side.in_deck.contains("o:sol-oracle"));
        assert!(
            side.in_deck.contains("n:Maybe Card"),
            "a maybeboard card is known"
        );
        assert!(
            !side.in_deck.contains("n:Gone"),
            "a zero-count row isn't in the deck"
        );
        assert_eq!(side.in_deck_by_role.get(&DeckRole::Ramp), Some(&1));
        assert_eq!(side.in_deck_by_role.get(&DeckRole::CardDraw), Some(&0));
    }

    /// The cross-printing fold: four printings of one card are one candidate holding every
    /// copy, ranked by the lowest rank any printing carries, shown as the lowest catalog id —
    /// and a card no held printing ranks is not a candidate at all.
    #[test]
    fn owned_rows_fold_by_identity_across_printings() {
        let row = |id: i32, oracle: &str, name: &str, rank: Option<i32>, q: i32, f: i32| {
            (
                id,
                Some(oracle.to_string()),
                name.to_string(),
                Some("G".to_string()),
                None,
                rank,
                q,
                f,
            )
        };
        let folded = fold_owned_rows(vec![
            row(30, "sol", "Sol Ring", Some(5), 1, 0),
            row(10, "sol", "Sol Ring", None, 2, 1), // a reprint imported before the rank column
            row(20, "sol", "Sol Ring", Some(3), 0, 1),
            row(40, "bear", "Grizzly Bears", None, 4, 0), // never ranked
            row(50, "bolt", "Lightning Bolt", Some(1), 0, 0), // owned nothing
            row(60, "cult", "Cultivate", Some(9), 1, 0),
        ]);
        let keys: Vec<&str> = folded.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            ["o:sol", "o:cult"],
            "most popular first; unranked and empty rows out"
        );
        let sol = &folded[0].1;
        assert_eq!(sol.owned, 5, "every held printing counts, ranked or not");
        assert_eq!(
            sol.edhrec_rank,
            Some(3),
            "the lowest rank any printing carries"
        );
        assert_eq!(
            sol.card_id, 10,
            "the lowest catalog id, so the answer is stable"
        );
    }

    /// A card with no oracle id folds by name — the shopping list's identity rule.
    #[test]
    fn owned_rows_without_an_oracle_id_fold_by_name() {
        let folded = fold_owned_rows(vec![
            (1, None, "Island".to_string(), None, None, Some(7), 3, 0),
            (2, None, "Island".to_string(), None, None, Some(7), 2, 0),
            (
                3,
                Some(String::new()),
                "Island".to_string(),
                None,
                None,
                None,
                1,
                0,
            ),
        ]);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].0, "n:Island");
        assert_eq!(folded[0].1.owned, 6);
    }

    #[test]
    fn the_caveats_always_name_the_rank_and_say_what_was_not_filtered() {
        let base = caveats(Some("commander"), Some(&["W".to_string()]), 10, 10);
        assert!(base[0].contains("global popularity"));
        assert_eq!(base.len(), 2);
        let untracked = caveats(None, None, 800, SCAN_CAP as i64);
        assert!(untracked.iter().any(|c| c.contains("no legality filter")));
        assert!(untracked.iter().any(|c| c.contains("no colour filter")));
        assert!(
            untracked
                .iter()
                .any(|c| c.contains("500 most popular of the 800"))
        );
    }
}
