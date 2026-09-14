//! The play table's **socket** (`/api/tools/{game}/play/rooms/{code}/ws`) — the half of the
//! tool that `tower::oneshot` cannot reach, because an HTTP upgrade needs a real connection.
//! These bind the real router on `127.0.0.1:0` with `axum::serve` and drive it with
//! `tokio-tungstenite`, so the handshake, the per-connection filtering and the close codes are
//! exercised end to end.
//!
//! What they are here to pin down is the one property the whole feature rests on: **hidden
//! information stays hidden**. The server is authoritative, so a client only knows what it is
//! sent — which means a bug that puts an opponent's hand in a patch is not a rendering
//! mistake, it is the game being unplayable. Every assertion about `hand` / `hand_count` /
//! library ids below is that rule, checked from the outside.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";
/// A seeded precon with a command zone — a real decklist for a seat to sit down with.
const COMMANDER_SLUG: &str = "dummy-universe-commander-dmu";

// ---------- A real server ----------

/// The bound router plus the address it is listening on. The serving task is aborted when
/// this is dropped, so a test never leaks a listener.
struct Served {
    app: TestApp,
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(app: TestApp) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let router = app.router.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Served { app, addr, task }
}

/// One client socket, with the two operations every test needs: send a `ClientMessage`, and
/// read the next `ServerMessage` (as JSON, so an added field never breaks a test).
struct Client {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Client {
    async fn connect(served: &Served, code: &str) -> Client {
        let url = format!("ws://{}/api/tools/mtg/play/rooms/{code}/ws", served.addr);
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("websocket upgrade");
        Client { socket }
    }

    async fn send(&mut self, message: Value) {
        self.socket
            .send(Message::text(message.to_string()))
            .await
            .expect("send frame");
    }

    /// Say hello and return the first frame back.
    async fn hello(&mut self, seat_token: Option<&str>) -> Value {
        self.send(json!({ "type": "hello", "seat_token": seat_token }))
            .await;
        self.next().await
    }

    /// The next server frame, failing the test rather than hanging if none arrives.
    async fn next(&mut self) -> Value {
        self.try_next().await.expect("expected a server frame")
    }

    async fn try_next(&mut self) -> Option<Value> {
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(5), self.socket.next())
                .await
                .expect("timed out waiting for a frame")?;
            match frame.expect("socket error") {
                Message::Text(text) => {
                    return Some(serde_json::from_str(&text).expect("a JSON frame"));
                }
                // A close is reported as its own pseudo-frame so a test can assert the code.
                Message::Close(frame) => {
                    return Some(json!({
                        "type": "__close",
                        "code": frame.as_ref().map(|f| u16::from(f.code)),
                    }));
                }
                _ => continue,
            }
        }
    }

    /// Skip frames until one of `type` arrives (presence patches ride the same stream).
    async fn next_of(&mut self, kind: &str) -> Value {
        for _ in 0..20 {
            let frame = self.next().await;
            if frame["type"] == kind {
                return frame;
            }
        }
        panic!("no `{kind}` frame arrived");
    }
}

// ---------- Fixtures ----------

/// Open a table and return `(code, host seat token, host seat id)`.
async fn open_table(app: &TestApp, access: &str) -> (String, String, i64) {
    let (status, _, join) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/play/rooms",
            access,
            json!({ "format": "commander", "max_players": 4 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create room: {join:?}");
    (
        join["room"]["code"].as_str().expect("code").to_string(),
        join["seat_token"].as_str().expect("token").to_string(),
        join["seat"]["id"].as_i64().expect("seat id"),
    )
}

/// Sit a guest down and return `(seat token, seat id)`.
async fn seat_guest(app: &TestApp, code: &str, name: &str) -> (String, i64) {
    let (status, _, join) = send(
        app,
        json_post(
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            json!({ "name": name }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "join: {join:?}");
    (
        join["seat_token"].as_str().expect("token").to_string(),
        join["seat"]["id"].as_i64().expect("seat id"),
    )
}

/// Load the seeded Commander precon into a seat.
async fn load_precon(app: &TestApp, code: &str, seat: i64, token: &str) {
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/tools/mtg/play/rooms/{code}/seats/{seat}/deck"
        ))
        .header("x-play-seat", token)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "source": "precon", "slug": COMMANDER_SLUG }).to_string(),
        ))
        .unwrap();
    let (status, _, body) = send(app, req).await;
    assert_eq!(status, StatusCode::OK, "load precon: {body:?}");
}

