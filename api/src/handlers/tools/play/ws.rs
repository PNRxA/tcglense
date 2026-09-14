//! The room socket: one WebSocket per open tab, and the only channel the live table speaks
//! over.
//!
//! The upgrade itself is unauthenticated — there is no place to put a header on a browser
//! WebSocket, so the credential rides the **first frame** instead. A connection must send
//! `hello` within [`HELLO_TIMEOUT`] (else it is closed `4001`); a `seat_token` that doesn't
//! match a seat of this room closes `4003`; no token at all is a **spectator**, which is a
//! first-class thing here (a friend watching the game) and gets exactly the public view.
//!
//! Three properties this loop is responsible for, none of which the engine can enforce:
//!
//! - **Per-connection filtering.** A patch is computed per socket by
//!   [`view::patch_for`](crate::play::view::patch_for), never once and broadcast, because two
//!   viewers of the same action are allowed to see different things. The `peek` answer to a
//!   scry goes to the one socket that asked.
//! - **A flood ceiling.** A token bucket ([`FRAME_RATE`] / s, burst [`FRAME_BURST`]) meters
//!   **every inbound frame**, not just the actions: a `resync` is a whole snapshot built and
//!   sent per frame, so metering only `action` would leave the cheapest frame to send as the
//!   most expensive one to serve. Over the bucket the frame is refused with `too_fast` rather
//!   than dropped silently, and a client that keeps hammering past
//!   [`MAX_CONSECUTIVE_REJECTS`] is closed `4008`. A snapshot request (`resync`, or a repeated
//!   `hello`) is additionally throttled to one per [`RESYNC_INTERVAL`]. Frames are bounded at
//!   [`MAX_FRAME_BYTES`] by the upgrade itself — tungstenite's own default is 64 **MiB**,
//!   which is not a bound a socket outside the HTTP limiters may keep.
//! - **Liveness.** The server pings every [`PING_INTERVAL`] and closes a connection that has
//!   sent nothing at all for [`IDLE_TIMEOUT`] (`1001`), so a peer that vanished without a close
//!   frame stops holding its seat "connected". A dead peer is usually noticed sooner, by the
//!   writer task's send failing.
//!
//! Nothing here ever `unwrap`s on socket I/O: a socket is a remote peer, and every failure
//! mode it has (a half-closed TCP connection, a truncated frame, a client that sends binary)
//! is an ordinary end-of-connection, logged at debug and nothing more.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};

use crate::entities::play_room;
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::require_game;
use crate::play::types::{ClientMessage, RoomStatus, SeatId, ServerMessage};
use crate::state::AppState;

use super::registry::{Outbound, Room};
use super::tokens::seat_by_token;
use super::{load_room, seats_of, summary_from};

/// How long a fresh connection has to say `hello`. Short — the SPA sends it in the `onopen`
/// handler — but not so short that a slow phone on a train loses the race.
///
/// `pub(crate)` and not `const fn`-folded into the loop so the socket tests can drive the
/// timeout path without actually waiting ten seconds.
pub(crate) const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Server-side keepalive: a protocol-level WebSocket ping, which the peer's stack answers
/// without waking its page. The SPA pings on top of that with its own `ping` frame.
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(30);
/// How long a connection may send **nothing at all** before it is closed. Comfortably above
/// the SPA's own 25s ping, so only a peer that has actually gone away trips it — and a peer
/// that has gone away must not keep its seat lit up at the table.
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// How long the writer task is given to flush what is queued once the reader is done.
const WRITER_DRAIN: Duration = Duration::from_secs(5);

