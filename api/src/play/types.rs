//! The **play** engine's vocabulary: the table state the server keeps, the per-viewer views
//! it sends, and the messages that flow over the room socket. Every wire shape here is
//! ts-rs exported (`Play*` in `web/src/lib/api/generated/`) so the SPA and the server share
//! one definition — the socket has no OpenAPI, so this file *is* its contract.
//!
//! Three layers, top to bottom:
//!
//! - **State** ([`RoomState`], [`SeatState`], [`CardInstance`], [`CardDef`]) — the
//!   authoritative table, persisted whole as JSON on `play_rooms.state`. Hidden information
//!   (library order, hands, face-down cards) lives here in the clear; nothing in this layer
//!   ever reaches a client directly.
//! - **Views** ([`Snapshot`], [`Patch`], [`SeatSnapshot`], [`CardView`]) — what one viewer
//!   is allowed to see, computed by `view.rs`. A library is a count, another seat's hand is
//!   a count, a face-down card someone else controls is an id with no `def`.
//! - **Messages** ([`ClientMessage`], [`Action`], [`ServerMessage`]) — the socket frames.
//!
//! Ids: a [`SeatId`] is the `play_seats.id` row id (stable across restarts, and what the
//! REST side hands out); a [`CardId`] is a random `u32` unique within the room, minted by
//! the engine when a game starts or a token is made — never a catalog id, so a client
//! can't address a card it hasn't been shown.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A card *instance* on this table (random per room, see the module docs).
pub type CardId = u32;
/// A seat — the `play_seats.id` row id.
pub type SeatId = i32;

// ---------- Vocabularies ----------

/// Where a card is. The command zone exists in every format (it's simply empty outside
/// Commander); the stack is not modelled — a manual table resolves spells by hand.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayZone"))]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Command,
}

impl Zone {
    pub fn as_str(self) -> &'static str {
        match self {
            Zone::Library => "library",
            Zone::Hand => "hand",
            Zone::Battlefield => "battlefield",
            Zone::Graveyard => "graveyard",
            Zone::Exile => "exile",
            Zone::Command => "command",
        }
    }
}

/// The turn structure the table bar walks through. Purely informational — nothing is
/// enforced by phase, it's a shared pointer to "where we are".
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayPhase"))]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Untap,
    Upkeep,
    Draw,
    Main1,
    Combat,
    Main2,
    End,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Untap => "untap",
            Phase::Upkeep => "upkeep",
            Phase::Draw => "draw",
            Phase::Main1 => "main1",
            Phase::Combat => "combat",
            Phase::Main2 => "main2",
            Phase::End => "end",
        }
    }
}

/// A room's lifecycle. Mirrors the `play_rooms.status` column (`as_str`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayRoomStatus"))]
#[serde(rename_all = "snake_case")]
pub enum RoomStatus {
    /// Seats are joining and loading decks; no table exists yet.
    Lobby,
    /// The table is live.
    Playing,
    /// The host ended the game (or everyone but one conceded). Read-only.
    Finished,
}

impl RoomStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RoomStatus::Lobby => "lobby",
            RoomStatus::Playing => "playing",
            RoomStatus::Finished => "finished",
        }
    }

    pub fn parse(s: &str) -> Option<RoomStatus> {
        match s {
            "lobby" => Some(RoomStatus::Lobby),
            "playing" => Some(RoomStatus::Playing),
            "finished" => Some(RoomStatus::Finished),
            _ => None,
        }
    }
}

/// The format vocabulary. Stored on `play_rooms.format`; decides the default starting life
/// and whether the command zone is used when a deck is loaded.
pub const FORMAT_COMMANDER: &str = "commander";
pub const FORMAT_CONSTRUCTED: &str = "constructed";
pub const FORMATS: [&str; 2] = [FORMAT_COMMANDER, FORMAT_CONSTRUCTED];

/// Default starting life for a format (`None` for an unknown format).
pub fn default_starting_life(format: &str) -> Option<i32> {
    match format {
        FORMAT_COMMANDER => Some(40),
        FORMAT_CONSTRUCTED => Some(20),
        _ => None,
    }
}