/// A two-seat table with both decks loaded, served and ready to start.
async fn table_ready() -> (Served, String, (String, i64), (String, i64)) {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, host_token, host_seat) = open_table(&app, &access).await;
    let (guest_token, guest_seat) = seat_guest(&app, &code, "Bob").await;
    load_precon(&app, &code, host_seat, &host_token).await;
    load_precon(&app, &code, guest_seat, &guest_token).await;
    let served = serve(app).await;
    (
        served,
        code,
        (host_token, host_seat),
        (guest_token, guest_seat),
    )
}

/// The seat entry of a snapshot / patch by id.
fn seat_in<'a>(frame: &'a Value, key: &str, seat: i64) -> &'a Value {
    frame[key]["seats"]
        .as_array()
        .expect("seats")
        .iter()
        .find(|s| s["id"].as_i64() == Some(seat))
        .unwrap_or_else(|| panic!("seat {seat} missing from the {key}"))
}

// ---------- The handshake ----------

#[tokio::test]
async fn a_socket_without_a_token_is_a_spectator_in_the_lobby() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, _, _) = open_table(&app, &access).await;
    let served = serve(app).await;

    let mut client = Client::connect(&served, &code).await;
    let frame = client.hello(None).await;
    assert_eq!(frame["type"], "lobby", "a lobby room opens with its seats");
    assert_eq!(
        frame["viewer_seat"],
        Value::Null,
        "no token means no seat: {frame:?}"
    );
    assert_eq!(frame["room"]["code"], code);
    assert_eq!(frame["room"]["status"], "lobby");
}

#[tokio::test]
async fn a_socket_with_a_seat_token_is_that_seat() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, token, seat) = open_table(&app, &access).await;
    let served = serve(app).await;

    let mut client = Client::connect(&served, &code).await;
    let frame = client.hello(Some(&token)).await;
    assert_eq!(frame["type"], "lobby");
    assert_eq!(frame["viewer_seat"], seat);

    // A live socket is what "connected" means — it is not a column, it is this.
    let seats = frame["room"]["seats"].as_array().expect("seats");
    let me = seats
        .iter()
        .find(|s| s["id"].as_i64() == Some(seat))
        .expect("my seat");
    // The opening frame is built before this connection is registered, so presence shows up
    // on the next lobby push; what matters here is that the field exists and is a bool.
    assert!(me["connected"].is_boolean());
}

#[tokio::test]
async fn a_stale_seat_token_closes_the_socket_rather_than_demoting_it() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, _, _) = open_table(&app, &access).await;
    let served = serve(app).await;

    let mut client = Client::connect(&served, &code).await;
    let frame = client.hello(Some("not-a-real-token")).await;
    assert_eq!(frame["type"], "__close", "expected a close: {frame:?}");
    assert_eq!(
        frame["code"], 4003,
        "a bad token is its own close code, not a silent downgrade to spectator"
    );
}

#[tokio::test]
async fn a_first_frame_that_is_not_a_hello_closes_the_socket() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, _, _) = open_table(&app, &access).await;
    let served = serve(app).await;

    let mut client = Client::connect(&served, &code).await;
    // Skipping the handshake and going straight to an action is the same failure as never
    // greeting at all — checked here instead of the ten-second timeout so the test is quick.
    client
        .send(json!({ "type": "action", "action": { "type": "draw", "n": 1 } }))
        .await;
    let frame = client.next().await;
    assert_eq!(frame["type"], "__close");
    assert_eq!(frame["code"], 4001);
}

// ---------- Starting, and what each viewer may see ----------

