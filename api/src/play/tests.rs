//! Unit tests for the pure play engine: the reducer's ownership and bounds rules, the
//! log's phrasing (which must never name a card nobody was shown), and the per-viewer
//! views. Everything runs on a `RoomState` literal with a seeded [`PlayRng`] — no DB, no
//! socket, no clock.

use chrono::{DateTime, Utc};

use super::engine::{self, ActionError};
use super::rng::PlayRng;
use super::types::{
    Action, CardDef, CardFace, CardId, CardInstance, LogKind, MAX_CHAT, MAX_LOG, PeekKind, Phase,
    Placement, RoomState, RoomStatus, SeatId, SeatState, Zone,
};
use super::view;

const ALICE: SeatId = 10;
const BOB: SeatId = 11;
const CARA: SeatId = 12;
const NAMES: [&str; 6] = ["Alice", "Bob", "Cara", "Dan", "Eve", "Finn"];

fn now() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("a valid timestamp")
}

fn face(name: &str) -> CardFace {
    CardFace {
        name: name.to_string(),
        mana_cost: Some("{1}".to_string()),
        type_line: Some("Artifact".to_string()),
        oracle_text: None,
        power: None,
        toughness: None,
        loyalty: None,
    }
}

fn card_def(name: &str) -> CardDef {
    CardDef {
        card_id: Some(format!("ext-{name}")),
        game: "mtg".to_string(),
        name: name.to_string(),
        faces: vec![face(name)],
        back_image: false,
        has_image: true,
        colors: vec!["W".to_string()],
        cmc: Some(1.0),
        is_commander: false,
        is_token: false,
    }
}

fn double_faced(name: &str) -> CardDef {
    let mut def = card_def(name);
    def.faces.push(face(&format!("{name} // back")));
    def.back_image = true;
    def
}

fn deck_for(index: usize) -> Vec<CardDef> {
    let mut deck = Vec::new();
    let mut general = card_def(&format!("General {index}"));
    general.is_commander = true;
    deck.push(general);
    for n in 0..20 {
        deck.push(card_def(&format!("Card {index}-{n}")));
    }
    deck
}

fn seats(count: usize) -> Vec<SeatState> {
    (0..count)
        .map(|i| {
            engine::new_seat_state(
                10 + i as SeatId,
                i as i32,
                NAMES[i].to_string(),
                i == 0,
                deck_for(i),
                Some(format!("Deck {i}")),
                40,
            )
        })
        .collect()
}

fn lobby(count: usize) -> RoomState {
    engine::new_room_state("commander", 40, seats(count))
}

/// A started game with a deterministic RNG and a deterministic active seat (Alice).
fn started(count: usize) -> (RoomState, PlayRng) {
    let mut state = lobby(count);
    let mut rng = PlayRng::seeded(7);
    engine::start_game(&mut state, &mut rng, now()).expect("start");
    state.turn.active_seat = Some(ALICE);
    (state, rng)
}

fn seat_of(state: &RoomState, id: SeatId) -> &SeatState {
    state.seats.iter().find(|s| s.id == id).expect("seat")
}

fn card_of(state: &RoomState, id: CardId) -> &CardInstance {
    state.cards.get(&id).expect("card")
}

fn commander_of(state: &RoomState, seat: SeatId) -> CardId {
    state
        .cards
        .values()
        .find(|c| c.owner == seat && c.def.is_commander)
        .expect("a commander")
        .id
}

fn mv(card: CardId, zone: Zone) -> Action {
    Action::MoveCard {
        card,
        zone,
        placement: None,
        x: None,
        y: None,
        face_down: None,
    }
}

fn act(state: &mut RoomState, rng: &mut PlayRng, actor: SeatId, action: Action) -> engine::Outcome {
    engine::apply(state, actor, action, rng, now()).expect("action accepted")
}

fn fails(state: &mut RoomState, rng: &mut PlayRng, actor: SeatId, action: Action) -> ActionError {
    engine::apply(state, actor, action, rng, now()).expect_err("action refused")
}

fn last_log(state: &RoomState) -> String {
    state.log.last().expect("a log entry").text.clone()
}

/// Put a card from Alice's hand onto her battlefield and answer its id.
fn play_from_hand(state: &mut RoomState, rng: &mut PlayRng, seat: SeatId, at: usize) -> CardId {
    let id = seat_of(state, seat).hand[at];
    act(state, rng, seat, mv(id, Zone::Battlefield));
    id
}

// ---------- Construction ----------

#[test]
fn new_room_state_is_an_empty_lobby() {
    let state = lobby(2);
    assert_eq!(state.version, 0);
    assert_eq!(state.status, RoomStatus::Lobby);
    assert_eq!(state.turn.number, 0);
    assert_eq!(state.turn.active_seat, None);
    assert_eq!(state.turn.phase, Phase::Untap);
    assert!(state.log.is_empty());
    assert_eq!(state.next_log_id, 1);
    assert_eq!(state.winner, None);
    assert!(state.cards.is_empty());
}

#[test]
fn new_seat_state_starts_empty_at_the_starting_life() {
    let seat = engine::new_seat_state(
        4,
        1,
        "Bob".to_string(),
        false,
        deck_for(1),
        Some("Deck".to_string()),
        20,
    );
    assert_eq!(seat.life, 20);
    assert!(!seat.out);
    assert_eq!(seat.connections, 0);
    assert_eq!(seat.deck.len(), 21);
    for zone in [
        &seat.library,
        &seat.hand,
        &seat.battlefield,
        &seat.graveyard,
        &seat.exile,
        &seat.command,
    ] {
        assert!(zone.is_empty());
    }
}

// ---------- start_game ----------

#[test]
fn start_game_deals_seven_and_fills_the_library() {
    let (state, _) = started(2);
    assert_eq!(state.status, RoomStatus::Playing);
    assert_eq!(state.version, 1);
    assert_eq!(state.turn.number, 1);
    assert_eq!(state.turn.phase, Phase::Untap);
    for seat in &state.seats {
        assert_eq!(seat.hand.len(), 7, "{} hand", seat.name);
        assert_eq!(seat.library.len(), 13, "{} library", seat.name);
        assert_eq!(seat.command.len(), 1, "{} command zone", seat.name);
        for id in &seat.hand {
            assert_eq!(card_of(&state, *id).zone, Zone::Hand);
        }
        for id in &seat.library {
            assert_eq!(card_of(&state, *id).zone, Zone::Library);
        }
    }
    assert_eq!(state.cards.len(), 42);
}