/// Per-player counters beyond life (commander damage is its own per-source map).
pub const PLAYER_COUNTERS: [&str; 3] = ["poison", "energy", "experience"];

/// Card counter names are free text (`+1/+1`, `loyalty`, `charge`, …), bounded here.
pub const MAX_COUNTER_NAME: usize = 24;
pub const MAX_CHAT: usize = 500;
pub const MAX_LOG: usize = 300;
/// How much of the log a fresh snapshot carries (the rest is history the client never had).
pub const SNAPSHOT_LOG: usize = 100;
/// Bounds on a `look_top` / `draw` request.
pub const MAX_DRAW: u32 = 30;
pub const MIN_PLAYERS: i32 = 2;
pub const MAX_PLAYERS: i32 = 6;
pub const MAX_DECK_CARDS: usize = 400;
pub const MAX_HAND_SIZE: u32 = 30;

// ---------- Card definitions ----------

/// One face of a card as the table shows it (the image itself is fetched through the
/// catalog image proxy by `card_id` + face index — see `cardImageUrl` in the SPA).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayCardFace"))]
pub struct CardFace {
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
}

/// What a card *is*, independent of where it sits: resolved once when a deck is loaded into
/// a seat (`handlers/tools/play/decks.rs`) and copied onto every instance. Kept lean on
/// purpose — the table renders the image, and the faces carry just enough text for the
/// hover preview / accessibility label.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayCardDef"))]
pub struct CardDef {
    /// Catalog external id (`cards.external_id`), used to build the image URL. `None` for an
    /// ad-hoc token typed in at the table (rendered as a text card).
    pub card_id: Option<String>,
    /// The catalog game slug the `card_id` belongs to (`mtg`).
    pub game: String,
    /// Display name (the front face's name for a double-faced card).
    pub name: String,
    /// Front face first; a transforming / modal DFC has two. Never empty.
    pub faces: Vec<CardFace>,
    /// Whether face index 1 has its own image (true for transform/MDFC layouts) — the SPA
    /// then requests `?face=1` when `face_index == 1`; otherwise the front image is reused.
    pub back_image: bool,
    /// Whether the catalog has any image for this printing (mirrors `Card.has_image`); a
    /// card without one renders as a text card instead of requesting a 404.
    pub has_image: bool,
    /// Colour letters (`W U B R G`) of the card, for the type-sorted battlefield grouping.
    pub colors: Vec<String>,
    pub cmc: Option<f64>,
    /// Loaded from the deck's command zone (Commander/partner) — starts in the command zone.
    pub is_commander: bool,
    /// A token: ceases to exist when it leaves the battlefield.
    pub is_token: bool,
}

// ---------- Authoritative state ----------

/// A card instance on the table. `owner` never changes; `controller` may (gain control).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CardInstance {
    pub id: CardId,
    pub def: CardDef,
    pub owner: SeatId,
    pub controller: SeatId,
    pub zone: Zone,
    pub tapped: bool,
    pub face_down: bool,
    /// Which of `def.faces` is up (0 front, 1 back).
    pub face_index: u8,
    /// A hand card shown to the table (public while it stays in hand).
    pub revealed: bool,
    /// `+1/+1`, `loyalty`, `charge`, … — name → count (0 removes the key).
    pub counters: BTreeMap<String, i32>,
    /// Battlefield placement as fractions (0..=1) of the controller's battlefield.
    pub x: f32,
    pub y: f32,
    /// Aura / Equipment / Fortification: the card this one is attached to (same battlefield).
    pub attached_to: Option<CardId>,
    /// Free-text P/T override for tokens typed in at the table (`3/3`).
    pub power_toughness: Option<String>,
}