/// Sustained **frames** per second one connection may send, and the burst it may spend at once
/// (a fast sequence of taps during a combat step is legitimate; a script is not).
pub(crate) const FRAME_RATE: f64 = 20.0;
pub(crate) const FRAME_BURST: f64 = 40.0;
/// Consecutive refusals before the connection is closed rather than answered — a client that
/// ignores `too_fast` this many times in a row is not a client.
pub(crate) const MAX_CONSECUTIVE_REJECTS: u32 = 200;
/// The shortest gap between two snapshot requests on one connection. A `resync` is the answer
/// to a version gap, which happens once in a while and never twice in a second.
pub(crate) const RESYNC_INTERVAL: Duration = Duration::from_secs(2);
/// The largest inbound message (and frame) the socket accepts. The biggest thing a client
/// legitimately sends is one `action` — a `reorder_top` of a scried pile is a few hundred
/// bytes — so 64 KiB is already generous, and it replaces tungstenite's 64 MiB default,
/// which would let one connection outside the HTTP limiters buffer a lot of memory.
pub(crate) const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Close codes. The `4xxx` (application) ones are read by `lib/playSocket.ts` as "final, do
/// not reconnect"; `CLOSE_INTERNAL` and `CLOSE_IDLE` are deliberately *not* in that range,
/// because both mean "try again", not "your credential is wrong".
const CLOSE_NO_HELLO: u16 = 4001;
const CLOSE_BAD_TOKEN: u16 = 4003;
const CLOSE_FLOODING: u16 = 4008;
/// The lookup behind `hello` failed on our side (a database error). Reporting that as a bad
/// token would tell the SPA to throw the seat away over an outage on this end.
const CLOSE_INTERNAL: u16 = 1011;
/// Nobody there: no frame for [`IDLE_TIMEOUT`].
const CLOSE_IDLE: u16 = 1001;

/// Upgrade to the room socket.
///
/// `GET /api/tools/{game}/play/rooms/{code}/ws`. The room must exist (a 404 otherwise, before
/// the upgrade), but nothing else is checked here — the first frame carries the credential.
pub async fn room_socket(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    Ok(upgrade
        .max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| connection(state, room, socket)))
}

/// One connection, start to finish.
async fn connection(state: AppState, row: play_room::Model, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // The hello handshake happens before anything is registered, so a connection that never
    // greets costs the room nothing at all.
    let hello = match tokio::time::timeout(HELLO_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<ClientMessage>(&text).ok(),
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return,
        Ok(Some(Ok(_))) => None,
        Ok(Some(Err(err))) => {
            tracing::debug!(error = %err, "play socket failed before hello");
            return;
        }
        Err(_) => {
            close_with(&mut sink, CLOSE_NO_HELLO, "no hello").await;
            return;
        }
    };
    let Some(ClientMessage::Hello { seat_token }) = hello else {
        close_with(&mut sink, CLOSE_NO_HELLO, "expected a hello frame").await;
        return;
    };

    // A token that names no seat of this room is a hard stop, not a downgrade to spectator:
    // a stale token must be visible to the player, not silently turn them into an audience.
    let seat: Option<SeatId> = match seat_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(token) => match seat_by_token(&state.db, row.id, token).await {
            Ok(Some(seat)) => Some(seat.id),
            Ok(None) => {
                close_with(&mut sink, CLOSE_BAD_TOKEN, "invalid seat token").await;
                return;
            }
            Err(err) => {
                // Our fault, not theirs: a `4003` would tell the SPA the token is dead and
                // make it drop the seat, which is exactly the wrong move during a blip here.
                tracing::warn!(error = %err, "play socket seat lookup failed");
                close_internal(&mut sink).await;
                return;
            }
        },
        None => None,
    };

    let room = state.play.room_for(&row);
    // `register` hands back the room this socket actually landed on: the sweeper may have
    // evicted the one `room_for` returned in between, and an orphaned room is a table nobody
    // else can reach.
    let (room, conn_id, mut rx) = state.play.register(&room, seat);

    // The writer task owns the sink: every frame the room fans out goes through this one
    // queue, so a slow peer back-pressures itself and never the room's mutex.
    let writer = tokio::spawn(async move {
        while let Some(outbound) = rx.recv().await {
            let (message, close) = match outbound {
                Outbound::Frame(message) => (message, None),
                Outbound::Ping => {
                    if sink.send(Message::Ping(Default::default())).await.is_err() {
                        break;
                    }
                    continue;
                }
                Outbound::Close(message, code) => (message, Some(code)),
            };
            let text = match serde_json::to_string(&message) {
                Ok(text) => text,
                Err(err) => {
                    tracing::debug!(error = %err, "failed to encode a play frame");
                    continue;
                }
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
            if let Some(code) = close {
                close_with(&mut sink, code, "closed").await;
                break;
            }
        }
        let _ = sink.close().await;
    });

    // The opening frame: a lobby is its seat rows, a live table is a snapshot.
    send_opening(&state, &row, &room, seat, conn_id).await;
    announce_presence(&state, &row, &room, seat, true).await;

    read_loop(&state, &row, &room, seat, conn_id, &mut stream).await;

    // Unregistering drops this connection's sender, so the writer drains whatever is still
    // queued — including a final `closed` frame and its close code — and then finishes on its
    // own. The wait is bounded so a peer that has gone away without closing can't pin the task.
    state.play.unregister(&room, conn_id);
    announce_presence(&state, &row, &room, seat, false).await;
    if tokio::time::timeout(WRITER_DRAIN, writer).await.is_err() {
        tracing::debug!(room = row.id, "play socket writer did not drain in time");
    }
}

