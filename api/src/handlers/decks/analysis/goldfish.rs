//! **Goldfishing** (issue #596): shuffle up, draw an opening seven, take a London
//! mulligan, and step through draws — the thing every deckbuilder does a dozen times while
//! tuning a list.
//!
//! The engine is **stateless and seeded**. A hand is a pure function of
//! `(deck, seed, mulligans, what was bottomed, how many cards drawn)`, all of which ride in
//! the query string, so there is no session table, no expiry, and no cleanup — and a hand
//! that produced a surprising result can be handed to someone else as a URL and reproduce
//! exactly. That is also why the shuffle uses a **hand-rolled SplitMix64 + Fisher–Yates**
//! rather than the `rand` crate: a seed is part of the wire contract here, and `rand`'s
//! generators explicitly do not promise the same stream across versions, so a dependency
//! bump would silently invalidate every shared hand. SplitMix64 is ~10 lines, is specified
//! by its constants, and can be reimplemented in any client that wants to predict a draw.
//!
//! London mulligan, exactly: each mulligan reshuffles (a fresh shuffle derived from the
//! seed and the mulligan count), you always draw a full opening hand, and you then put
//! `mulligans` cards on the **bottom** of the library — which is where they go, so a long
//! enough draw step really can reach them again.

use serde::{Deserialize, Serialize};

use crate::entities::card;
use crate::error::AppError;
use crate::handlers::shared::CardResponse;

use super::stats::{default_library_section_ids, parse_section_ids};
use super::{AnalysisEntry, DeckAnalysisInput};

/// Most mulligans a request may claim. A mulligan to zero is seven; the cap is generous
/// and only exists so a request can't ask for an unbounded number of reshuffles.
const MAX_MULLIGANS: u32 = 20;
/// Largest opening hand a request may ask for. Seven is the game; the rest is for the
/// formats and effects that open on more.
const MAX_OPENING: u32 = 40;
/// Most cards a single request may draw past the opening hand — a whole long game, and a
/// bound on the response size.
const MAX_DRAWS: u32 = 500;
/// Largest library this will shuffle. A deck row's counts are caller-controlled (up to a
/// million per finish, and a deck has no cap on rows), and the shuffle materialises one slot
/// per **copy** — so without this a single `GET` could ask the server to build and
/// Fisher–Yates a multi-gigabyte vector to deal seven cards. Far above any real deck: a
/// Commander deck is 100 and the largest cube anyone plays is a few thousand.
const MAX_LIBRARY: i64 = 20_000;

/// Query string of the goldfish read. Everything the hand depends on is here, so the same
/// URL always yields the same hand.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct GoldfishParams {
    /// Shuffle seed. Omit for a fresh random one — the response echoes it back so the hand
    /// can be replayed or shared.
    pub seed: Option<u32>,
    /// How many times the hand was mulliganed. Each one reshuffles and costs a card to the
    /// bottom (London).
    pub mulligans: Option<u32>,
    /// Comma-separated **external card ids** the player put on the bottom, at most one per
    /// mulligan. Each must be a card in the hand as drawn.
    pub bottom: Option<String>,
    /// Cards drawn after the opening hand (the draw step). Clamped to the library.
    pub draws: Option<u32>,
    /// Opening hand size (default 7).
    pub opening: Option<u32>,
    /// Comma-separated section ids to shuffle. Omit for the default library — everything
    /// that isn't a maybeboard, a command zone, or a sideboard.
    pub sections: Option<String>,
}

/// A goldfished hand: what you're holding, what you bottomed, and what's left.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct GoldfishHand {
    /// The seed this hand was shuffled with — echoed so a random one can be replayed.
    pub seed: u32,
    pub mulligans: u32,
    /// The opening hand size actually dealt (clamped to the library).
    pub opening: u32,
    /// Cards drawn past the opening hand (clamped to what the library had).
    pub draws: u32,
    /// Cards that still have to go to the bottom before the game starts — `mulligans`
    /// minus however many the request already named.
    pub to_bottom: u32,
    /// The hand, in the order the cards were drawn: the opening hand first (minus anything
    /// bottomed), then each draw-step card.
    pub hand: Vec<CardResponse>,
    /// What was put on the bottom, in the order the request named it.
    pub bottomed: Vec<CardResponse>,
    /// Cards still in the library.
    pub library_size: i64,
    /// The shuffled library's size before any cards were dealt.
    pub library_total: i64,
    /// The section ids that made up the library for this shuffle.
    pub section_ids: Vec<i32>,
}

// ---------- The shuffle ----------

/// SplitMix64 — Steele et al.'s mixing function, used here as the whole generator. Fixed
/// constants, no library, identical output everywhere: exactly what a seed that appears on
/// the wire needs.
fn split_mix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The generator state for one `(seed, mulligans)` pair. Mixing the mulligan count in is
/// what makes a mulligan a genuine reshuffle rather than the same order minus a card.
fn shuffle_state(seed: u32, mulligans: u32) -> u64 {
    (u64::from(seed) << 32) | u64::from(mulligans)
}

/// Fisher–Yates, seeded. The modulo below is biased by at most 1 part in 2^64/len — for a
/// library of a few hundred cards that is far below anything a shuffle could express, and
/// keeping it makes the algorithm one line to reimplement.
fn shuffle<T>(items: &mut [T], seed: u32, mulligans: u32) {
    let mut state = shuffle_state(seed, mulligans);
    // Warm the state once so a low seed doesn't start from a near-zero mix.
    let _ = split_mix64(&mut state);
    for index in (1..items.len()).rev() {
        let pick = (split_mix64(&mut state) % (index as u64 + 1)) as usize;
        items.swap(index, pick);
    }
}