#[test]
fn start_game_puts_commanders_in_the_command_zone() {
    let mut state = lobby(2);
    let mut rng = PlayRng::seeded(3);
    engine::start_game(&mut state, &mut rng, now()).expect("start");
    for seat in &state.seats {
        let id = seat.command[0];
        let card = card_of(&state, id);
        assert!(card.def.is_commander);
        assert_eq!(card.zone, Zone::Command);
        assert_eq!(card.owner, seat.id);
        assert_eq!(card.controller, seat.id);
        assert!((card.x - 0.5).abs() < f32::EPSILON);
        assert!((card.y - 0.5).abs() < f32::EPSILON);
        assert!(!card.tapped);
        assert_eq!(card.face_index, 0);
    }
}

#[test]
fn start_game_picks_a_starting_seat_among_the_seats() {
    let mut picks = std::collections::BTreeSet::new();
    for seed in 0..40u64 {
        let mut state = lobby(3);
        let mut rng = PlayRng::seeded(seed);
        let changes = engine::start_game(&mut state, &mut rng, now()).expect("start");
        let active = state.turn.active_seat.expect("an active seat");
        assert!(state.seats.iter().any(|s| s.id == active));
        assert_eq!(changes.cards.len(), state.cards.len());
        assert_eq!(changes.seats.len(), 3);
        picks.insert(active);
    }
    assert!(picks.len() > 1, "the starting seat should not be fixed");
}

#[test]
fn start_game_logs_who_goes_first_and_reports_the_new_entry() {
    let mut state = lobby(2);
    let mut rng = PlayRng::seeded(11);
    let changes = engine::start_game(&mut state, &mut rng, now()).expect("start");
    assert_eq!(changes.log_from, 0);
    assert_eq!(state.log.len(), 1);
    assert_eq!(state.log[0].kind, LogKind::System);
    assert!(state.log[0].text.starts_with("Game started — "));
}

#[test]
fn start_game_refuses_a_second_start() {
    let (mut state, mut rng) = started(2);
    let err = engine::start_game(&mut state, &mut rng, now()).expect_err("refused");
    assert_eq!(err, ActionError::NotLobby);
}

#[test]
fn start_game_refuses_too_few_seats_and_a_missing_deck() {
    let mut rng = PlayRng::seeded(1);
    let mut one = lobby(1);
    assert_eq!(
        engine::start_game(&mut one, &mut rng, now()).expect_err("refused"),
        ActionError::TooFewSeats
    );

    let mut two = lobby(2);
    two.seats[1].deck.clear();
    assert_eq!(
        engine::start_game(&mut two, &mut rng, now()).expect_err("refused"),
        ActionError::NoDeck("Bob".to_string())
    );
    assert_eq!(two.status, RoomStatus::Lobby);
    assert_eq!(two.version, 0);
}

// ---------- Library actions ----------

#[test]
fn draw_moves_the_top_card_into_the_hand() {
    let (mut state, mut rng) = started(2);
    let top = seat_of(&state, ALICE).library[0];
    let out = act(&mut state, &mut rng, ALICE, Action::Draw { n: 2 });
    let alice = seat_of(&state, ALICE);
    assert_eq!(alice.hand.len(), 9);
    assert_eq!(alice.library.len(), 11);
    assert_eq!(alice.hand[7], top);
    assert_eq!(card_of(&state, top).zone, Zone::Hand);
    assert_eq!(out.changes.cards.len(), 2);
    assert!(out.changes.seats.contains(&ALICE));
    assert_eq!(last_log(&state), "Alice drew 2 cards");
}

#[test]
fn draw_is_clamped_to_the_library_and_bounded() {
    let (mut state, mut rng) = started(2);
    let pos = state.seats.iter().position(|s| s.id == ALICE).unwrap();
    state.seats[pos].library.truncate(1);
    act(&mut state, &mut rng, ALICE, Action::Draw { n: 5 });
    assert_eq!(seat_of(&state, ALICE).library.len(), 0);
    assert_eq!(last_log(&state), "Alice drew 1 card");

    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, Action::Draw { n: 0 }),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, Action::Draw { n: 999 }),
        ActionError::Invalid(_)
    ));
}

#[test]
fn shuffle_keeps_the_same_cards_and_logs() {
    let (mut state, mut rng) = started(2);
    let before: std::collections::BTreeSet<CardId> =
        seat_of(&state, ALICE).library.iter().copied().collect();
    act(&mut state, &mut rng, ALICE, Action::Shuffle);
    let after: std::collections::BTreeSet<CardId> =
        seat_of(&state, ALICE).library.iter().copied().collect();
    assert_eq!(before, after);
    assert_eq!(last_log(&state), "Alice shuffled their library");
}

#[test]
fn mulligan_returns_the_hand_and_redraws() {
    let (mut state, mut rng) = started(2);
    let old: Vec<CardId> = seat_of(&state, ALICE).hand.clone();
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Mulligan { hand_size: 6 },
    );
    let alice = seat_of(&state, ALICE);
    assert_eq!(alice.hand.len(), 6);
    assert_eq!(alice.library.len(), 14);
    for id in &old {
        let card = card_of(&state, *id);
        assert!(matches!(card.zone, Zone::Hand | Zone::Library));
    }
    assert_eq!(last_log(&state), "Alice mulliganed to 6 cards");
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Mulligan { hand_size: 500 }
        ),
        ActionError::Invalid(_)
    ));
}

#[test]
fn reorder_top_rewrites_the_prefix() {
    let (mut state, mut rng) = started(2);
    let top: Vec<CardId> = seat_of(&state, ALICE).library[0..3].to_vec();
    let flipped = vec![top[2], top[0], top[1]];
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::ReorderTop {
            cards: flipped.clone(),
        },
    );
    assert_eq!(seat_of(&state, ALICE).library[0..3], flipped[..]);
    assert_eq!(
        last_log(&state),
        "Alice rearranged the top 3 cards of their library"
    );
}