#[tokio::test]
async fn starting_the_game_deals_each_seat_a_hand_only_it_can_see() {
    let (served, code, (host_token, host_seat), (guest_token, guest_seat)) = table_ready().await;

    let mut host = Client::connect(&served, &code).await;
    assert_eq!(host.hello(Some(&host_token)).await["type"], "lobby");
    let mut guest = Client::connect(&served, &code).await;
    assert_eq!(guest.hello(Some(&guest_token)).await["type"], "lobby");

    host.send(json!({ "type": "start" })).await;

    let host_snapshot = host.next_of("snapshot").await;
    let guest_snapshot = guest.next_of("snapshot").await;
    assert_eq!(host_snapshot["snapshot"]["status"], "playing");

    // Each seat has seven cards, and every seat's *count* is public…
    for (frame, seat) in [(&host_snapshot, host_seat), (&guest_snapshot, guest_seat)] {
        for other in [host_seat, guest_seat] {
            assert_eq!(
                seat_in(frame, "snapshot", other)["hand_count"],
                7,
                "every seat's hand size is public"
            );
        }
        // …but only the viewer's own hand carries ids.
        assert_eq!(
            seat_in(frame, "snapshot", seat)["hand"]
                .as_array()
                .expect("own hand")
                .len(),
            7,
            "a seat sees its own hand"
        );
    }
    let opponent_hand = seat_in(&host_snapshot, "snapshot", guest_seat)["hand"]
        .as_array()
        .expect("the opponent's hand field");
    assert!(
        opponent_hand.is_empty(),
        "an opponent's hand is a count, never ids: {opponent_hand:?}"
    );

    // And no library card is anywhere in the payload — the library is a count on both sides.
    for frame in [&host_snapshot, &guest_snapshot] {
        let libraries: Vec<&Value> = frame["snapshot"]["cards"]
            .as_array()
            .expect("cards")
            .iter()
            .filter(|c| c["zone"] == "library")
            .collect();
        assert!(
            libraries.is_empty(),
            "a library's order is the secret: {libraries:?}"
        );
        assert!(
            seat_in(frame, "snapshot", host_seat)["library_count"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    }

    // The ids the host was dealt are not in the guest's snapshot at all (not merely hidden).
    let host_hand: Vec<i64> = seat_in(&host_snapshot, "snapshot", host_seat)["hand"]
        .as_array()
        .expect("hand")
        .iter()
        .filter_map(|id| id.as_i64())
        .collect();
    let guest_card_ids: Vec<i64> = guest_snapshot["snapshot"]["cards"]
        .as_array()
        .expect("cards")
        .iter()
        .filter_map(|c| c["id"].as_i64())
        .collect();
    for id in &host_hand {
        assert!(
            !guest_card_ids.contains(id),
            "card {id} from the host's hand leaked into the guest's snapshot"
        );
    }
}

#[tokio::test]
async fn a_draw_patches_both_seats_but_only_one_learns_the_card() {
    let (served, code, (host_token, host_seat), (guest_token, guest_seat)) = table_ready().await;

    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&host_token)).await;
    let mut guest = Client::connect(&served, &code).await;
    guest.hello(Some(&guest_token)).await;

    host.send(json!({ "type": "start" })).await;
    let started = host.next_of("snapshot").await;
    guest.next_of("snapshot").await;
    let version = started["snapshot"]["version"].as_u64().expect("version");

    host.send(json!({
        "type": "action",
        "id": 1,
        "action": { "type": "draw", "n": 1 }
    }))
    .await;

    let host_patch = host.next_of("patch").await;
    let guest_patch = guest.next_of("patch").await;

    // The version is a total order: exactly one step per applied action, the same number for
    // everyone, which is what lets a client detect a gap and ask to resync.
    assert_eq!(host_patch["patch"]["version"], version + 1);
    assert_eq!(guest_patch["patch"]["version"], version + 1);

    // The drawer's hand grew by one and it knows which card…
    let mine = seat_in(&host_patch, "patch", host_seat);
    assert_eq!(mine["hand_count"], 8);
    assert_eq!(mine["hand"].as_array().expect("hand").len(), 8);
    assert!(
        host_patch["patch"]["cards"]
            .as_array()
            .expect("cards")
            .iter()
            .any(|c| c["zone"] == "hand" && c["def"].is_object()),
        "the drawn card arrives in full for the seat that drew it: {host_patch:?}"
    );

    // …while the opponent learns only that the count changed.
    let theirs = seat_in(&guest_patch, "patch", host_seat);
    assert_eq!(theirs["hand_count"], 8, "the count is public");
    assert!(
        theirs["hand"].as_array().expect("hand").is_empty(),
        "and nothing else is: {theirs:?}"
    );
    assert!(
        !guest_patch["patch"]["cards"]
            .as_array()
            .expect("cards")
            .iter()
            .any(|c| c["zone"] == "hand"),
        "no hand card of another seat rides an opponent's patch: {guest_patch:?}"
    );
    let _ = guest_seat;
}

