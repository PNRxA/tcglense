//! The live tables: every room that currently has (or recently had) a socket on it, held in
//! memory, plus the background sweeper that writes them back.
//!
//! **Why in memory at all.** A table action is a socket frame, not a request: tapping a
//! creature or drawing a card must fan out to four other players in microseconds, and every
//! one of them would otherwise cost a `SELECT` + `UPDATE` of a multi-hundred-kilobyte JSON
//! blob. So the authoritative [`RoomState`] lives here behind one mutex per room, the socket
//! loop mutates it directly, and durability is a **debounce**: a dirty room is written back
//! by [`spawn_sweeper`] every [`SWEEP_INTERVAL`]. The cost of that trade is bounded and
//! stated — a crash loses at most a couple of seconds of a manual game, which is the same
//! thing a dropped connection already costs — and the upside is that the hot path touches no
//! database at all.
//!
//! **Hydration is lazy and one-way.** A room enters the map the first time anything touches
//! it (a socket connecting, a REST read asking who's connected), reading `play_rooms.state`
//! if it is set. It leaves again once it has had no connections for [`EVICT_AFTER`] **and has
//! been written back successfully** — a failed write-back keeps the room dirty and in memory
//! rather than trading a live table for a stale row. Connection counts are deliberately not
//! meaningful in the column: hydration zeroes them and `start` seeds them from the live
//! sockets, because presence is a property of this process and nothing else.
//!
//! **One lock order, always.** `table` (the async mutex holding the state) is taken before
//! `connections` (a sync mutex held only long enough to clone the senders), and never the
//! other way round. Outbound frames go to an unbounded channel drained by a per-connection
//! writer task in [`super::ws`], so a slow client can never stall the room's mutex.
//!
//! The lobby is the one state that is *not* here: a room in `lobby` has no `RoomState` (it is
//! its seat rows), so the registry only carries its connections and pushes
//! [`RoomSummary`] frames at them when a REST write changes something.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};

use crate::entities::prelude::PlayRoom;
use crate::entities::{play_room, play_seat};
use crate::error::AppError;
use crate::play::engine::{self, ActionError};
use crate::play::rng::PlayRng;
#[cfg(test)]
use crate::play::types::Snapshot;
use crate::play::types::{Action, RoomState, RoomStatus, RoomSummary, SeatId, ServerMessage};
use crate::play::view;

/// How often dirty rooms are written back (and idle ones evicted).
pub(crate) const SWEEP_INTERVAL: Duration = Duration::from_secs(2);
/// How long a room with no connections stays in memory before it is persisted and dropped.
pub(crate) const EVICT_AFTER: Duration = Duration::from_secs(10 * 60);

/// The close code every server-initiated, final close uses (the room is gone, or the seat
/// was removed). The SPA treats any `4xxx` as "don't reconnect" — see `lib/playSocket.ts`.
pub(crate) const CLOSE_ROOM_GONE: u16 = 4004;

/// What the writer task of one connection is asked to do. Almost always just "send this
/// frame"; [`Outbound::Close`] is the server saying goodbye, and carries the close code the
/// socket should end on so the SPA can tell *why* (`4004` room gone, `4008` flooding — see
/// [`super::ws`]) rather than seeing an anonymous drop.
pub(crate) enum Outbound {
    Frame(ServerMessage),
    /// A protocol-level WebSocket ping (the keepalive the server sends of its own accord).
    /// It rides the same queue as the frames so a dead peer is noticed by the writer task
    /// failing, never by the read loop guessing.
    Ping,
    Close(ServerMessage, u16),
}

/// One open socket on a room.
struct Connection {
    id: u64,
    /// The seat this connection holds, or `None` for a spectator.
    seat: Option<SeatId>,
    tx: tokio::sync::mpsc::UnboundedSender<Outbound>,
}

/// A room's connection set, plus when it last became empty (for eviction).
struct Connections {
    entries: Vec<Connection>,
    empty_since: Option<Instant>,
}

