//! The play table's REST surface (`/api/tools/{game}/play/...`): who may open and close a
//! table, how a seat is authorized when the person holding it has no account, and the two
//! places the tool could leak something — the invite code (which must not become an existence
//! oracle) and the seat token (which must be the *only* thing that moves a seat).
//!
//! The socket half lives in [`super::play_ws`]; these drive the real router in-process, so
//! the extractor choices (`SessionUser` for the host's routes, `MaybeUser` + `X-Play-Seat`
//! for the guest's) and the `no-store` headers are exercised exactly as in production.

use super::decks::create_key;
use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// The seeded Commander precon, so a seat can load a real decklist.
const COMMANDER_SLUG: &str = "dummy-universe-commander-dmu";

// ---------- Request builders ----------

/// A JSON POST carrying a seat token instead of a session.
fn json_with_seat(method: &str, uri: &str, seat_token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-play-seat", seat_token)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A bodyless request carrying a seat token.
fn empty_with_seat(method: &str, uri: &str, seat_token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-play-seat", seat_token)
        .body(Body::empty())
        .unwrap()
}

/// A bodyless request carrying a session bearer.
fn empty_with_bearer(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

// ---------- Fixtures ----------

/// Open a table as `token`'s user and return the `JoinResponse`.
async fn create_room(app: &TestApp, token: &str, body: Value) -> Value {
    let (status, _, out) = send(
        app,
        json_with_bearer("POST", "/api/tools/mtg/play/rooms", token, body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create room failed: {out:?}");
    out
}

/// A four-seat Commander table.
async fn create_pod(app: &TestApp, token: &str) -> Value {
    create_room(app, token, json!({ "format": "commander" })).await
}

fn code_of(join: &Value) -> String {
    join["room"]["code"].as_str().expect("room code").to_string()
}

fn seat_id_of(join: &Value) -> i64 {
    join["seat"]["id"].as_i64().expect("seat id")
}

fn token_of(join: &Value) -> String {
    join["seat_token"].as_str().expect("seat token").to_string()
}

/// Join a table as a guest with just a name.
async fn join_as_guest(app: &TestApp, code: &str, name: &str) -> Value {
    let (status, _, out) = send(
        app,
        json_post(
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            json!({ "name": name }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "guest join failed: {out:?}");
    out
}

/// The first seeded card's external id (for building a deck to load).
async fn sample_card_id(app: &TestApp) -> String {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=5")).await;
    assert_eq!(status, StatusCode::OK, "listing seeded cards: {body:?}");
    body["data"][0]["id"]
        .as_str()
        .expect("a seeded card id")
        .to_string()
}

// ---------- Auth ----------

#[tokio::test]
async fn opening_and_listing_tables_needs_a_real_session() {
    let app = test_app().await;

    // No credential at all.
    for req in [
        get("/api/tools/mtg/play/rooms"),
        json_post("/api/tools/mtg/play/rooms", json!({ "format": "commander" })),
    ] {
        let (status, headers, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "a play response is never cached"
        );
    }

    // An API key is a real credential but the wrong kind: play is a session feature, so it is
    // a 403 (like API-key management and the price alerts), not a 401.
    let (access, _) = register(&app, "host@example.com", PW).await;
    let key = create_key(&app, &access, "read_write").await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/play/rooms",
            &key,
            json!({ "format": "commander" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "api key create: {body:?}");
    let (status, _, _) = send(&app, get_with_bearer("/api/tools/mtg/play/rooms", &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "api key list");
}

#[tokio::test]
async fn creating_a_table_seats_the_host_and_hands_back_a_token() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;

    let join = create_room(
        &app,
        &access,
        json!({ "label": "Friday pod", "format": "commander", "max_players": 4 }),
    )
    .await;

    // The code is the shareable identity: six characters from the unambiguous alphabet.
    let code = code_of(&join);
    assert_eq!(code.chars().count(), 6, "room code is six characters: {code}");
    assert!(
        code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        "room code is upper-case alphanumeric: {code}"
    );
    assert!(!code.contains(['0', 'O', '1', 'I']), "unambiguous: {code}");

    // The host is already sitting at seat 0, and the token is shown exactly once.
    assert_eq!(join["room"]["status"], "lobby");
    assert_eq!(join["room"]["label"], "Friday pod");
    assert_eq!(join["room"]["starting_life"], 40, "commander's default");
    assert_eq!(join["seat"]["seat_index"], 0);
    assert_eq!(join["seat"]["is_host"], true);
    assert_eq!(join["seat"]["is_user"], true);
    assert!(!token_of(&join).is_empty());
    assert_eq!(join["room"]["seats"].as_array().expect("seats").len(), 1);

    // Nothing in the public room read carries a seat's credential.
    let (status, headers, room) = send(
        &app,
        get(&format!("/api/tools/mtg/play/rooms/{code}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    let body = room.to_string();
    assert!(
        !body.contains("token"),
        "the public room read must not carry any token material: {body}"
    );
}

#[tokio::test]
async fn a_bad_format_or_seat_count_is_refused() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;

    for body in [
        json!({ "format": "pauper-but-online" }),
        json!({ "format": "commander", "max_players": 1 }),
        json!({ "format": "commander", "max_players": 9 }),
        json!({ "format": "commander", "starting_life": 0 }),
    ] {
        let (status, _, out) = send(
            &app,
            json_with_bearer("POST", "/api/tools/mtg/play/rooms", &access, body.clone()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body} should be refused, got {out:?}"
        );
    }
}

#[tokio::test]
async fn a_room_read_by_code_is_public_and_an_unknown_code_is_a_404() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let code = code_of(&create_pod(&app, &access).await);

    // Public: no credential needed, because the join page renders before anyone has one.
    let (status, _, body) = send(&app, get(&format!("/api/tools/mtg/play/rooms/{code}"))).await;
    assert_eq!(status, StatusCode::OK, "public room read: {body:?}");
    assert_eq!(body["code"], code);

    // Codes are stored upper-case, so a link pasted in lower case still resolves.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/tools/mtg/play/rooms/{}",
            code.to_lowercase()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "lower-cased code: {body:?}");

    let (status, headers, _) = send(&app, get("/api/tools/mtg/play/rooms/ZZZZZZ")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn the_room_list_shows_only_tables_you_are_at() {
    let app = test_app().await;
    let (alice, _) = register(&app, "alice@example.com", PW).await;
    let (bob, _) = register(&app, "bob@example.com", PW).await;

    let hers = code_of(&create_pod(&app, &alice).await);
    let his = code_of(&create_pod(&app, &bob).await);

    let (status, _, body) = send(&app, get_with_bearer("/api/tools/mtg/play/rooms", &alice)).await;
    assert_eq!(status, StatusCode::OK);
    let codes: Vec<&str> = body["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|r| r["code"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(codes, vec![hers.as_str()], "only her own table");
    assert!(!codes.contains(&his.as_str()));

    // Taking a seat at his table puts it in her list too — "rooms I'm at", not "rooms I own".
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{his}/join"),
            &alice,
            json!({ "name": "Alice" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, body) = send(&app, get_with_bearer("/api/tools/mtg/play/rooms", &alice)).await;
    let codes: Vec<&str> = body["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|r| r["code"].as_str().unwrap_or_default())
        .collect();
    assert!(codes.contains(&his.as_str()) && codes.contains(&hers.as_str()));
}

// ---------- Joining ----------

#[tokio::test]
async fn a_guest_needs_a_name_and_then_gets_a_seat() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let code = code_of(&create_pod(&app, &access).await);

    // No account and no name: there is nothing to call this player.
    let (status, _, body) = send(
        &app,
        json_post(&format!("/api/tools/mtg/play/rooms/{code}/join"), json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a nameless guest: {body:?}"
    );

    let join = join_as_guest(&app, &code, "Bob").await;
    assert_eq!(join["seat"]["seat_index"], 1, "the next free chair");
    assert_eq!(join["seat"]["is_user"], false, "a guest has no account");
    assert_eq!(join["seat"]["is_host"], false);
    assert_eq!(join["room"]["seats"].as_array().expect("seats").len(), 2);
    assert!(!token_of(&join).is_empty());
}

#[tokio::test]
async fn re_joining_with_a_seat_token_returns_the_same_seat() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let code = code_of(&create_pod(&app, &access).await);
    let first = join_as_guest(&app, &code, "Bob").await;

    // A reload must not cost you your chair — or your token.
    let (status, _, again) = send(
        &app,
        json_post(
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            json!({ "seat_token": token_of(&first) }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "re-join: {again:?}");
    assert_eq!(seat_id_of(&again), seat_id_of(&first));
    assert_eq!(token_of(&again), token_of(&first));
    assert_eq!(
        again["room"]["seats"].as_array().expect("seats").len(),
        2,
        "re-joining must not eat a second chair"
    );
}

#[tokio::test]
async fn a_signed_in_player_re_takes_their_seat_with_a_fresh_token() {
    let app = test_app().await;
    let (host, _) = register(&app, "host@example.com", PW).await;
    let (bob, _) = register(&app, "bob@example.com", PW).await;
    let code = code_of(&create_pod(&app, &host).await);

    // An email-first account has no username yet, so it names itself at the table — the same
    // 422 a nameless guest gets, and the same fix.
    let (status, _, nameless) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            &bob,
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an account with no handle still needs a table name: {nameless:?}"
    );

    let first = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            &bob,
            json!({ "name": "Bob" }),
        ),
    )
    .await
    .2;
    let (status, _, again) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            &bob,
            json!({ "name": "Bob" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "second join: {again:?}");
    assert_eq!(
        seat_id_of(&again),
        seat_id_of(&first),
        "the account resolves to the same chair"
    );
    assert_ne!(
        token_of(&again),
        token_of(&first),
        "a new device gets a new token, and the old one stops working"
    );
    assert_eq!(again["room"]["seats"].as_array().expect("seats").len(), 2);

    // The rotation is real: the first token no longer authorizes that seat.
    let seat_id = seat_id_of(&first);
    let (status, _, _) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat_id}/ready"),
            &token_of(&first),
            json!({ "ready": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the rotated-away token");
}

#[tokio::test]
async fn a_full_table_refuses_another_seat() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let code = code_of(
        &create_room(
            &app,
            &access,
            json!({ "format": "constructed", "max_players": 2 }),
        )
        .await,
    );
    join_as_guest(&app, &code, "Bob").await;

    let (status, _, body) = send(
        &app,
        json_post(
            &format!("/api/tools/mtg/play/rooms/{code}/join"),
            json!({ "name": "Carol" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a full table: {body:?}");
}

// ---------- Seat authorization ----------

#[tokio::test]
async fn a_seat_scoped_call_needs_that_seats_own_token() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let host = create_pod(&app, &access).await;
    let code = code_of(&host);
    let guest = join_as_guest(&app, &code, "Bob").await;
    let guest_seat = seat_id_of(&guest);
    let host_seat = seat_id_of(&host);

    let ready_uri = format!("/api/tools/mtg/play/rooms/{code}/seats/{guest_seat}/ready");

    // No token at all, a wrong token, and *another seat's* token are all the same 401 —
    // the rejection must not tell you which of the three you got wrong.
    let (status, _, _) = send(
        &app,
        json_post(&ready_uri, json!({ "ready": false })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no seat token");

    for token in [token_of(&host), "not-a-token".to_string()] {
        let (status, _, body) = send(
            &app,
            json_with_seat("POST", &ready_uri, &token, json!({ "ready": false })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "the wrong seat's token: {body:?}"
        );
        assert_eq!(body["error"], "invalid_seat_token");
    }

    // And a session, even the host's, is not a substitute for the seat's token.
    let (status, _, _) = send(
        &app,
        json_with_bearer("POST", &ready_uri, &access, json!({ "ready": false })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a session is not a seat");

    // A seat id from another room is the same 401, not a 404 that confirms it exists.
    let other = code_of(&create_pod(&app, &access).await);
    let (status, _, _) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{other}/seats/{guest_seat}/ready"),
            &token_of(&guest),
            json!({ "ready": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a foreign seat id");
    let _ = host_seat;
}

#[tokio::test]
async fn readying_up_needs_a_deck() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let host = create_pod(&app, &access).await;
    let code = code_of(&host);
    let seat = seat_id_of(&host);
    let token = token_of(&host);

    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/ready"),
            &token,
            json!({ "ready": true }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "ready without a deck: {body:?}"
    );

    // Un-readying is always fine — it promises nothing.
    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/ready"),
            &token,
            json!({ "ready": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "un-ready: {body:?}");
    assert_eq!(body["ready"], false);
}

// ---------- Deck loading ----------

#[tokio::test]
async fn a_seat_can_load_a_precon_and_then_ready_up() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let host = create_pod(&app, &access).await;
    let code = code_of(&host);
    let seat = seat_id_of(&host);
    let token = token_of(&host);

    let (status, headers, body) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/deck"),
            &token,
            json!({ "source": "precon", "slug": COMMANDER_SLUG }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "load precon: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["deck_source"], "precon");
    assert!(body["deck_card_count"].as_i64().unwrap_or(0) > 0);
    assert!(
        !body["commanders"]
            .as_array()
            .expect("commanders")
            .is_empty(),
        "a Commander table seats the precon's command zone: {body:?}"
    );
    assert_eq!(body["ready"], false);

    // Now readying is allowed.
    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/ready"),
            &token,
            json!({ "ready": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "ready with a deck: {body:?}");
    assert_eq!(body["ready"], true);

    // An unknown precon is a 404, not an empty deck.
    let (status, _, _) = send(
        &app,
        json_with_seat(
            "POST",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/deck"),
            &token,
            json!({ "source": "precon", "slug": "no-such-precon" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn loading_one_of_your_own_decks_is_scoped_to_you() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice@example.com", PW).await;
    let (bob, _) = register(&app, "bob@example.com", PW).await;
    let card = sample_card_id(&app).await;

    // Alice builds a deck with one card in it.
    let (status, _, deck) = send(
        &app,
        json_with_bearer("POST", "/api/decks/mtg", &alice, json!({ "name": "Goblins" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create deck: {deck:?}");
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &alice,
            json!({ "quantity": 2, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add card: {body:?}");

    let room = create_pod(&app, &alice).await;
    let code = code_of(&room);
    let seat = seat_id_of(&room);
    let deck_uri = format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/deck");

    // The deck source needs the session as well as the seat token — "my decks" is meaningless
    // without an account.
    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &deck_uri,
            &token_of(&room),
            json!({ "source": "deck", "deck_id": deck_id }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "no session: {body:?}"
    );

    let mut req = json_with_seat(
        "POST",
        &deck_uri,
        &token_of(&room),
        json!({ "source": "deck", "deck_id": deck_id }),
    );
    req.headers_mut().insert(
        AUTHORIZATION,
        format!("Bearer {alice}").parse().expect("bearer header"),
    );
    let (status, _, body) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK, "load own deck: {body:?}");
    assert_eq!(body["deck_source"], "deck");
    assert_eq!(body["deck_name"], "Goblins");
    assert_eq!(body["deck_card_count"], 2, "two copies, two cards");

    // Bob's session can't pull Alice's deck through Bob's own seat: a foreign deck is a 404.
    let bob_seat = {
        let joined = send(
            &app,
            json_with_bearer(
                "POST",
                &format!("/api/tools/mtg/play/rooms/{code}/join"),
                &bob,
                json!({ "name": "Bob" }),
            ),
        )
        .await
        .2;
        (seat_id_of(&joined), token_of(&joined))
    };
    let mut req = json_with_seat(
        "POST",
        &format!(
            "/api/tools/mtg/play/rooms/{code}/seats/{}/deck",
            bob_seat.0
        ),
        &bob_seat.1,
        json!({ "source": "deck", "deck_id": deck_id }),
    );
    req.headers_mut().insert(
        AUTHORIZATION,
        format!("Bearer {bob}").parse().expect("bearer header"),
    );
    let (status, _, body) = send(&app, req).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "someone else's deck: {body:?}"
    );
}

#[tokio::test]
async fn a_pasted_list_with_an_unknown_card_names_it_rather_than_shrinking_the_deck() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let room = create_pod(&app, &access).await;
    let code = code_of(&room);
    let seat = seat_id_of(&room);
    let deck_uri = format!("/api/tools/mtg/play/rooms/{code}/seats/{seat}/deck");

    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &deck_uri,
            &token_of(&room),
            json!({ "source": "text", "text": "1 Definitely Not A Real Card\n" }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an unresolved list: {body:?}"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Definitely Not A Real Card"),
        "the refusal names the card it couldn't find: {body:?}"
    );

    // A list of real cards loads, with the `Commander` header putting one in the command zone.
    let (_, _, cards) = send(&app, get("/api/games/mtg/cards?page_size=3")).await;
    let names: Vec<&str> = cards["data"]
        .as_array()
        .expect("cards")
        .iter()
        .map(|c| c["name"].as_str().expect("name"))
        .collect();
    let list = format!(
        "Commander\n1 {}\n\nMainboard\n2 {}\n",
        names[0], names[1]
    );
    let (status, _, body) = send(
        &app,
        json_with_seat(
            "POST",
            &deck_uri,
            &token_of(&room),
            json!({ "source": "text", "text": list }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a real list: {body:?}");
    assert_eq!(body["deck_source"], "text");
    assert_eq!(body["deck_card_count"], 3);
    assert_eq!(
        body["commanders"].as_array().expect("commanders").len(),
        1,
        "the `Commander` section header seats a commander: {body:?}"
    );
}

// ---------- Leaving and closing ----------

#[tokio::test]
async fn only_the_host_can_close_a_table_and_everyone_else_sees_a_404() {
    let app = test_app().await;
    let (alice, _) = register(&app, "alice@example.com", PW).await;
    let (bob, _) = register(&app, "bob@example.com", PW).await;
    let code = code_of(&create_pod(&app, &alice).await);
    let uri = format!("/api/tools/mtg/play/rooms/{code}");

    // A stranger holding the code gets the same answer as for a code that doesn't exist —
    // no existence oracle over invite codes.
    let (status, _, _) = send(&app, empty_with_bearer("DELETE", &uri, &bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a non-host deleting");
    let (status, _, _) = send(
        &app,
        empty_with_bearer("DELETE", "/api/tools/mtg/play/rooms/ZZZZZZ", &bob),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "an unknown code");
    // …and the table is still there.
    let (status, _, _) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::OK);

    // The host closes it, and it is gone for everyone.
    let (status, _, _) = send(&app, empty_with_bearer("DELETE", &uri, &alice)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the table is gone");
}

#[tokio::test]
async fn a_seat_can_leave_and_the_host_can_remove_one_but_not_their_own() {
    let app = test_app().await;
    let (access, _) = register(&app, "host@example.com", PW).await;
    let host = create_pod(&app, &access).await;
    let code = code_of(&host);

    // A guest leaves under their own token.
    let bob = join_as_guest(&app, &code, "Bob").await;
    let bob_seat = seat_id_of(&bob);
    let (status, _, _) = send(
        &app,
        empty_with_seat(
            "DELETE",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{bob_seat}"),
            &token_of(&bob),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The host removes someone else's seat with nothing but their session.
    let carol = join_as_guest(&app, &code, "Carol").await;
    let carol_seat = seat_id_of(&carol);
    let (status, _, _) = send(
        &app,
        empty_with_bearer(
            "DELETE",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{carol_seat}"),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // But the host can't leave their own table — that would leave it nobody's.
    let host_seat = seat_id_of(&host);
    let (status, _, body) = send(
        &app,
        empty_with_seat(
            "DELETE",
            &format!("/api/tools/mtg/play/rooms/{code}/seats/{host_seat}"),
            &token_of(&host),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "the host leaving: {body:?}");

    let (_, _, room) = send(&app, get(&format!("/api/tools/mtg/play/rooms/{code}"))).await;
    assert_eq!(room["seats"].as_array().expect("seats").len(), 1);
}