/// This socket came or went: tell the table (a live one records presence on the seat) or the
/// lobby (which has no table, so its presence *is* the connection set and everyone watching
/// needs a fresh summary to redraw the connected dots).
///
/// A spectator is nobody's presence on a live table, but it changes nothing in the lobby
/// either — so only a seated connection is worth a push.
async fn announce_presence(
    state: &AppState,
    row: &play_room::Model,
    room: &Arc<Room>,
    seat: Option<SeatId>,
    connected: bool,
) {
    let Some(seat) = seat else { return };
    if state.play.set_connected(room, seat, connected).await {
        return;
    }
    // Still a lobby: built through the same `summary_from` every REST write uses, so a dot
    // that lit up because someone opened a tab and one that lit up because they joined are
    // the same frame.
    if let Err(err) = super::push_lobby(state, row).await {
        tracing::debug!(error = %err, "failed to push a lobby presence frame");
    }
}

/// Hello's answer: the lobby summary while the room hasn't started, otherwise the table.
///
/// The snapshot half is built **and enqueued inside the registry, under the state lock**
/// (`send_snapshot`), so a patch applied a moment later can never overtake the snapshot it
/// was computed after — this connection's queue is monotonic by construction.
async fn send_opening(
    state: &AppState,
    row: &play_room::Model,
    room: &Arc<Room>,
    seat: Option<SeatId>,
    conn_id: u64,
) {
    if state.play.send_snapshot(room, conn_id, seat).await {
        return;
    }
    match lobby_frame(state, row, seat).await {
        Ok(message) => state.play.send_one(room, conn_id, message),
        Err(err) => {
            tracing::debug!(error = %err, "failed to build the opening lobby frame");
        }
    }
}

/// The `lobby` frame for one viewer, built through the same [`summary_from`] the REST reads
/// use — so a page that polled and a page that was pushed can never disagree.
async fn lobby_frame(
    state: &AppState,
    row: &play_room::Model,
    seat: Option<SeatId>,
) -> Result<ServerMessage, AppError> {
    let seats = seats_of(&state.db, row.id).await?;
    let mut room = summary_from(state, row, &seats);
    room.viewer_seat = seat;
    Ok(ServerMessage::Lobby {
        room,
        viewer_seat: seat,
    })
}