impl Connections {
    fn new() -> Self {
        Connections {
            entries: Vec::new(),
            empty_since: Some(Instant::now()),
        }
    }
}

/// The mutable half of a live room: the table itself (absent while it is a lobby), the RNG
/// that shuffles it, and whether it has changed since the last sweep.
struct Table {
    state: Option<RoomState>,
    /// Built on first use, never persisted — a shuffle only has to be fair, not reproducible
    /// (see [`crate::play::rng`]). Lazy so a lobby-only room never pays for it.
    rng: Option<PlayRng>,
    dirty: bool,
}

/// One live room.
pub(crate) struct Room {
    pub id: i32,
    table: tokio::sync::Mutex<Table>,
    connections: Mutex<Connections>,
}

impl Room {
    fn new(id: i32, state: Option<RoomState>) -> Self {
        Room {
            id,
            table: tokio::sync::Mutex::new(Table {
                state,
                rng: None,
                dirty: false,
            }),
            connections: Mutex::new(Connections::new()),
        }
    }

    /// A snapshot of the table for `viewer`, or `None` while the room is a lobby.
    ///
    /// Test-only: the socket's own opening frame goes through
    /// [`PlayRegistry::send_snapshot`], which builds *and* enqueues it under the state lock so
    /// nothing can overtake it. Reading one out on its own is only ever a test asking what the
    /// table holds.
    #[cfg(test)]
    pub(crate) async fn snapshot_for(&self, viewer: Option<SeatId>) -> Option<Snapshot> {
        let table = self.table.lock().await;
        table
            .state
            .as_ref()
            .map(|state| view::snapshot_for(state, viewer))
    }

    /// The seats with at least one live socket.
    fn connected_seats(&self) -> HashSet<SeatId> {
        let entries = self.connections.lock().unwrap_or_else(|e| e.into_inner());
        entries.entries.iter().filter_map(|c| c.seat).collect()
    }

    /// How many sockets that seat holds right now.
    fn seat_connection_count(&self, seat: SeatId) -> usize {
        let entries = self.connections.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .entries
            .iter()
            .filter(|c| c.seat == Some(seat))
            .count()
    }
}

/// Every live room, keyed by `play_rooms.id`.
pub struct PlayRegistry {
    rooms: Mutex<HashMap<i32, Arc<Room>>>,
    next_connection_id: AtomicU64,
}

impl Default for PlayRegistry {
    fn default() -> Self {
        PlayRegistry {
            rooms: Mutex::new(HashMap::new()),
            next_connection_id: AtomicU64::new(1),
        }
    }
}

impl PlayRegistry {
    /// The live room for `row`, hydrating it from `play_rooms.state` on first touch.
    ///
    /// Hydration is best-effort: a `state` column that no longer parses (a schema change
    /// under a persisted table) yields a room with no table rather than a failed request —
    /// the host can start a new game, which is the only honest recovery.
    pub(crate) fn room_for(&self, row: &play_room::Model) -> Arc<Room> {
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(room) = rooms.get(&row.id) {
            return room.clone();
        }
        let state = row.state.as_deref().and_then(|json| {
            serde_json::from_str::<RoomState>(json)
                .inspect_err(|err| {
                    tracing::warn!(room = row.id, error = %err, "discarding unreadable play room state");
                })
                .ok()
        });
        // Presence is a property of *this* process: the live sockets are the only source, and
        // a hydrated room has none. The column may well carry the counts the table had when it
        // was last swept, so they are zeroed here rather than trusted — otherwise a restart
        // would show a pod of players who are all, in fact, gone.
        let state = state.map(|mut state: RoomState| {
            for seat in &mut state.seats {
                seat.connections = 0;
            }
            state
        });
        let room = Arc::new(Room::new(row.id, state));
        rooms.insert(row.id, room.clone());
        room
    }