// ---------- Dealing ----------

/// The library as one slot per copy, in the deck's own deterministic row order before the
/// shuffle. Each slot is an index into the loaded rows, so a drawn card can be turned back
/// into its full catalog payload.
fn library_slots(entries: &[AnalysisEntry], rows: &[usize]) -> Vec<usize> {
    let mut slots = Vec::new();
    for row in rows {
        for _ in 0..entries[*row].copies() {
            slots.push(*row);
        }
    }
    slots
}

/// Deal a hand. `models` are the deck's catalog rows, positionally aligned with
/// `input.entries`.
pub(crate) fn analyse_goldfish(
    input: &DeckAnalysisInput,
    models: &[card::Model],
    params: &GoldfishParams,
) -> Result<GoldfishHand, AppError> {
    let seed = params.seed.unwrap_or_else(rand::random::<u32>);
    let mulligans = params.mulligans.unwrap_or(0);
    if mulligans > MAX_MULLIGANS {
        return Err(AppError::Validation(format!(
            "mulligans must be at most {MAX_MULLIGANS}"
        )));
    }
    let opening = params.opening.unwrap_or(7);
    if opening > MAX_OPENING {
        return Err(AppError::Validation(format!(
            "opening hand must be at most {MAX_OPENING} cards"
        )));
    }
    let requested_draws = params.draws.unwrap_or(0);
    if requested_draws > MAX_DRAWS {
        return Err(AppError::Validation(format!(
            "draws must be at most {MAX_DRAWS}"
        )));
    }

    let section_ids = match params.sections.as_deref() {
        Some(raw) => parse_section_ids(raw)?,
        None => default_library_section_ids(&input.sections),
    };

    // Row indices, not references: a library slot has to map back to a catalog model, and
    // `models` is aligned with `input.entries`.
    let rows = input.row_indices_in_sections(&section_ids);
    // Count before building: `library_slots` expands one slot per copy, so the refusal has
    // to happen while the size is still just a sum.
    let total: i64 = rows.iter().map(|row| input.entries[*row].copies()).sum();
    if total > MAX_LIBRARY {
        return Err(AppError::Validation(format!(
            "this deck holds {total} cards in the selected sections — at most {MAX_LIBRARY} \
             can be shuffled into a hand"
        )));
    }
    let mut library = library_slots(&input.entries, &rows);
    let library_total = library.len() as i64;
    shuffle(&mut library, seed, mulligans);

    let dealt = (opening as usize).min(library.len());
    let mut hand: Vec<usize> = library[..dealt].to_vec();
    let mut rest: Vec<usize> = library[dealt..].to_vec();

    // Bottoming: each named card leaves the hand and joins the back of the library, in the
    // order the request named it. An id that isn't in the hand is the caller's mistake, not
    // a silently smaller hand.
    let bottom_ids: Vec<&str> = params
        .bottom
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    if bottom_ids.len() > mulligans as usize {
        return Err(AppError::Validation(format!(
            "at most {mulligans} card(s) may be put on the bottom after {mulligans} mulligan(s)"
        )));
    }
    let mut bottomed: Vec<usize> = Vec::with_capacity(bottom_ids.len());
    for id in &bottom_ids {
        let position = hand
            .iter()
            .position(|slot| input.entries[*slot].facts.id == *id)
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "card {id} is not in the hand and can't be bottomed"
                ))
            })?;
        bottomed.push(hand.remove(position));
    }
    rest.extend(bottomed.iter().copied());

    let draws = (requested_draws as usize).min(rest.len());
    hand.extend(rest.drain(..draws));

    let payload = |slots: &[usize]| -> Vec<CardResponse> {
        slots
            .iter()
            .map(|slot| CardResponse::from(models[*slot].clone()))
            .collect()
    };

    Ok(GoldfishHand {
        seed,
        mulligans,
        opening: dealt as u32,
        draws: draws as u32,
        to_bottom: mulligans.saturating_sub(bottom_ids.len() as u32),
        hand: payload(&hand),
        bottomed: payload(&bottomed),
        library_size: rest.len() as i64,
        library_total,
        section_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shuffle_is_deterministic_and_a_permutation() {
        let mut a: Vec<usize> = (0..60).collect();
        let mut b: Vec<usize> = (0..60).collect();
        shuffle(&mut a, 1234, 0);
        shuffle(&mut b, 1234, 0);
        assert_eq!(a, b, "the same seed deals the same order");

        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..60).collect::<Vec<_>>(),
            "nothing is lost or duplicated"
        );
        assert_ne!(a, (0..60).collect::<Vec<_>>(), "and it actually shuffled");
    }

    #[test]
    fn a_mulligan_is_a_real_reshuffle() {
        let mut kept: Vec<usize> = (0..60).collect();
        let mut mulliganed: Vec<usize> = (0..60).collect();
        shuffle(&mut kept, 99, 0);
        shuffle(&mut mulliganed, 99, 1);
        assert_ne!(
            &kept[..7],
            &mulliganed[..7],
            "the same seed one mulligan later is a different hand"
        );
    }

    #[test]
    fn different_seeds_deal_different_hands() {
        let mut first: Vec<usize> = (0..60).collect();
        let mut second: Vec<usize> = (0..60).collect();
        shuffle(&mut first, 1, 0);
        shuffle(&mut second, 2, 0);
        assert_ne!(first, second);
    }

    #[test]
    fn a_one_card_library_needs_no_swaps() {
        let mut one = vec![7usize];
        shuffle(&mut one, 42, 0);
        assert_eq!(one, vec![7]);
        let mut none: Vec<usize> = Vec::new();
        shuffle(&mut none, 42, 0);
        assert!(none.is_empty());
    }
}