#[test]
fn reorder_top_refuses_anything_but_the_exact_top() {
    let (mut state, mut rng) = started(2);
    let library = seat_of(&state, ALICE).library.clone();
    let deeper = vec![library[0], library[5]];
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::ReorderTop { cards: deeper }
        ),
        ActionError::Invalid(_)
    ));
    let repeated = vec![library[0], library[0]];
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::ReorderTop { cards: repeated }
        ),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::ReorderTop { cards: vec![] }
        ),
        ActionError::Invalid(_)
    ));
    assert_eq!(seat_of(&state, ALICE).library, library);
}

// ---------- Moving ----------

#[test]
fn move_hand_to_battlefield_places_and_untaps() {
    let (mut state, mut rng) = started(2);
    let id = seat_of(&state, ALICE).hand[0];
    let name = card_of(&state, id).def.name.clone();
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveCard {
            card: id,
            zone: Zone::Battlefield,
            placement: None,
            x: Some(2.5),
            y: Some(-1.0),
            face_down: None,
        },
    );
    let card = card_of(&state, id);
    assert_eq!(card.zone, Zone::Battlefield);
    assert!(!card.tapped);
    assert!(!card.face_down);
    assert_eq!(card.x, 1.0, "x is clamped into the battlefield");
    assert_eq!(card.y, 0.0, "y is clamped into the battlefield");
    assert_eq!(seat_of(&state, ALICE).hand.len(), 6);
    assert_eq!(seat_of(&state, ALICE).battlefield, vec![id]);
    assert_eq!(
        last_log(&state),
        format!("Alice moved {name} to the battlefield")
    );
}

#[test]
fn move_battlefield_to_graveyard_resets_the_card_and_goes_on_top() {
    let (mut state, mut rng) = started(2);
    let first = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let second = play_from_hand(&mut state, &mut rng, ALICE, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Tap {
            card: first,
            tapped: true,
        },
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Counter {
            card: first,
            name: "+1/+1".to_string(),
            delta: 2,
        },
    );
    act(&mut state, &mut rng, ALICE, mv(first, Zone::Graveyard));
    act(&mut state, &mut rng, ALICE, mv(second, Zone::Graveyard));

    let card = card_of(&state, first);
    assert_eq!(card.zone, Zone::Graveyard);
    assert!(!card.tapped);
    assert!(card.counters.is_empty());
    assert_eq!(card.face_index, 0);
    // graveyard[0] is the top — the most recently put card.
    assert_eq!(seat_of(&state, ALICE).graveyard, vec![second, first]);
    assert!(seat_of(&state, ALICE).battlefield.is_empty());
}

#[test]
fn move_to_the_library_honours_placement() {
    let (mut state, mut rng) = started(2);
    let id = seat_of(&state, ALICE).hand[0];
    let bottom_target = *seat_of(&state, ALICE).library.last().unwrap();
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveCard {
            card: id,
            zone: Zone::Library,
            placement: Some(Placement::Bottom),
            x: None,
            y: None,
            face_down: None,
        },
    );
    let library = &seat_of(&state, ALICE).library;
    assert_eq!(*library.last().unwrap(), id);
    assert_eq!(library[library.len() - 2], bottom_target);

    let other = seat_of(&state, ALICE).hand[0];
    act(&mut state, &mut rng, ALICE, mv(other, Zone::Library));
    assert_eq!(seat_of(&state, ALICE).library[0], other, "top by default");
}

#[test]
fn a_card_reaches_every_zone_from_the_hand() {
    for zone in [
        Zone::Battlefield,
        Zone::Graveyard,
        Zone::Exile,
        Zone::Command,
    ] {
        let (mut state, mut rng) = started(2);
        let id = seat_of(&state, ALICE).hand[0];
        act(&mut state, &mut rng, ALICE, mv(id, zone));
        assert_eq!(card_of(&state, id).zone, zone);
        let alice = seat_of(&state, ALICE);
        let (list, expected) = match zone {
            Zone::Battlefield => (&alice.battlefield, vec![id]),
            Zone::Graveyard => (&alice.graveyard, vec![id]),
            Zone::Exile => (&alice.exile, vec![id]),
            // The commander is already sitting in the command zone; `top` puts this one
            // in front of it.
            _ => (&alice.command, vec![id, commander_of(&state, ALICE)]),
        };
        assert_eq!(list, &expected, "{zone:?}");
        assert_eq!(alice.hand.len(), 6);
    }
}

#[test]
fn a_token_leaving_the_battlefield_is_deleted() {
    let (mut state, mut rng) = started(2);
    let out = act(
        &mut state,
        &mut rng,
        ALICE,
        Action::CreateToken {
            name: "Treasure".to_string(),
            card_id: None,
            type_line: Some("Token Artifact — Treasure".to_string()),
            power_toughness: None,
            colors: vec![],
            x: 0.4,
            y: 0.4,
            count: 1,
        },
    );
    let id = *out.changes.cards.iter().next().expect("a token");
    let out = act(&mut state, &mut rng, ALICE, mv(id, Zone::Graveyard));
    assert!(!state.cards.contains_key(&id));
    assert!(out.changes.removed.contains(&id));
    assert!(seat_of(&state, ALICE).graveyard.is_empty());
    assert!(seat_of(&state, ALICE).battlefield.is_empty());
}

#[test]
fn a_card_leaving_the_battlefield_detaches_what_was_on_it() {
    let (mut state, mut rng) = started(2);
    let creature = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let aura = play_from_hand(&mut state, &mut rng, ALICE, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Attach {
            card: aura,
            to: Some(creature),
        },
    );
    assert_eq!(card_of(&state, aura).attached_to, Some(creature));

    let out = act(&mut state, &mut rng, ALICE, mv(creature, Zone::Graveyard));
    assert_eq!(card_of(&state, aura).attached_to, None);
    assert_eq!(card_of(&state, aura).zone, Zone::Battlefield);
    assert!(out.changes.cards.contains(&aura));
}

#[test]
fn move_card_refuses_a_card_another_seat_controls() {
    let (mut state, mut rng) = started(2);
    let bobs = seat_of(&state, BOB).hand[0];
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, mv(bobs, Zone::Graveyard)),
        ActionError::NotYourCard
    );
    assert_eq!(card_of(&state, bobs).zone, Zone::Hand);
    assert_eq!(state.version, 1, "a refusal leaves the version alone");
}