    /// The live room already in memory for `room_id`, if any (never hydrates).
    fn live(&self, room_id: i32) -> Option<Arc<Room>> {
        self.rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&room_id)
            .cloned()
    }

    /// Which seats of `room_id` have a live socket. Empty for a room nobody has opened, which
    /// is exactly right: presence is a property of the process, not of the row.
    pub(crate) fn connected_seats(&self, room_id: i32) -> HashSet<SeatId> {
        self.live(room_id)
            .map(|room| room.connected_seats())
            .unwrap_or_default()
    }

    /// Register a new socket and return the room it actually landed on, its id, and the
    /// receiving half of its outbound queue.
    ///
    /// The room comes back because the `Arc` the caller was handed may have stopped being the
    /// live entry in between: [`room_for`](Self::room_for) and this call are two lock
    /// acquisitions, and a sweep can evict an idle room between them. Re-validating under the
    /// map lock is what stops a reconnecting socket from being attached to an orphan — a room
    /// nothing else can reach, whose patches nobody else would ever see.
    pub(crate) fn register(
        &self,
        room: &Arc<Room>,
        seat: Option<SeatId>,
    ) -> (
        Arc<Room>,
        u64,
        tokio::sync::mpsc::UnboundedReceiver<Outbound>,
    ) {
        let id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        let live = match rooms.get(&room.id) {
            // Still the live entry: the ordinary case.
            Some(existing) if Arc::ptr_eq(existing, room) => room.clone(),
            // Someone else re-hydrated the room while we were away — join *that* table.
            Some(existing) => existing.clone(),
            // The sweeper evicted it; this handle is still the freshest thing anyone has, so
            // put it back rather than hydrating a second copy of the same table.
            None => {
                rooms.insert(room.id, room.clone());
                room.clone()
            }
        };
        {
            let mut entries = live.connections.lock().unwrap_or_else(|e| e.into_inner());
            entries.entries.push(Connection { id, seat, tx });
            entries.empty_since = None;
        }
        (live, id, rx)
    }

    /// Drop a socket. The room stays in memory until the sweeper ages it out.
    pub(crate) fn unregister(&self, room: &Arc<Room>, conn_id: u64) {
        let mut entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        entries.entries.retain(|c| c.id != conn_id);
        if entries.entries.is_empty() {
            entries.empty_since = Some(Instant::now());
        }
    }

    /// Push a fresh lobby frame at everyone watching `room_id` — called by every REST write
    /// that changes who is at the table, so a lobby page never has to poll.
    pub(crate) fn push_lobby(&self, room_id: i32, summary: RoomSummary) {
        let Some(room) = self.live(room_id) else {
            return;
        };
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        for conn in &entries.entries {
            let _ = conn.tx.send(Outbound::Frame(ServerMessage::Lobby {
                room: RoomSummary {
                    viewer_seat: conn.seat,
                    ..summary.clone()
                },
                viewer_seat: conn.seat,
            }));
        }
    }

    /// Send one frame to one connection (silently dropped if its writer task has already
    /// gone — a socket that closed mid-fan-out is an ordinary outcome, not an error).
    pub(crate) fn send_one(&self, room: &Arc<Room>, conn_id: u64, message: ServerMessage) {
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = entries.entries.iter().find(|c| c.id == conn_id) {
            let _ = conn.tx.send(Outbound::Frame(message));
        }
    }

    /// Send a protocol-level ping to one connection (the server's own keepalive).
    pub(crate) fn ping_one(&self, room: &Arc<Room>, conn_id: u64) {
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = entries.entries.iter().find(|c| c.id == conn_id) {
            let _ = conn.tx.send(Outbound::Ping);
        }
    }

    /// Build this connection's [`Snapshot`] **and enqueue it while the state lock is still
    /// held**, answering `false` when the room is still a lobby (which has no table, and whose
    /// opening frame the caller builds from the seat rows instead).
    ///
    /// The two halves are inseparable on purpose: building the snapshot, dropping the lock and
    /// *then* sending it would let an action applied in between put its patch on this
    /// connection's queue ahead of the older snapshot, and the client would fold a newer patch
    /// into a staler view. Under the lock the queue is monotonic by construction.
    pub(crate) async fn send_snapshot(
        &self,
        room: &Arc<Room>,
        conn_id: u64,
        viewer: Option<SeatId>,
    ) -> bool {
        let table = room.table.lock().await;
        let Some(state) = table.state.as_ref() else {
            return false;
        };
        let snapshot = view::snapshot_for(state, viewer);
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = entries.entries.iter().find(|c| c.id == conn_id) {
            let _ = conn
                .tx
                .send(Outbound::Frame(ServerMessage::Snapshot { snapshot }));
        }
        true
    }

    /// Send one last frame to one connection and close it with `code`.
    pub(crate) fn close_one(
        &self,
        room: &Arc<Room>,
        conn_id: u64,
        message: ServerMessage,
        code: u16,
    ) {
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = entries.entries.iter().find(|c| c.id == conn_id) {
            let _ = conn.tx.send(Outbound::Close(message, code));
        }
    }

    /// Apply one seat's action to the table and fan the resulting patch out to every
    /// connection **as that connection is allowed to see it** — this is the only place a
    /// table delta reaches the wire, so it is the choke point that keeps a library order or
    /// someone else's hand off it.
    ///
    /// The private `peek` answer (a scry, a library search) goes to the one socket that asked,
    /// not to the seat: two tabs of the same player are two viewers, and only the one that
    /// pressed the button is expecting an answer.
    pub(crate) async fn apply_action(
        &self,
        room: &Arc<Room>,
        conn_id: u64,
        actor: SeatId,
        action: Action,
    ) -> Result<(), ActionError> {
        let mut table = room.table.lock().await;
        let Table { state, rng, dirty } = &mut *table;
        let Some(state) = state.as_mut() else {
            return Err(ActionError::NotPlaying);
        };
        let rng = rng.get_or_insert_with(PlayRng::from_entropy);
        let outcome = engine::apply(state, actor, action, rng, Utc::now())?;
        *dirty = true;

        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        for conn in &entries.entries {
            let patch = view::patch_for(state, &outcome.changes, conn.seat);
            let _ = conn
                .tx
                .send(Outbound::Frame(ServerMessage::Patch { patch }));
        }
        if let Some(peek) = outcome.peek
            && let Some(conn) = entries.entries.iter().find(|c| c.id == conn_id)
        {
            let _ = conn.tx.send(Outbound::Frame(ServerMessage::Peek { peek }));
        }
        Ok(())
    }

    /// Lobby -> playing: build the table from the seats' loaded decks, deal, persist, and send
    /// every connection its own [`Snapshot`].
    ///
    /// The build happens here rather than in the engine because it is the one step that needs
    /// the database (the seats and their stored decklists); everything after it is pure.
    pub(crate) async fn start(
        &self,
        db: &DatabaseConnection,
        room: &Arc<Room>,
        row: &play_room::Model,
        seats: &[play_seat::Model],
    ) -> Result<(), ActionError> {
        let mut table = room.table.lock().await;
        // The lobby gate again, this time under the lock that decides it. The caller checked
        // the row, but two `start` frames can pass that check together (a double-click, two
        // host tabs) and the second must not deal a second opening hand over the first.
        if table.state.is_some() {
            return Err(ActionError::NotLobby);
        }
        let seat_states = seats
            .iter()
            .map(|seat| {
                engine::new_seat_state(
                    seat.id,
                    seat.seat_index,
                    seat.display_name.clone(),
                    seat.user_id == Some(row.host_user_id),
                    super::seat_deck(seat),
                    seat.deck_name.clone(),
                    row.starting_life,
                )
            })
            .collect();
        let mut state = engine::new_room_state(&row.format, row.starting_life, seat_states);
        // Presence carries over from the lobby: everyone watching already has a socket, and a
        // table that opened with every seat greyed out would be lying about the room it was
        // just built from. The count is seeded as a flag (0 or 1) because the registry only
        // ever tells the engine about the 0<->1 transitions — a second tab is not a second
        // player — so anything larger could never be decremented back to "gone".
        let live = room.connected_seats();
        for seat in &mut state.seats {
            seat.connections = u32::from(live.contains(&seat.id));
        }
        let rng = table.rng.get_or_insert_with(PlayRng::from_entropy);
        engine::start_game(&mut state, rng, Utc::now())?;

        // A started game is the one transition worth a synchronous write: the status change
        // is what every later REST read and every reconnect resolves against, so it must not
        // wait on the sweeper. It is also the one write whose failure is *not* survivable —
        // a table installed over a row that still says `lobby` would deal hands the next
        // reconnect could never resolve against — so the table is installed only after it.
        if !persist(db, row.id, &state, RoomStatus::Playing).await {
            return Err(ActionError::Invalid(
                "couldn't save the table; try starting again",
            ));
        }

        // Sent before the state is installed rather than after, so no `expect` is needed to
        // read back what we just built — a request path never panics.
        {
            let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
            for conn in &entries.entries {
                let _ = conn.tx.send(Outbound::Frame(ServerMessage::Snapshot {
                    snapshot: view::snapshot_for(&state, conn.seat),
                }));
            }
        }
        table.state = Some(state);
        table.dirty = false;
        Ok(())
    }

    /// A socket for `seat` came or went. Only the 0<->1 transitions are interesting (a second
    /// tab is not a second player), so the count is consulted before the engine is told.
    ///
    /// Answers **`false` when the room is still a lobby**: a lobby has no table to record
    /// presence on — its connection set *is* the presence — so the caller pushes a `lobby`
    /// frame instead, which is how the lobby's connected dots light up and go out.
    pub(crate) async fn set_connected(
        &self,
        room: &Arc<Room>,
        seat: SeatId,
        connected: bool,
    ) -> bool {
        let mut table = room.table.lock().await;
        let Table { state, dirty, .. } = &mut *table;
        let Some(state) = state.as_mut() else {
            return false;
        };
        let count = room.seat_connection_count(seat);
        // `register` runs before this, `unregister` before it too: so "connected" means the
        // seat just went from 0 to 1, and "disconnected" means it has none left.
        let transition = if connected { count == 1 } else { count == 0 };
        if !transition {
            return true;
        }
        match engine::set_connected(state, seat, connected, Utc::now()) {
            Ok(changes) => {
                *dirty = true;
                let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
                for conn in &entries.entries {
                    let patch = view::patch_for(state, &changes, conn.seat);
                    let _ = conn
                        .tx
                        .send(Outbound::Frame(ServerMessage::Patch { patch }));
                }
            }
            Err(err) => {
                tracing::debug!(room = room.id, seat, error = %err, "presence update refused");
            }
        }
        true
    }

    /// Close every socket on a room and forget it. Each connection is told *why* first
    /// ([`ServerMessage::Closed`]); the writer task then closes the socket with
    /// [`CLOSE_ROOM_GONE`], which the SPA reads as "final, don't reconnect".
    pub(crate) fn close_room(&self, room_id: i32, reason: &str) {
        let Some(room) = self.live(room_id) else {
            return;
        };
        {
            let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
            for conn in &entries.entries {
                let _ = conn.tx.send(Outbound::Close(
                    ServerMessage::Closed {
                        reason: reason.to_string(),
                    },
                    CLOSE_ROOM_GONE,
                ));
            }
        }
        self.rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&room_id);
    }

    /// Test-only: drop a room from the map without touching its sockets, standing in for the
    /// sweeper's eviction so a test can prove that what comes back out of `play_rooms.state`
    /// is the table that was played.
    #[cfg(test)]
    pub(crate) fn forget(&self, room_id: i32) {
        self.rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&room_id);
    }

    /// Close the sockets of one seat (it was removed from the table) without disturbing the
    /// rest of the room.
    pub(crate) fn close_seat(&self, room_id: i32, seat: SeatId, reason: &str) {
        let Some(room) = self.live(room_id) else {
            return;
        };
        let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
        for conn in entries.entries.iter().filter(|c| c.seat == Some(seat)) {
            let _ = conn.tx.send(Outbound::Close(
                ServerMessage::Closed {
                    reason: reason.to_string(),
                },
                CLOSE_ROOM_GONE,
            ));
        }
    }

    /// One sweep: write back every dirty table, then drop the rooms that have been empty for
    /// [`EVICT_AFTER`]. Called on a timer by [`spawn_sweeper`], and directly by tests.
    ///
    /// **A room is evicted only after it has actually been persisted.** A write that fails
    /// leaves the room dirty *and* in the map, so the next tick tries again: dropping it would
    /// throw the live table away in favour of a row that is minutes stale, which is the one
    /// way this debounce could lose a whole game rather than a couple of seconds of one.
    pub(crate) async fn sweep(&self, db: &DatabaseConnection) {
        let live: Vec<(i32, Arc<Room>)> = {
            let rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
            rooms.iter().map(|(id, room)| (*id, room.clone())).collect()
        };

        let mut evict = Vec::new();
        for (id, room) in live {
            let idle = {
                let entries = room.connections.lock().unwrap_or_else(|e| e.into_inner());
                entries.entries.is_empty()
                    && entries
                        .empty_since
                        .is_some_and(|since| since.elapsed() >= EVICT_AFTER)
            };

            let mut table = room.table.lock().await;
            let mut persisted = true;
            if table.dirty
                && let Some(state) = table.state.as_ref()
            {
                persisted = persist(db, id, state, state.status).await;
                if persisted {
                    table.dirty = false;
                } else {
                    tracing::warn!(
                        room = id,
                        "play room stayed dirty; keeping it in memory for the next sweep"
                    );
                }
            }
            drop(table);

            if idle && persisted {
                evict.push(id);
            }
        }

        if !evict.is_empty() {
            let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
            for id in evict {
                // Re-check under the map lock: a socket may have arrived since the scan.
                let still_idle = rooms.get(&id).is_some_and(|room| {
                    room.connections
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .entries
                        .is_empty()
                });
                if still_idle {
                    rooms.remove(&id);
                }
            }
        }
    }
}