/// One seat's table: its zones (ordered id lists; `library[0]` is the **top**), its life and
/// counters, and its commander damage taken keyed by the *source* seat.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeatState {
    pub id: SeatId,
    pub seat_index: i32,
    pub name: String,
    pub is_host: bool,
    /// Loaded before the game starts; consumed by `start_game` into `cards` + `library`.
    pub deck: Vec<CardDef>,
    pub deck_name: Option<String>,
    pub life: i32,
    /// `poison` / `energy` / `experience` → count (absent = 0).
    pub counters: BTreeMap<String, i32>,
    /// Commander damage taken, keyed by the source seat id.
    pub commander_damage: BTreeMap<SeatId, i32>,
    /// Conceded / eliminated — still at the table, greyed out.
    pub out: bool,
    /// Live socket count, maintained by the registry (not persisted meaningfully).
    pub connections: u32,
    pub library: Vec<CardId>,
    /// `hand[0]` is the leftmost card.
    pub hand: Vec<CardId>,
    pub battlefield: Vec<CardId>,
    /// `graveyard[0]` is the **top** (most recently put) card.
    pub graveyard: Vec<CardId>,
    pub exile: Vec<CardId>,
    pub command: Vec<CardId>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayTurn"))]
pub struct TurnState {
    /// 1-based; 0 before the first turn (never happens once `playing`).
    pub number: u32,
    pub active_seat: Option<SeatId>,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayLogKind"))]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    /// Something a seat did (drew, tapped, moved …), phrased by the engine.
    Action,
    /// A chat line typed by a seat.
    Chat,
    /// Table events not attributed to a seat (game started, seat connected …).
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayLogEntry"))]
pub struct LogEntry {
    /// Monotonic within the room.
    pub id: u32,
    pub at: DateTime<Utc>,
    pub kind: LogKind,
    pub seat: Option<SeatId>,
    pub text: String,
}

/// The whole table. Persisted as JSON; `version` bumps by exactly one per applied action
/// (the socket's patch ordering contract).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RoomState {
    pub version: u32,
    pub status: RoomStatus,
    pub format: String,
    pub starting_life: i32,
    pub seats: Vec<SeatState>,
    pub cards: BTreeMap<CardId, CardInstance>,
    pub turn: TurnState,
    pub log: Vec<LogEntry>,
    pub next_log_id: u32,
    pub winner: Option<SeatId>,
}

// ---------- Views ----------

/// A card as one viewer sees it. `def: None` means the viewer may know the card exists (a
/// face-down permanent someone else controls) but not what it is.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayCardView"))]
pub struct CardView {
    pub id: CardId,
    pub def: Option<CardDef>,
    pub owner: SeatId,
    pub controller: SeatId,
    pub zone: Zone,
    pub tapped: bool,
    pub face_down: bool,
    pub face_index: u8,
    pub revealed: bool,
    pub counters: BTreeMap<String, i32>,
    pub x: f32,
    pub y: f32,
    pub attached_to: Option<CardId>,
    pub power_toughness: Option<String>,
}

/// One seat as a viewer sees it: public zones as id lists, the library as a count, the hand
/// as ids for the seat's own viewer (plus any revealed card) and a count for everyone else.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlaySeatSnapshot"))]
pub struct SeatSnapshot {
    pub id: SeatId,
    pub seat_index: i32,
    pub name: String,
    pub is_host: bool,
    pub deck_name: Option<String>,
    pub life: i32,
    pub counters: BTreeMap<String, i32>,
    pub commander_damage: BTreeMap<SeatId, i32>,
    pub out: bool,
    pub connected: bool,
    pub library_count: u32,
    pub hand_count: u32,
    /// `Some` only for the viewer's own seat (the full hand, in order); for other seats it
    /// lists just the cards they've revealed.
    pub hand: Option<Vec<CardId>>,
    pub battlefield: Vec<CardId>,
    pub graveyard: Vec<CardId>,
    pub exile: Vec<CardId>,
    pub command: Vec<CardId>,
}

/// The full view for one connection, sent on hello and on resync.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlaySnapshot"))]
pub struct Snapshot {
    pub version: u32,
    pub status: RoomStatus,
    pub format: String,
    pub starting_life: i32,
    /// The seat this connection holds (`None` = spectator).
    pub viewer_seat: Option<SeatId>,
    pub seats: Vec<SeatSnapshot>,
    pub cards: Vec<CardView>,
    pub turn: TurnState,
    /// The most recent `SNAPSHOT_LOG` entries, oldest first.
    pub log: Vec<LogEntry>,
    pub winner: Option<SeatId>,
}