#[test]
fn move_card_treats_a_library_card_as_unknown() {
    let (mut state, mut rng) = started(2);
    let own = seat_of(&state, ALICE).library[0];
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, mv(own, Zone::Hand)),
        ActionError::NoSuchCard
    );
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, mv(4_000_000_001, Zone::Hand)),
        ActionError::NoSuchCard
    );
}

#[test]
fn move_library_card_lifts_a_card_out_of_the_middle() {
    let (mut state, mut rng) = started(2);
    let deep = seat_of(&state, ALICE).library[5];
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveLibraryCard {
            card: deep,
            zone: Zone::Hand,
            placement: None,
            x: None,
            y: None,
            face_down: None,
        },
    );
    let alice = seat_of(&state, ALICE);
    assert_eq!(alice.library.len(), 12);
    assert!(!alice.library.contains(&deep));
    assert_eq!(*alice.hand.last().unwrap(), deep);
    assert_eq!(last_log(&state), "Alice moved a card to the hand");
}

#[test]
fn move_library_card_refuses_cards_outside_the_actors_library() {
    let (mut state, mut rng) = started(2);
    let in_hand = seat_of(&state, ALICE).hand[0];
    let bobs = seat_of(&state, BOB).library[0];
    let call = |card| Action::MoveLibraryCard {
        card,
        zone: Zone::Hand,
        placement: None,
        x: None,
        y: None,
        face_down: None,
    };
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, call(in_hand)),
        ActionError::WrongZone
    );
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, call(bobs)),
        ActionError::NotYourCard
    );
}

#[test]
fn the_log_never_names_a_card_that_stayed_hidden() {
    let (mut state, mut rng) = started(2);
    let id = seat_of(&state, ALICE).hand[0];
    let name = card_of(&state, id).def.name.clone();
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveCard {
            card: id,
            zone: Zone::Library,
            placement: Some(Placement::Top),
            x: None,
            y: None,
            face_down: None,
        },
    );
    assert_eq!(last_log(&state), "Alice moved a card to the library");
    assert!(!last_log(&state).contains(&name));

    let hidden = seat_of(&state, ALICE).hand[0];
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveCard {
            card: hidden,
            zone: Zone::Battlefield,
            placement: None,
            x: Some(0.2),
            y: Some(0.2),
            face_down: Some(true),
        },
    );
    assert_eq!(last_log(&state), "Alice moved a card to the battlefield");
    assert!(card_of(&state, hidden).face_down);
}

// ---------- Battlefield state ----------

#[test]
fn tap_needs_the_battlefield_and_logs_by_name() {
    let (mut state, mut rng) = started(2);
    let in_hand = seat_of(&state, ALICE).hand[0];
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Tap {
                card: in_hand,
                tapped: true
            }
        ),
        ActionError::WrongZone
    );
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let name = card_of(&state, id).def.name.clone();
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Tap {
            card: id,
            tapped: true,
        },
    );
    assert!(card_of(&state, id).tapped);
    assert_eq!(last_log(&state), format!("Alice tapped {name}"));
}

#[test]
fn untap_all_only_touches_the_actors_own_cards() {
    let (mut state, mut rng) = started(2);
    let mine = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let theirs = play_from_hand(&mut state, &mut rng, BOB, 0);
    for (seat, card) in [(ALICE, mine), (BOB, theirs)] {
        act(
            &mut state,
            &mut rng,
            seat,
            Action::Tap { card, tapped: true },
        );
    }
    let out = act(&mut state, &mut rng, ALICE, Action::UntapAll);
    assert!(!card_of(&state, mine).tapped);
    assert!(card_of(&state, theirs).tapped);
    assert_eq!(out.changes.cards, [mine].into_iter().collect());
    assert_eq!(last_log(&state), "Alice untapped everything");
}

#[test]
fn set_position_clamps_and_does_not_log() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let before = state.log.len();
    let version = state.version;
    let out = act(
        &mut state,
        &mut rng,
        ALICE,
        Action::SetPosition {
            card: id,
            x: 0.25,
            y: 9.0,
        },
    );
    assert_eq!(state.log.len(), before, "set_position is silent");
    assert_eq!(out.changes.log_from, state.log.len());
    assert_eq!(state.version, version + 1);
    assert_eq!(card_of(&state, id).x, 0.25);
    assert_eq!(card_of(&state, id).y, 1.0);
}

#[test]
fn toggle_face_only_flips_a_card_with_two_faces() {
    let (mut state, mut rng) = started(2);
    let single = play_from_hand(&mut state, &mut rng, ALICE, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::ToggleFace { card: single },
    );
    assert_eq!(card_of(&state, single).face_index, 0);

    let dfc = seat_of(&state, ALICE).hand[0];
    state.cards.get_mut(&dfc).unwrap().def = double_faced("Delver");
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::ToggleFace { card: dfc },
    );
    assert_eq!(card_of(&state, dfc).face_index, 1);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::ToggleFace { card: dfc },
    );
    assert_eq!(card_of(&state, dfc).face_index, 0);
}

#[test]
fn set_face_down_is_battlefield_only() {
    let (mut state, mut rng) = started(2);
    let in_hand = seat_of(&state, ALICE).hand[0];
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::SetFaceDown {
                card: in_hand,
                face_down: true
            }
        ),
        ActionError::WrongZone
    );
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::SetFaceDown {
            card: id,
            face_down: true,
        },
    );
    assert!(card_of(&state, id).face_down);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::SetFaceDown {
            card: id,
            face_down: false,
        },
    );
    assert!(!card_of(&state, id).face_down);
}

#[test]
fn counters_add_and_are_removed_at_zero() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let counter = |delta| Action::Counter {
        card: id,
        name: "  +1/+1 ".to_string(),
        delta,
    };
    act(&mut state, &mut rng, ALICE, counter(3));
    assert_eq!(card_of(&state, id).counters.get("+1/+1"), Some(&3));
    act(&mut state, &mut rng, ALICE, counter(-5));
    assert!(card_of(&state, id).counters.is_empty());

    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, counter(0)),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Counter {
                card: id,
                name: "x".repeat(200),
                delta: 1
            }
        ),
        ActionError::Invalid(_)
    ));
}