#[tokio::test]
async fn a_spectator_sees_the_table_but_no_hand_and_cannot_act() {
    let (served, code, (host_token, host_seat), (guest_token, _)) = table_ready().await;

    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&host_token)).await;
    let mut guest = Client::connect(&served, &code).await;
    guest.hello(Some(&guest_token)).await;
    let mut watcher = Client::connect(&served, &code).await;
    watcher.hello(None).await;

    host.send(json!({ "type": "start" })).await;
    let seen = watcher.next_of("snapshot").await;
    host.next_of("snapshot").await;

    // A spectator sees the table exactly as an opponent does: counts, no ids.
    for seat in seen["snapshot"]["seats"].as_array().expect("seats") {
        assert_eq!(seat["hand_count"], 7);
        assert!(
            seat["hand"].as_array().expect("hand").is_empty(),
            "a spectator holds no seat, so no hand is theirs to see: {seat:?}"
        );
    }
    assert_eq!(seen["snapshot"]["viewer_seat"], Value::Null);

    // And watching is not playing.
    watcher
        .send(json!({
            "type": "action",
            "id": 9,
            "action": { "type": "draw", "n": 1 }
        }))
        .await;
    let error = watcher.next_of("error").await;
    assert_eq!(error["code"], "spectator");
    assert_eq!(
        error["id"], 9,
        "the rejection echoes the client's action id"
    );
    let _ = host_seat;
}

#[tokio::test]
async fn only_the_host_can_start_the_game() {
    let (served, code, (host_token, _), (guest_token, _)) = table_ready().await;

    let mut guest = Client::connect(&served, &code).await;
    guest.hello(Some(&guest_token)).await;
    guest.send(json!({ "type": "start" })).await;
    let error = guest.next_of("error").await;
    assert_eq!(error["code"], "host_only", "a guest starting: {error:?}");

    // The host can, and a second start is refused because the room is no longer a lobby.
    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&host_token)).await;
    host.send(json!({ "type": "start" })).await;
    assert_eq!(
        host.next_of("snapshot").await["snapshot"]["status"],
        "playing"
    );
    host.send(json!({ "type": "start" })).await;
    assert_eq!(host.next_of("error").await["code"], "not_lobby");
}

#[tokio::test]
async fn chat_reaches_the_whole_table_and_ping_is_answered() {
    let (served, code, (host_token, host_seat), (guest_token, _)) = table_ready().await;

    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&host_token)).await;
    let mut guest = Client::connect(&served, &code).await;
    guest.hello(Some(&guest_token)).await;

    host.send(json!({ "type": "ping" })).await;
    assert_eq!(host.next_of("pong").await["type"], "pong");

    host.send(json!({ "type": "start" })).await;
    host.next_of("snapshot").await;
    guest.next_of("snapshot").await;

    host.send(json!({
        "type": "action",
        "action": { "type": "chat", "text": "glhf" }
    }))
    .await;

    for client in [&mut host, &mut guest] {
        let patch = client.next_of("patch").await;
        let log = patch["patch"]["log"].as_array().expect("log");
        assert!(
            log.iter().any(|entry| entry["kind"] == "chat"
                && entry["text"].as_str().unwrap_or_default().contains("glhf")
                && entry["seat"].as_i64() == Some(host_seat)),
            "chat reaches everyone at the table: {log:?}"
        );
    }
}

#[tokio::test]
async fn a_resync_rebuilds_the_whole_view_for_the_asking_seat() {
    let (served, code, (host_token, host_seat), (guest_token, _)) = table_ready().await;

    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&host_token)).await;
    let mut guest = Client::connect(&served, &code).await;
    guest.hello(Some(&guest_token)).await;
    host.send(json!({ "type": "start" })).await;
    host.next_of("snapshot").await;
    guest.next_of("snapshot").await;

    guest.send(json!({ "type": "resync" })).await;
    let again = guest.next_of("snapshot").await;
    assert_eq!(again["snapshot"]["status"], "playing");
    assert!(
        seat_in(&again, "snapshot", host_seat)["hand"]
            .as_array()
            .expect("hand")
            .is_empty(),
        "a resync is not a way to ask for someone else's hand: {again:?}"
    );
}

#[tokio::test]
async fn closing_the_room_closes_every_socket_on_it() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let (code, token, _) = open_table(&app, &access).await;
    let served = serve(app).await;

    let mut host = Client::connect(&served, &code).await;
    host.hello(Some(&token)).await;

    let (status, _, _) = send(
        &served.app,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/tools/mtg/play/rooms/{code}"))
            .header(AUTHORIZATION, format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The socket is told why before it is closed, and closed with the "don't come back" code.
    let closed = host.next_of("closed").await;
    assert!(
        closed["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("host"),
        "the close says who closed it: {closed:?}"
    );
    let frame = host.next().await;
    assert_eq!(frame["type"], "__close");
    assert_eq!(frame["code"], 4004);
}