/// The main read loop: client frames in, engine results (and refusals) out.
async fn read_loop(
    state: &AppState,
    row: &play_room::Model,
    room: &Arc<Room>,
    seat: Option<SeatId>,
    conn_id: u64,
    stream: &mut futures_util::stream::SplitStream<WebSocket>,
) {
    let mut bucket = TokenBucket::new(FRAME_RATE, FRAME_BURST);
    let mut rejects = 0u32;
    let mut last_snapshot: Option<Instant> = None;
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick of an interval fires immediately; skip it so a fresh socket isn't pinged
    // in the same breath as its snapshot.
    ping.tick().await;
    // Reset by every inbound frame, so the deadline survives our own ping ticks: a keepalive
    // the *server* sent is no evidence at all that anyone is still there.
    let mut idle_at = tokio::time::Instant::now() + IDLE_TIMEOUT;

    loop {
        let frame = tokio::select! {
            frame = stream.next() => {
                idle_at = tokio::time::Instant::now() + IDLE_TIMEOUT;
                frame
            }
            _ = ping.tick() => {
                // A ping rides the same outbound queue as everything else, so a dead peer is
                // noticed by the writer task failing rather than here.
                state.play.ping_one(room, conn_id);
                continue;
            }
            _ = tokio::time::sleep_until(idle_at) => {
                state.play.close_one(
                    room,
                    conn_id,
                    ServerMessage::Closed {
                        reason: "no frames for a while".to_string(),
                    },
                    CLOSE_IDLE,
                );
                break;
            }
        };

        // A text frame is parsed *before* it is metered so a refusal can still echo the
        // client's action id; everything else on the wire (binary, a peer's own ping/pong)
        // costs a token too but has no answer. `None` here is "not text at all", `Some(None)`
        // is text this protocol can't read — which is still a frame we had to receive.
        let parsed = match frame {
            Some(Ok(Message::Text(text))) => {
                Some(serde_json::from_str::<ClientMessage>(&text).ok())
            }
            Some(Ok(Message::Close(_))) | None => break,
            Some(Ok(_)) => None,
            Some(Err(err)) => {
                tracing::debug!(error = %err, "play socket read failed");
                break;
            }
        };
        let id = match &parsed {
            Some(Some(ClientMessage::Action { id, .. })) => *id,
            _ => None,
        };

        // Every inbound frame is metered, not just the actions: `resync` answers with a whole
        // snapshot, and `hello`/`ping` still cost a wake-up and a reply.
        if !bucket.take() {
            rejects = rejects.saturating_add(1);
            send_error(
                state,
                room,
                conn_id,
                id,
                "too_fast",
                "slow down — too many frames at once",
            );
            if rejects >= MAX_CONSECUTIVE_REJECTS {
                // The writer sends this then closes the socket with 4008; stop reading.
                state.play.close_one(
                    room,
                    conn_id,
                    ServerMessage::Closed {
                        reason: "too many frames".to_string(),
                    },
                    CLOSE_FLOODING,
                );
                break;
            }
            continue;
        }
        rejects = 0;

        let Some(parsed) = parsed else { continue };
        let Some(message) = parsed else {
            send_error(state, room, conn_id, None, "bad_frame", "unreadable frame");
            continue;
        };

        match message {
            ClientMessage::Ping => state.play.send_one(room, conn_id, ServerMessage::Pong),
            // A second hello is a no-op rather than an error: a reconnecting client that
            // greets an already-greeted socket should just get its state back. Both frames
            // cost a full snapshot, so both go through the same throttle — one snapshot per
            // `RESYNC_INTERVAL`, however often they are asked for.
            ClientMessage::Hello { .. } | ClientMessage::Resync => {
                let now = Instant::now();
                if last_snapshot.is_some_and(|last| now.duration_since(last) < RESYNC_INTERVAL) {
                    send_error(
                        state,
                        room,
                        conn_id,
                        None,
                        "too_fast",
                        "slow down — one resync at a time",
                    );
                    continue;
                }
                last_snapshot = Some(now);
                send_opening(state, row, room, seat, conn_id).await;
            }
            ClientMessage::Start => {
                if let Err(err) = handle_start(state, row, room, seat).await {
                    send_error(state, room, conn_id, None, err.0, &err.1);
                }
            }
            ClientMessage::Action { id, action } => {
                let Some(actor) = seat else {
                    send_error(
                        state,
                        room,
                        conn_id,
                        id,
                        "spectator",
                        "you're watching this table, not playing at it",
                    );
                    continue;
                };
                if let Err(err) = state.play.apply_action(room, conn_id, actor, action).await {
                    send_error(state, room, conn_id, id, err.code(), &err.to_string());
                }
            }
        }
    }
}