#[test]
fn reveal_is_hand_only_and_names_the_card() {
    let (mut state, mut rng) = started(2);
    let played = play_from_hand(&mut state, &mut rng, ALICE, 0);
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Reveal {
                card: played,
                revealed: true
            }
        ),
        ActionError::WrongZone
    );
    let id = seat_of(&state, ALICE).hand[0];
    let name = card_of(&state, id).def.name.clone();
    let out = act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Reveal {
            card: id,
            revealed: true,
        },
    );
    assert!(card_of(&state, id).revealed);
    assert!(out.changes.seats.contains(&ALICE));
    assert_eq!(
        last_log(&state),
        format!("Alice revealed {name} from their hand")
    );
}

#[test]
fn attach_refuses_itself_a_loop_and_a_non_battlefield_target() {
    let (mut state, mut rng) = started(2);
    let a = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let b = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let in_hand = seat_of(&state, ALICE).hand[0];

    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Attach {
                card: a,
                to: Some(a)
            }
        ),
        ActionError::Invalid(_)
    ));
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Attach {
                card: a,
                to: Some(in_hand)
            }
        ),
        ActionError::WrongZone
    );

    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Attach {
            card: a,
            to: Some(b),
        },
    );
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Attach {
                card: b,
                to: Some(a)
            }
        ),
        ActionError::Invalid(_),
    ));
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Attach { card: a, to: None },
    );
    assert_eq!(card_of(&state, a).attached_to, None);
}

#[test]
fn take_control_moves_the_card_between_battlefields() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, BOB, 0);
    let name = card_of(&state, id).def.name.clone();
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            BOB,
            Action::TakeControl {
                card: id,
                x: 0.1,
                y: 0.1
            }
        ),
        ActionError::Invalid(_)
    ));
    let out = act(
        &mut state,
        &mut rng,
        ALICE,
        Action::TakeControl {
            card: id,
            x: 0.1,
            y: 0.9,
        },
    );
    let card = card_of(&state, id);
    assert_eq!(card.controller, ALICE);
    assert_eq!(card.owner, BOB);
    assert_eq!(card.x, 0.1);
    assert_eq!(seat_of(&state, ALICE).battlefield, vec![id]);
    assert!(seat_of(&state, BOB).battlefield.is_empty());
    assert!(out.changes.seats.contains(&ALICE) && out.changes.seats.contains(&BOB));
    assert_eq!(last_log(&state), format!("Alice took control of {name}"));
}

#[test]
fn a_borrowed_permanent_leaves_to_its_owners_zone() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, BOB, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::TakeControl {
            card: id,
            x: 0.5,
            y: 0.5,
        },
    );
    let out = act(&mut state, &mut rng, ALICE, mv(id, Zone::Graveyard));
    let card = card_of(&state, id);
    assert_eq!(card.owner, BOB);
    assert_eq!(
        card.controller, BOB,
        "the loan ends when it leaves the table"
    );
    assert_eq!(seat_of(&state, BOB).graveyard, vec![id]);
    assert!(seat_of(&state, ALICE).graveyard.is_empty());
    assert!(seat_of(&state, ALICE).battlefield.is_empty());
    assert!(out.changes.seats.contains(&ALICE) && out.changes.seats.contains(&BOB));
}

#[test]
fn create_token_mints_the_requested_count_and_is_bounded() {
    let (mut state, mut rng) = started(2);
    let token = |count| Action::CreateToken {
        name: " Soldier ".to_string(),
        card_id: None,
        type_line: Some("Token Creature — Soldier".to_string()),
        power_toughness: Some("1/1".to_string()),
        colors: vec!["W".to_string()],
        x: 0.5,
        y: 0.5,
        count,
    };
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, token(0)),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, token(21)),
        ActionError::Invalid(_)
    ));
    let out = act(&mut state, &mut rng, ALICE, token(3));
    assert_eq!(out.changes.cards.len(), 3);
    assert_eq!(seat_of(&state, ALICE).battlefield.len(), 3);
    let mut xs = Vec::new();
    for id in &out.changes.cards {
        let card = card_of(&state, *id);
        assert!(card.def.is_token);
        assert_eq!(card.def.name, "Soldier");
        assert_eq!(card.def.game, "mtg");
        assert_eq!(card.zone, Zone::Battlefield);
        assert_eq!(card.controller, ALICE);
        assert_eq!(card.power_toughness.as_deref(), Some("1/1"));
        xs.push(card.x);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(xs[1] > xs[0], "each extra token is nudged aside");
    assert_eq!(last_log(&state), "Alice created 3 Soldier tokens");
}

#[test]
fn clone_card_makes_a_token_copy_beside_the_original() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    let name = card_of(&state, id).def.name.clone();
    let out = act(&mut state, &mut rng, ALICE, Action::CloneCard { card: id });
    let copy_id = *out.changes.cards.iter().next().expect("a copy");
    let copy = card_of(&state, copy_id);
    assert_ne!(copy_id, id);
    assert!(copy.def.is_token);
    assert_eq!(copy.def.name, name);
    assert_eq!(copy.zone, Zone::Battlefield);
    assert_eq!(seat_of(&state, ALICE).battlefield.len(), 2);
    assert_eq!(last_log(&state), format!("Alice copied {name}"));

    let in_hand = seat_of(&state, ALICE).hand[0];
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::CloneCard { card: in_hand }
        ),
        ActionError::WrongZone
    );
}

// ---------- Seat state ----------

#[test]
fn life_moves_and_clamps() {
    let (mut state, mut rng) = started(2);
    act(&mut state, &mut rng, ALICE, Action::Life { delta: -3 });
    assert_eq!(seat_of(&state, ALICE).life, 37);
    assert_eq!(last_log(&state), "Alice lost 3 life (37)");
    for _ in 0..20 {
        act(&mut state, &mut rng, ALICE, Action::Life { delta: -1000 });
    }
    assert_eq!(seat_of(&state, ALICE).life, -999);
    for _ in 0..20 {
        act(&mut state, &mut rng, ALICE, Action::Life { delta: 1000 });
    }
    assert_eq!(seat_of(&state, ALICE).life, 9999);
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, Action::Life { delta: 0 }),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, Action::Life { delta: 5000 }),
        ActionError::Invalid(_)
    ));
}