/// Write one table back to its row, answering whether the row now holds it. A failure is
/// logged rather than fatal — the live table is still correct — but it is *reported*, because
/// the caller's next move depends on it: the sweeper keeps the room dirty and in memory, and
/// `start` refuses to install a table whose `playing` status never reached the row.
async fn persist(
    db: &DatabaseConnection,
    room_id: i32,
    state: &RoomState,
    status: RoomStatus,
) -> bool {
    let json = match serde_json::to_string(state) {
        Ok(json) => json,
        Err(err) => {
            tracing::warn!(room = room_id, error = %err, "failed to serialize play room state");
            return false;
        }
    };
    let update = play_room::ActiveModel {
        id: Set(room_id),
        state: Set(Some(json)),
        status: Set(status.as_str().to_string()),
        updated_at: Set(Utc::now()),
        ..Default::default()
    };
    if let Err(err) = update.update(db).await {
        tracing::warn!(room = room_id, error = %err, "failed to persist play room state");
        return false;
    }
    true
}

/// Touch a room's `updated_at` (the listing's sort key) after a REST write that changed its
/// seats rather than its table.
pub(crate) async fn touch_room<C: sea_orm::ConnectionTrait>(
    db: &C,
    room_id: i32,
) -> Result<(), AppError> {
    use sea_orm::{ColumnTrait, QueryFilter};
    PlayRoom::update_many()
        .col_expr(
            play_room::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(Utc::now()),
        )
        .filter(play_room::Column::Id.eq(room_id))
        .exec(db)
        .await?;
    Ok(())
}

/// The persistence + eviction loop. Started from [`crate::tasks::start`] rather than wired
/// into the router, so it is not part of the request path and does not run in the drained
/// maintenance router.
pub fn spawn_sweeper(registry: Arc<PlayRegistry>, db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            registry.sweep(&db).await;
        }
    });
}