/// The delta one applied action produced, as one viewer sees it. `seats` carries every
/// changed seat in full (they're small); `cards` every changed card the viewer may see;
/// `removed` every changed card the viewer may no longer see (left the table, or went
/// somewhere hidden). `turn`/`status`/`winner` ride every patch — they're three words.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayPatch"))]
pub struct Patch {
    pub version: u32,
    pub status: RoomStatus,
    pub seats: Vec<SeatSnapshot>,
    pub cards: Vec<CardView>,
    pub removed: Vec<CardId>,
    pub turn: TurnState,
    pub log: Vec<LogEntry>,
    pub winner: Option<SeatId>,
}

// ---------- Actions (client → server) ----------

/// Where in an ordered zone a card goes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayPlacement"))]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Top,
    Bottom,
}

/// Everything a seat can do at the table. Validated by `engine::apply` against the actor:
/// a seat may only move/tap/counter cards it **controls** (hand + library + graveyard +
/// exile + command are owner-controlled zones), only change its **own** life/counters, and
/// only the host may `start`, `set_active` and `end_game`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayAction"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Draw `n` from the top of the library into the hand (clamped to what's there).
    Draw {
        n: u32,
    },
    /// Shuffle the library.
    Shuffle,
    /// Hand → library, shuffle, draw `hand_size`.
    Mulligan {
        hand_size: u32,
    },
    /// Move a visible card the actor controls to another zone of the **actor's** table.
    /// Battlefield placement uses `x`/`y`; ordered zones use `placement` (default top).
    /// A token leaving the battlefield is deleted.
    MoveCard {
        card: CardId,
        zone: Zone,
        placement: Option<Placement>,
        x: Option<f32>,
        y: Option<f32>,
        face_down: Option<bool>,
    },
    /// A card the actor was shown by `look_top` / `search_library` (still in the library).
    MoveLibraryCard {
        card: CardId,
        zone: Zone,
        placement: Option<Placement>,
        x: Option<f32>,
        y: Option<f32>,
        face_down: Option<bool>,
    },
    /// Put these (all currently the top-most cards of the library, in any order) back on top
    /// in the given order — the tail of a `look_top` / scry.
    ReorderTop {
        cards: Vec<CardId>,
    },
    /// Battlefield only; not logged.
    SetPosition {
        card: CardId,
        x: f32,
        y: f32,
    },
    Tap {
        card: CardId,
        tapped: bool,
    },
    /// Untap everything the actor controls.
    UntapAll,
    /// Transform / flip to the other face (DFCs; a single-faced card is unchanged).
    ToggleFace {
        card: CardId,
    },
    SetFaceDown {
        card: CardId,
        face_down: bool,
    },
    /// Add `delta` to a named counter on a card (result ≤ 0 removes it).
    Counter {
        card: CardId,
        name: String,
        delta: i32,
    },
    /// Show / hide a hand card to the table.
    Reveal {
        card: CardId,
        revealed: bool,
    },
    /// Attach to another battlefield card (any controller) or detach (`to: None`).
    Attach {
        card: CardId,
        to: Option<CardId>,
    },
    /// Take control of a battlefield card another seat controls (it moves to the actor's
    /// battlefield at `x`/`y`).
    TakeControl {
        card: CardId,
        x: f32,
        y: f32,
    },
    /// Make a token under the actor's control. `card_id` = a catalog token printing when
    /// picked from the search; otherwise a typed text card.
    CreateToken {
        name: String,
        card_id: Option<String>,
        type_line: Option<String>,
        power_toughness: Option<String>,
        colors: Vec<String>,
        x: f32,
        y: f32,
        /// How many identical tokens (1..=20).
        count: u32,
    },
    /// Copy a battlefield card the actor controls as a token next to it.
    CloneCard {
        card: CardId,
    },
    Life {
        delta: i32,
    },
    PlayerCounter {
        name: String,
        delta: i32,
    },
    /// Commander damage the actor **took** from `from_seat`'s commander.
    CommanderDamage {
        from_seat: SeatId,
        delta: i32,
    },
    /// Move to the next seat that is still in the game; phase resets to `untap`.
    PassTurn,
    SetPhase {
        phase: Phase,
    },
    /// Host only.
    SetActive {
        seat: SeatId,
    },
    Roll {
        sides: u32,
    },
    FlipCoin,
    Chat {
        text: String,
    },
    /// Answered with a private `peek` (the top `n` cards, top first). Not logged in detail
    /// (the log says "looked at the top N").
    LookTop {
        n: u32,
    },
    /// Answered with a private `peek` of the whole library in order.
    SearchLibrary,
    /// The actor drops out (still watches). If one seat remains, the game finishes.
    Concede,
    /// Host only: finish the game, optionally naming a winner.
    EndGame {
        winner: Option<SeatId>,
    },
}