#[test]
fn player_counters_are_vocabulary_bound_and_floor_at_zero() {
    let (mut state, mut rng) = started(2);
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::PlayerCounter {
                name: "rad".to_string(),
                delta: 1
            }
        ),
        ActionError::Invalid(_)
    ));
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::PlayerCounter {
            name: "poison".to_string(),
            delta: 3,
        },
    );
    assert_eq!(seat_of(&state, ALICE).counters.get("poison"), Some(&3));
    assert_eq!(last_log(&state), "Alice's poison is now 3");
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::PlayerCounter {
            name: "poison".to_string(),
            delta: -9,
        },
    );
    assert!(seat_of(&state, ALICE).counters.is_empty());
}

#[test]
fn commander_damage_needs_a_real_source_and_floors_at_zero() {
    let (mut state, mut rng) = started(2);
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::CommanderDamage {
                from_seat: 999,
                delta: 3
            }
        ),
        ActionError::NoSuchSeat
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::CommanderDamage {
            from_seat: BOB,
            delta: 7,
        },
    );
    assert_eq!(seat_of(&state, ALICE).commander_damage.get(&BOB), Some(&7));
    assert_eq!(
        last_log(&state),
        "Alice has taken 7 commander damage from Bob"
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::CommanderDamage {
            from_seat: BOB,
            delta: -50,
        },
    );
    assert!(seat_of(&state, ALICE).commander_damage.is_empty());
}

// ---------- Turns, host powers and the end ----------

#[test]
fn pass_turn_skips_seats_that_are_out_and_wraps() {
    let (mut state, mut rng) = started(3);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::SetActive { seat: ALICE },
    );
    act(&mut state, &mut rng, BOB, Action::Concede);
    assert_eq!(state.status, RoomStatus::Playing);

    act(&mut state, &mut rng, ALICE, Action::PassTurn);
    assert_eq!(state.turn.active_seat, Some(CARA));
    assert_eq!(state.turn.number, 2);
    assert_eq!(state.turn.phase, Phase::Untap);
    assert_eq!(last_log(&state), "Alice passed the turn to Cara (turn 2)");

    act(&mut state, &mut rng, CARA, Action::PassTurn);
    assert_eq!(state.turn.active_seat, Some(ALICE), "and wraps around");
    assert_eq!(state.turn.number, 3);
}

#[test]
fn set_phase_is_free_and_set_active_is_host_only() {
    let (mut state, mut rng) = started(2);
    act(
        &mut state,
        &mut rng,
        BOB,
        Action::SetPhase {
            phase: Phase::Combat,
        },
    );
    assert_eq!(state.turn.phase, Phase::Combat);
    assert_eq!(last_log(&state), "Bob moved to combat");

    assert_eq!(
        fails(&mut state, &mut rng, BOB, Action::SetActive { seat: BOB }),
        ActionError::HostOnly
    );
    act(&mut state, &mut rng, ALICE, Action::SetActive { seat: BOB });
    assert_eq!(state.turn.active_seat, Some(BOB));
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, Action::SetActive { seat: 404 }),
        ActionError::NoSuchSeat
    );
}

#[test]
fn conceding_down_to_one_seat_finishes_the_game() {
    let (mut state, mut rng) = started(2);
    act(&mut state, &mut rng, ALICE, Action::Concede);
    assert!(seat_of(&state, ALICE).out);
    assert_eq!(state.status, RoomStatus::Finished);
    assert_eq!(state.winner, Some(BOB));
    assert_eq!(last_log(&state), "Alice conceded — Bob wins");
    assert_eq!(
        fails(&mut state, &mut rng, BOB, Action::Draw { n: 1 }),
        ActionError::NotPlaying
    );
}

#[test]
fn an_out_seat_can_still_chat_but_nothing_else() {
    let (mut state, mut rng) = started(3);
    act(&mut state, &mut rng, BOB, Action::Concede);
    assert_eq!(state.status, RoomStatus::Playing);
    assert_eq!(
        fails(&mut state, &mut rng, BOB, Action::Draw { n: 1 }),
        ActionError::SeatOut
    );
    act(
        &mut state,
        &mut rng,
        BOB,
        Action::Chat {
            text: "  gg  ".to_string(),
        },
    );
    let entry = state.log.last().unwrap();
    assert_eq!(entry.kind, LogKind::Chat);
    assert_eq!(entry.text, "gg");
    assert_eq!(entry.seat, Some(BOB));
}

#[test]
fn chat_works_in_the_lobby_and_after_the_end_and_is_bounded() {
    let mut state = lobby(2);
    let mut rng = PlayRng::seeded(2);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Chat {
            text: "hi".to_string(),
        },
    );
    assert_eq!(state.version, 1);
    assert_eq!(last_log(&state), "hi");

    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Chat {
                text: "   ".to_string()
            }
        ),
        ActionError::Invalid(_)
    ));
    assert!(matches!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::Chat {
                text: "x".repeat(MAX_CHAT + 1)
            }
        ),
        ActionError::Invalid(_)
    ));
    assert_eq!(
        fails(&mut state, &mut rng, 999, Action::Chat { text: "?".into() }),
        ActionError::NoSuchSeat
    );
}

#[test]
fn end_game_is_host_only_and_names_a_winner() {
    let (mut state, mut rng) = started(2);
    assert_eq!(
        fails(&mut state, &mut rng, BOB, Action::EndGame { winner: None }),
        ActionError::HostOnly
    );
    assert_eq!(
        fails(
            &mut state,
            &mut rng,
            ALICE,
            Action::EndGame { winner: Some(77) }
        ),
        ActionError::NoSuchSeat
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::EndGame { winner: Some(BOB) },
    );
    assert_eq!(state.status, RoomStatus::Finished);
    assert_eq!(state.winner, Some(BOB));
    assert_eq!(last_log(&state), "Alice ended the game — Bob wins");
}

#[test]
fn dice_and_coins_land_in_range_and_are_bounded() {
    let (mut state, mut rng) = started(2);
    for _ in 0..30 {
        act(&mut state, &mut rng, ALICE, Action::Roll { sides: 20 });
        let text = last_log(&state);
        let value: u32 = text
            .rsplit(' ')
            .next()
            .and_then(|v| v.parse().ok())
            .expect("a rolled value");
        assert!((1..=20).contains(&value), "{text}");
        assert!(text.starts_with("Alice rolled a d20: "));
    }
    for _ in 0..10 {
        act(&mut state, &mut rng, ALICE, Action::FlipCoin);
        let text = last_log(&state);
        assert!(text == "Alice flipped a coin: heads" || text == "Alice flipped a coin: tails");
    }
    for sides in [0, 1, 1001] {
        assert!(matches!(
            fails(&mut state, &mut rng, ALICE, Action::Roll { sides }),
            ActionError::Invalid(_)
        ));
    }
}