/// `start` is its own frame rather than an engine action because it needs the database: the
/// table is built from the seat rows and their stored decklists, which the pure engine has no
/// way to read.
async fn handle_start(
    state: &AppState,
    row: &play_room::Model,
    room: &Arc<Room>,
    seat: Option<SeatId>,
) -> Result<(), (&'static str, String)> {
    let fresh = super::load_room(state, &row.game, &row.code)
        .await
        .map_err(|_| ("no_such_room", "this table is gone".to_string()))?;
    if super::room_status(&fresh).map_err(|_| ("invalid", "unknown room state".to_string()))?
        != RoomStatus::Lobby
    {
        return Err(("not_lobby", "the game has already started".to_string()));
    }
    let seats = seats_of(&state.db, fresh.id)
        .await
        .map_err(|_| ("invalid", "could not read the table".to_string()))?;

    // Host-only, and "host" is the seat whose account owns the room — not merely a signed-in
    // spectator, so someone watching cannot start the game.
    let is_host = seat.is_some_and(|id| {
        seats
            .iter()
            .any(|s| s.id == id && s.user_id == Some(fresh.host_user_id))
    });
    if !is_host {
        return Err(("host_only", "only the host can start the game".to_string()));
    }

    state
        .play
        .start(&state.db, room, &fresh, &seats)
        .await
        .map_err(|err| (err.code(), err.to_string()))
}

/// Send one `error` frame to one connection.
fn send_error(
    state: &AppState,
    room: &Arc<Room>,
    conn_id: u64,
    id: Option<u32>,
    code: &str,
    message: &str,
) {
    state.play.send_one(
        room,
        conn_id,
        ServerMessage::Error {
            id,
            code: code.to_string(),
            message: message.to_string(),
        },
    );
}

/// Close a socket whose handshake failed on **our** side. It gets the same `closed` frame a
/// registered connection would (so the SPA can say why) and a `1011`, deliberately outside the
/// `4xxx` range: the seat token is fine, this end isn't, and the client should come back.
async fn close_internal(sink: &mut futures_util::stream::SplitSink<WebSocket, Message>) {
    let frame = ServerMessage::Closed {
        reason: "internal".to_string(),
    };
    if let Ok(text) = serde_json::to_string(&frame) {
        let _ = sink.send(Message::Text(text.into())).await;
    }
    close_with(sink, CLOSE_INTERNAL, "internal error").await;
}

/// Close a socket we never registered (the handshake failed), best-effort.
async fn close_with(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    code: u16,
    reason: &str,
) {
    let _ = sink
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.to_string().into(),
        })))
        .await;
    let _ = sink.close().await;
}

/// A simple GCRA-ish token bucket, per connection. Not the shared `governor` limiter: that
/// one is keyed by IP or user and lives across requests, whereas this is a property of one
/// socket and dies with it.
struct TokenBucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    fn new(rate: f64, burst: f64) -> Self {
        TokenBucket {
            rate,
            burst,
            tokens: burst,
            last: Instant::now(),
        }
    }

    /// Spend one token, refilling first. `false` when the bucket is empty.
    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bucket_allows_a_burst_then_refills_at_the_rate() {
        let mut bucket = TokenBucket::new(FRAME_RATE, FRAME_BURST);
        for i in 0..40 {
            assert!(bucket.take(), "action {i} is within the burst");
        }
        assert!(!bucket.take(), "the 41st in the same instant is refused");
        // Rewinding the clock stands in for time passing: half a second at 20/s is 10 tokens.
        bucket.last -= Duration::from_millis(500);
        for i in 0..10 {
            assert!(bucket.take(), "refilled token {i}");
        }
        assert!(!bucket.take(), "and no more than the refill");
    }
}