/// What one viewer is privately shown after `look_top` / `search_library`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayPeek"))]
pub struct Peek {
    /// Top first for a `look_top`; library order for a search.
    pub cards: Vec<CardView>,
    /// `look_top` or `search_library`.
    pub kind: PeekKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayPeekKind"))]
#[serde(rename_all = "snake_case")]
pub enum PeekKind {
    LookTop,
    SearchLibrary,
}

// ---------- Socket frames ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayClientMessage"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Must be the first frame. No token = spectator.
    Hello {
        seat_token: Option<String>,
    },
    /// `id` is echoed on the matching `error` so the client can attribute a rejection.
    Action {
        id: Option<u32>,
        action: Action,
    },
    /// Host only, lobby only: build the table from the seats' loaded decks.
    Start,
    /// The client noticed a version gap; answered with a fresh `snapshot`.
    Resync,
    Ping,
}

/// A seat as the lobby (REST `PlayRoomSummary` and the socket's `lobby` frame) shows it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlaySeatView"))]
pub struct SeatView {
    pub id: SeatId,
    pub seat_index: i32,
    pub display_name: String,
    pub is_host: bool,
    /// Backed by an account (a guest seat is not).
    pub is_user: bool,
    pub ready: bool,
    /// Has at least one live socket.
    pub connected: bool,
    /// `deck` / `precon` / `text`, or `None` while nothing is loaded.
    pub deck_source: Option<String>,
    pub deck_name: Option<String>,
    pub deck_card_count: Option<u32>,
    /// Names of the cards loaded into the command zone (empty outside Commander).
    pub commanders: Vec<String>,
}

/// A room as the REST reads and the socket's `lobby` frame describe it — one shape for both
/// so the lobby page renders the same thing whether it polled or was pushed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayRoomSummary"))]
pub struct RoomSummary {
    pub id: i32,
    pub code: String,
    pub game: String,
    pub label: String,
    pub format: String,
    pub starting_life: i32,
    pub max_players: i32,
    pub status: RoomStatus,
    pub seats: Vec<SeatView>,
    /// The seat held by whoever this summary was built for (`None` when built for nobody in
    /// particular — a public read by a stranger, or a spectator's lobby frame).
    pub viewer_seat: Option<SeatId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What `POST rooms` and `POST rooms/{code}/join` answer: the room, the caller's seat and the
/// seat token the socket's `hello` needs (shown once — only its hash is stored).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayJoinResponse"))]
pub struct JoinResponse {
    pub room: RoomSummary,
    pub seat: SeatView,
    pub seat_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PlayServerMessage"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent right after a successful hello, and whenever the room is still in its lobby and
    /// something about it changed (a seat joined / left / loaded a deck / readied).
    Lobby {
        room: RoomSummary,
        viewer_seat: Option<SeatId>,
    },
    /// The full table for this viewer (after hello once playing, on `resync`, on start).
    Snapshot {
        snapshot: Snapshot,
    },
    Patch {
        patch: Patch,
    },
    /// Private answer to `look_top` / `search_library`.
    Peek {
        peek: Peek,
    },
    /// A rejected action (or protocol error). `id` echoes the client's action id.
    Error {
        id: Option<u32>,
        code: String,
        message: String,
    },
    Pong,
    /// Sent just before a server-initiated close (room deleted, seat removed).
    Closed {
        reason: String,
    },
}