// ---------- Peeks ----------

#[test]
fn look_top_answers_privately_without_leaking_into_the_log() {
    let (mut state, mut rng) = started(2);
    let top: Vec<CardId> = seat_of(&state, ALICE).library[0..3].to_vec();
    let names: Vec<String> = top
        .iter()
        .map(|id| card_of(&state, *id).def.name.clone())
        .collect();
    let out = act(&mut state, &mut rng, ALICE, Action::LookTop { n: 3 });
    let peek = out.peek.expect("a peek");
    assert_eq!(peek.kind, PeekKind::LookTop);
    assert_eq!(
        peek.cards.iter().map(|c| c.id).collect::<Vec<_>>(),
        top,
        "top first"
    );
    assert!(peek.cards.iter().all(|c| c.def.is_some()));
    assert_eq!(
        last_log(&state),
        "Alice looked at the top 3 cards of their library"
    );
    for name in names {
        assert!(!last_log(&state).contains(&name), "the log leaked {name}");
    }
    // The cards stayed put and nobody's view changed.
    assert_eq!(seat_of(&state, ALICE).library[0..3], top[..]);
    assert!(out.changes.cards.is_empty());
    assert!(matches!(
        fails(&mut state, &mut rng, ALICE, Action::LookTop { n: 0 }),
        ActionError::Invalid(_)
    ));
}

#[test]
fn search_library_peeks_the_whole_library_in_order() {
    let (mut state, mut rng) = started(2);
    let library = seat_of(&state, ALICE).library.clone();
    let out = act(&mut state, &mut rng, ALICE, Action::SearchLibrary);
    let peek = out.peek.expect("a peek");
    assert_eq!(peek.kind, PeekKind::SearchLibrary);
    assert_eq!(peek.cards.iter().map(|c| c.id).collect::<Vec<_>>(), library);
    assert_eq!(last_log(&state), "Alice searched their library");
}

// ---------- Views ----------

#[test]
fn a_snapshot_hides_libraries_and_other_hands() {
    let (state, _) = started(2);
    let mine = view::snapshot_for(&state, Some(ALICE));
    let alice = mine.seats.iter().find(|s| s.id == ALICE).unwrap();
    let bob = mine.seats.iter().find(|s| s.id == BOB).unwrap();

    assert_eq!(alice.hand.as_ref().unwrap().len(), 7, "my own hand is open");
    assert_eq!(alice.library_count, 13);
    assert_eq!(bob.hand.as_ref().unwrap().len(), 0, "their hand is closed");
    assert_eq!(bob.hand_count, 7);
    assert_eq!(bob.library_count, 13);

    // No library card, and none of Bob's hand, reaches me.
    for card in &mine.cards {
        assert_ne!(card.zone, Zone::Library);
        assert!(card.zone != Zone::Hand || card.owner == ALICE);
    }
    assert_eq!(mine.viewer_seat, Some(ALICE));
    assert_eq!(mine.cards.len(), 7 + 1 + 1, "my hand plus both commanders");

    let spectator = view::snapshot_for(&state, None);
    assert_eq!(spectator.viewer_seat, None);
    assert_eq!(
        spectator.cards.len(),
        2,
        "only the command zones are public"
    );
    for seat in &spectator.seats {
        assert!(seat.hand.as_ref().unwrap().is_empty());
    }
}

#[test]
fn a_revealed_hand_card_becomes_public() {
    let (mut state, mut rng) = started(2);
    let id = seat_of(&state, ALICE).hand[0];
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Reveal {
            card: id,
            revealed: true,
        },
    );
    let theirs = view::snapshot_for(&state, Some(BOB));
    let alice = theirs.seats.iter().find(|s| s.id == ALICE).unwrap();
    assert_eq!(alice.hand, Some(vec![id]));
    let seen = theirs.cards.iter().find(|c| c.id == id).expect("the card");
    assert!(seen.def.is_some());
    assert!(seen.revealed);
}

#[test]
fn a_face_down_permanent_hides_its_definition_from_everyone_else() {
    let (mut state, mut rng) = started(2);
    let id = seat_of(&state, ALICE).hand[0];
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::MoveCard {
            card: id,
            zone: Zone::Battlefield,
            placement: None,
            x: Some(0.3),
            y: Some(0.3),
            face_down: Some(true),
        },
    );
    let mine = view::card_view(card_of(&state, id), Some(ALICE)).expect("visible to me");
    assert!(mine.def.is_some());
    for viewer in [Some(BOB), None] {
        let theirs = view::card_view(card_of(&state, id), viewer).expect("visible as an id");
        assert!(theirs.def.is_none());
        assert!(theirs.face_down);
        assert_eq!(theirs.id, id);
    }
}

#[test]
fn a_patch_sends_a_card_that_went_hidden_as_removed() {
    let (mut state, mut rng) = started(2);
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    // Everyone can see it on the battlefield…
    let out = act(&mut state, &mut rng, ALICE, mv(id, Zone::Hand));
    let mine = view::patch_for(&state, &out.changes, Some(ALICE));
    let theirs = view::patch_for(&state, &out.changes, Some(BOB));
    assert!(mine.cards.iter().any(|c| c.id == id));
    assert!(!mine.removed.contains(&id));
    // …but once it is back in a hand, other seats must forget it.
    assert!(!theirs.cards.iter().any(|c| c.id == id));
    assert!(theirs.removed.contains(&id));
    assert_eq!(theirs.version, state.version);
    assert_eq!(theirs.status, state.status);
    assert_eq!(theirs.turn, state.turn);
}

#[test]
fn a_patch_carries_only_the_changed_seats_and_the_new_log() {
    let (mut state, mut rng) = started(3);
    let out = act(&mut state, &mut rng, BOB, Action::Life { delta: -2 });
    let patch = view::patch_for(&state, &out.changes, Some(ALICE));
    assert_eq!(patch.seats.len(), 1);
    assert_eq!(patch.seats[0].id, BOB);
    assert_eq!(patch.seats[0].life, 38);
    assert_eq!(patch.log.len(), 1);
    assert_eq!(patch.log[0].text, "Bob lost 2 life (38)");
    assert!(patch.cards.is_empty());
}

#[test]
fn a_removed_token_reaches_every_viewer_as_removed() {
    let (mut state, mut rng) = started(2);
    let made = act(
        &mut state,
        &mut rng,
        ALICE,
        Action::CreateToken {
            name: "Beast".to_string(),
            card_id: None,
            type_line: None,
            power_toughness: Some("3/3".to_string()),
            colors: vec!["G".to_string()],
            x: 0.5,
            y: 0.5,
            count: 1,
        },
    );
    let id = *made.changes.cards.iter().next().unwrap();
    let out = act(&mut state, &mut rng, ALICE, mv(id, Zone::Exile));
    for viewer in [Some(ALICE), Some(BOB), None] {
        let patch = view::patch_for(&state, &out.changes, viewer);
        assert!(patch.removed.contains(&id));
        assert!(!patch.cards.iter().any(|c| c.id == id));
    }
}

#[test]
fn seat_snapshots_report_connection_from_the_socket_count() {
    let (mut state, _) = started(2);
    let changes = engine::set_connected(&mut state, ALICE, true, now()).expect("connect");
    assert_eq!(seat_of(&state, ALICE).connections, 1);
    assert_eq!(last_log(&state), "Alice connected");
    assert_eq!(state.log[changes.log_from..].len(), 1);
    let snap = view::snapshot_for(&state, Some(ALICE));
    assert!(snap.seats.iter().find(|s| s.id == ALICE).unwrap().connected);
    assert!(!snap.seats.iter().find(|s| s.id == BOB).unwrap().connected);

    // A second socket is not a second arrival, and the first to leave is not a departure.
    let logged = state.log.len();
    engine::set_connected(&mut state, ALICE, true, now()).expect("connect");
    engine::set_connected(&mut state, ALICE, false, now()).expect("disconnect");
    assert_eq!(state.log.len(), logged);
    engine::set_connected(&mut state, ALICE, false, now()).expect("disconnect");
    assert_eq!(last_log(&state), "Alice disconnected");
    assert_eq!(seat_of(&state, ALICE).connections, 0);
    // Saturating: one more close does not underflow.
    engine::set_connected(&mut state, ALICE, false, now()).expect("disconnect");
    assert_eq!(seat_of(&state, ALICE).connections, 0);
    assert_eq!(
        engine::set_connected(&mut state, 404, true, now()).expect_err("refused"),
        ActionError::NoSuchSeat
    );
}

// ---------- Version, log and serde ----------

#[test]
fn every_accepted_action_bumps_the_version_by_exactly_one() {
    let (mut state, mut rng) = started(2);
    let mut version = state.version;
    let id = seat_of(&state, ALICE).hand[0];
    let actions = vec![
        Action::Draw { n: 1 },
        Action::Shuffle,
        mv(id, Zone::Battlefield),
        Action::Tap {
            card: id,
            tapped: true,
        },
        Action::SetPosition {
            card: id,
            x: 0.2,
            y: 0.2,
        },
        Action::UntapAll,
        Action::Life { delta: -1 },
        Action::SetPhase { phase: Phase::End },
        Action::PassTurn,
        Action::FlipCoin,
        Action::Chat {
            text: "hello".to_string(),
        },
        Action::SearchLibrary,
    ];
    for action in actions {
        act(&mut state, &mut rng, ALICE, action);
        assert_eq!(state.version, version + 1);
        version = state.version;
    }
    // A refusal changes nothing.
    let _ = fails(&mut state, &mut rng, ALICE, Action::Draw { n: 0 });
    assert_eq!(state.version, version);
}

#[test]
fn the_log_is_capped_and_log_from_still_points_at_the_new_entry() {
    let (mut state, mut rng) = started(2);
    for i in 0..(MAX_LOG + 40) {
        let out = act(
            &mut state,
            &mut rng,
            ALICE,
            Action::Chat {
                text: format!("line {i}"),
            },
        );
        let fresh = &state.log[out.changes.log_from..];
        assert_eq!(fresh.len(), 1, "exactly the entry this action appended");
        assert_eq!(fresh[0].text, format!("line {i}"));
        assert!(state.log.len() <= MAX_LOG);
    }
    assert_eq!(state.log.len(), MAX_LOG);
    assert_eq!(
        state.log.last().unwrap().text,
        format!("line {}", MAX_LOG + 39)
    );
    // Log ids stay monotonic across the trim.
    let ids: Vec<u32> = state.log.iter().map(|e| e.id).collect();
    assert!(ids.windows(2).all(|w| w[1] > w[0]));
}

#[test]
fn a_snapshot_carries_only_the_tail_of_a_long_log() {
    let (mut state, mut rng) = started(2);
    for i in 0..150 {
        act(
            &mut state,
            &mut rng,
            ALICE,
            Action::Chat {
                text: format!("line {i}"),
            },
        );
    }
    let snap = view::snapshot_for(&state, Some(ALICE));
    assert_eq!(snap.log.len(), super::types::SNAPSHOT_LOG);
    assert_eq!(snap.log.last().unwrap().text, "line 149");
}

#[test]
fn room_state_round_trips_through_serde() {
    let (mut state, mut rng) = started(3);
    let id = play_from_hand(&mut state, &mut rng, ALICE, 0);
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Counter {
            card: id,
            name: "+1/+1".to_string(),
            delta: 2,
        },
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::CommanderDamage {
            from_seat: CARA,
            delta: 5,
        },
    );
    act(
        &mut state,
        &mut rng,
        ALICE,
        Action::Chat {
            text: "glhf".to_string(),
        },
    );
    let json = serde_json::to_string(&state).expect("serialize");
    let back: RoomState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(state, back);
}

#[test]
fn actions_are_refused_before_the_game_starts() {
    let mut state = lobby(2);
    let mut rng = PlayRng::seeded(5);
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, Action::Draw { n: 1 }),
        ActionError::NotPlaying
    );
    assert_eq!(
        fails(&mut state, &mut rng, ALICE, Action::PassTurn),
        ActionError::NotPlaying
    );
    assert_eq!(state.version, 0);
    assert!(state.log.is_empty());
}
