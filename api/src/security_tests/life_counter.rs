//! The life-counter tool (`/api/tools/{game}/life/...`): authentication gating, per-session
//! **ownership isolation** (a seat and a life event have no `user_id`, so every route must prove
//! the parent session is the caller's — a cross-user id is a 404, never a 403), the finished-game
//! immutability contract, the tap/undo round trip, read-only-key write gating, and the deck-record
//! derivation.
//!
//! Drives the real router, so the per-user `no-store` headers and the extractor choices
//! (`AuthUser` reads / `WritableUser` writes) are exercised exactly as in production.

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// Start a game and return its full detail body.
async fn start_game(app: &TestApp, token: &str, body: Value) -> Value {
    let (status, _, out) = send(
        app,
        json_with_bearer("POST", "/api/tools/mtg/life/sessions", token, body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start game failed: {out:?}");
    out
}

/// A two-player game with default everything.
async fn start_duel(app: &TestApp, token: &str) -> Value {
    start_game(
        app,
        token,
        json!({ "starting_life": 20, "players": [{ "name": "Alice" }, { "name": "Bob" }] }),
    )
    .await
}

fn session_id(detail: &Value) -> i64 {
    detail["session"]["id"].as_i64().expect("session id")
}

fn player_id(detail: &Value, index: usize) -> i64 {
    detail["session"]["players"][index]["id"]
        .as_i64()
        .expect("player id")
}

/// Create a deck for the token's user and return its id.
async fn create_deck(app: &TestApp, token: &str, name: &str) -> i64 {
    let (status, _, body) = send(
        app,
        json_with_bearer("POST", "/api/decks/mtg", token, json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create deck failed: {body:?}");
    body["id"].as_i64().expect("deck id")
}

/// Mint a scoped API key for a signed-in user.
async fn create_key(app: &TestApp, access: &str, scope: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            json!({ "name": "k", "scope": scope }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {body:?}");
    body["key"].as_str().expect("plaintext key").to_string()
}

#[tokio::test]
async fn life_counter_requires_authentication() {
    let app = test_app().await;

    // No bearer -> 401, and per-user data must never be shared-cached.
    for uri in [
        "/api/tools/mtg/life/sessions",
        "/api/tools/mtg/life/sessions/1",
        "/api/tools/mtg/life/decks",
    ] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }
}

#[tokio::test]
async fn starting_a_game_seats_everyone_on_full_life_with_an_empty_history() {
    let app = test_app().await;
    let (access, _) = register(&app, "duelist@example.com", PW).await;

    let detail = start_game(
        &app,
        &access,
        json!({
            "name": "Friday pod",
            "format": "commander",
            "starting_life": 40,
            "players": [{ "name": "Alice" }, {}, { "name": "Carol", "rotation": 90 }],
        }),
    )
    .await;

    let session = &detail["session"];
    assert_eq!(session["status"], "active");
    assert_eq!(session["starting_life"], 40);
    // Three players with no layout asked for: the arrangement three players actually sit in.
    assert_eq!(session["layout"], "facing");
    assert!(session["finished_at"].is_null());

    let players = session["players"].as_array().expect("players");
    assert_eq!(players.len(), 3);
    for (index, player) in players.iter().enumerate() {
        assert_eq!(player["position"], index as i64);
        assert_eq!(player["life"], 40, "everyone starts on full life");
        assert_eq!(player["starting_life"], 40);
        assert_eq!(player["result"], "none");
    }
    // An unnamed seat is filled in rather than left blank — the mat has to be readable.
    assert_eq!(players[1]["name"], "Player 2");
    // An explicit rotation wins over the layout's default.
    assert_eq!(players[2]["rotation"], 90);
    // A brand-new game has no history.
    assert_eq!(detail["events"].as_array().expect("events").len(), 0);
}

#[tokio::test]
async fn a_game_needs_players_and_rejects_a_bad_layout_or_rotation() {
    let app = test_app().await;
    let (access, _) = register(&app, "validate@example.com", PW).await;

    for (body, why) in [
        (json!({ "players": [] }), "no players"),
        (
            json!({ "players": [{}], "layout": "spiral" }),
            "unknown layout",
        ),
        (
            json!({ "players": [{ "rotation": 45 }] }),
            "off-vocabulary rotation",
        ),
        (
            json!({ "players": [{ "starting_life": 0 }] }),
            "zero starting life",
        ),
    ] {
        let (status, _, out) = send(
            &app,
            json_with_bearer("POST", "/api/tools/mtg/life/sessions", &access, body),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{why} should be a 422: {out:?}"
        );
    }

    // An unknown game is a 404, not a validation error.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/nope/life/sessions",
            &access,
            json!({ "players": [{}] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn taps_move_the_total_and_land_in_the_history_and_can_be_undone() {
    let app = test_app().await;
    let (access, _) = register(&app, "tapper@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);

    // A committed run of taps: one relative change.
    let (status, _, change) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &access,
            json!({ "delta": -3 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{change:?}");
    assert_eq!(change["player"]["life"], 17);
    assert_eq!(change["event"]["delta"], -3);
    assert_eq!(change["event"]["life_after"], 17);
    assert_eq!(change["event"]["kind"], "adjust");
    let first_event = change["event"]["id"].as_i64().expect("event id");

    // An absolute correction: recorded as a `set`, with the distance it moved.
    let (status, _, change) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &access,
            json!({ "life": 12 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{change:?}");
    assert_eq!(change["player"]["life"], 12);
    assert_eq!(change["event"]["kind"], "set");
    assert_eq!(change["event"]["delta"], -5);

    // Both changes are in the history, in order.
    let (status, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = detail["events"].as_array().expect("events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["delta"], -3);
    assert_eq!(events[1]["kind"], "set");

    // Undoing the FIRST change re-folds the chain: the later `set` still pins 12, and only its
    // own reported delta moves (now -8, from the untouched 20).
    let (status, _, detail) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/tools/mtg/life/sessions/{id}/events/{first_event}"
            ))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail:?}");
    let events = detail["events"].as_array().expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["life_after"], 12);
    assert_eq!(events[0]["delta"], -8);
    assert_eq!(detail["session"]["players"][0]["life"], 12);
}

#[tokio::test]
async fn a_life_change_needs_exactly_one_of_delta_or_life() {
    let app = test_app().await;
    let (access, _) = register(&app, "ambiguous@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);
    let uri = format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life");

    for (body, why) in [
        (json!({}), "neither"),
        (json!({ "delta": -1, "life": 10 }), "both"),
        (json!({ "delta": 5000 }), "delta over the cap"),
        (json!({ "life": 100000 }), "life out of range"),
    ] {
        let (status, _, out) = send(&app, json_with_bearer("POST", &uri, &access, body)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{why} should be a 422: {out:?}"
        );
    }
}

#[tokio::test]
async fn another_users_session_seat_and_event_are_all_404_never_403() {
    let app = test_app().await;
    let (owner, _) = register(&app, "owner@example.com", PW).await;
    let (intruder, _) = register(&app, "intruder@example.com", PW).await;

    let detail = start_duel(&app, &owner).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);
    // Give the game one event so the intruder has a real event id to aim at.
    let (_, _, change) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &owner,
            json!({ "delta": -1 }),
        ),
    )
    .await;
    let event = change["event"]["id"].as_i64().expect("event id");

    // Reads.
    let (status, _, _) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &intruder),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "reading another user's game");

    // Every write path: the seat and the event hang off the session, so each must re-prove it.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &intruder,
            json!({ "delta": -5 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "tapping another user's seat");

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}"),
            &intruder,
            json!({ "name": "Pwned" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "editing another user's seat");

    for (method, uri) in [
        ("DELETE", format!("/api/tools/mtg/life/sessions/{id}")),
        (
            "DELETE",
            format!("/api/tools/mtg/life/sessions/{id}/events/{event}"),
        ),
        (
            "DELETE",
            format!("/api/tools/mtg/life/sessions/{id}/players/{alice}"),
        ),
    ] {
        let (status, _, _) = send(
            &app,
            Request::builder()
                .method(method)
                .uri(&uri)
                .header("authorization", format!("Bearer {intruder}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }

    // And the owner's game is untouched by any of it.
    let (status, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["session"]["players"][0]["name"], "Alice");
    assert_eq!(detail["session"]["players"][0]["life"], 19);
    assert_eq!(detail["events"].as_array().expect("events").len(), 1);
}

#[tokio::test]
async fn a_seat_cannot_link_to_another_users_deck() {
    let app = test_app().await;
    let (owner, _) = register(&app, "deckowner@example.com", PW).await;
    let (other, _) = register(&app, "deckother@example.com", PW).await;
    let foreign_deck = create_deck(&app, &owner, "Krenko").await;

    // At create time...
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/life/sessions",
            &other,
            json!({ "players": [{ "name": "A", "deck_id": foreign_deck }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ...and when editing a seat afterwards.
    let detail = start_duel(&app, &other).await;
    let id = session_id(&detail);
    let seat = player_id(&detail, 0);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{seat}"),
            &other,
            json!({ "name": "A", "deck_id": foreign_deck }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_finished_game_is_immutable_and_can_be_rematched() {
    let app = test_app().await;
    let (access, _) = register(&app, "finisher@example.com", PW).await;
    let deck = create_deck(&app, &access, "Atraxa").await;

    let detail = start_game(
        &app,
        &access,
        json!({
            "starting_life": 40,
            "layout": "facing",
            "players": [
                { "name": "Alice", "deck_id": deck },
                { "name": "Bob" },
            ],
        }),
    )
    .await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);
    let bob = player_id(&detail, 1);

    let (status, _, finished) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/finish"),
            &access,
            json!({ "winner_player_id": alice }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished:?}");
    assert_eq!(finished["session"]["status"], "finished");
    assert!(finished["session"]["finished_at"].is_string());
    assert_eq!(finished["session"]["players"][0]["result"], "win");
    assert_eq!(finished["session"]["players"][1]["result"], "loss");

    // Every edit is refused with a 409 — a counted result must not drift.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &access,
            json!({ "delta": -5 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "tapping a finished game");

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{bob}"),
            &access,
            json!({ "name": "Robert" }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "editing a finished game's seat"
    );

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/finish"),
            &access,
            json!({ "winner_player_id": bob }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "re-finishing");

    // A rematch copies the table (names, decks, starting life, layout) onto a fresh game.
    let rematch = start_game(&app, &access, json!({ "from_session_id": id })).await;
    let session = &rematch["session"];
    assert_ne!(session_id(&rematch), id);
    assert_eq!(session["status"], "active");
    assert_eq!(session["starting_life"], 40);
    assert_eq!(session["layout"], "facing");
    let players = session["players"].as_array().expect("players");
    assert_eq!(players.len(), 2);
    assert_eq!(players[0]["name"], "Alice");
    assert_eq!(players[0]["deck_id"], deck);
    assert_eq!(players[0]["life"], 40, "a rematch starts on full life");
    assert_eq!(players[0]["result"], "none");
    assert_eq!(rematch["events"].as_array().expect("events").len(), 0);
}

#[tokio::test]
async fn finishing_with_a_null_winner_records_a_draw_for_the_table() {
    let app = test_app().await;
    let (access, _) = register(&app, "drawer@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);

    let (status, _, finished) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/finish"),
            &access,
            json!({ "winner_player_id": null }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished:?}");
    for player in finished["session"]["players"].as_array().expect("players") {
        assert_eq!(player["result"], "draw");
    }

    // A winner that isn't one of the game's seats is a 404, not a silently ignored field.
    let other = start_duel(&app, &access).await;
    let stranger = player_id(&other, 0);
    let fresh = start_duel(&app, &access).await;
    let fresh_id = session_id(&fresh);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{fresh_id}/finish"),
            &access,
            json!({ "winner_player_id": stranger }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn seats_can_be_added_removed_and_reordered_and_positions_stay_dense() {
    let app = test_app().await;
    let (access, _) = register(&app, "seating@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);

    // Add a third.
    let (status, _, detail) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players"),
            &access,
            json!({ "name": "Carol" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail:?}");
    let players = detail["session"]["players"].as_array().expect("players");
    assert_eq!(players.len(), 3);
    assert_eq!(players[2]["name"], "Carol");
    assert_eq!(players[2]["position"], 2);
    assert_eq!(
        players[2]["life"], 20,
        "inherits the session's starting life"
    );

    // Reorder: Carol first.
    let ids: Vec<i64> = players
        .iter()
        .map(|p| p["id"].as_i64().expect("id"))
        .collect();
    let (status, _, detail) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/reorder"),
            &access,
            json!({ "player_ids": [ids[2], ids[0], ids[1]] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail:?}");
    let players = detail["session"]["players"].as_array().expect("players");
    assert_eq!(players[0]["name"], "Carol");
    assert_eq!(
        players
            .iter()
            .map(|p| p["position"].as_i64())
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2)],
    );

    // A partial permutation is refused rather than silently collapsing the order.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/reorder"),
            &access,
            json!({ "player_ids": [ids[0]] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Remove the middle seat: the rest renumber with no gap.
    let (status, _, detail) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/tools/mtg/life/sessions/{id}/players/{}",
                ids[0]
            ))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail:?}");
    let players = detail["session"]["players"].as_array().expect("players");
    assert_eq!(players.len(), 2);
    assert_eq!(
        players
            .iter()
            .map(|p| p["position"].as_i64())
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)],
    );
}

#[tokio::test]
async fn the_last_seat_cannot_be_removed() {
    let app = test_app().await;
    let (access, _) = register(&app, "solo@example.com", PW).await;
    let detail = start_game(&app, &access, json!({ "players": [{ "name": "Solo" }] })).await;
    let id = session_id(&detail);
    let seat = player_id(&detail, 0);

    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/tools/mtg/life/sessions/{id}/players/{seat}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    // A game with no players isn't a game — delete the session instead.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn deleting_a_session_takes_its_seats_and_history_with_it() {
    let app = test_app().await;
    let (access, _) = register(&app, "deleter@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);
    let (_, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &access,
            json!({ "delta": -4 }),
        ),
    )
    .await;

    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/tools/mtg/life/sessions/{id}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The session is gone...
    let (status, _, _) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ...and so are its children, rather than left orphaned in the tables.
    use crate::entities::prelude::{LifeEvent, LifeSessionPlayer};
    use sea_orm::{EntityTrait, PaginatorTrait};
    assert_eq!(
        LifeSessionPlayer::find()
            .count(&app.state.db)
            .await
            .expect("count seats"),
        0,
    );
    assert_eq!(
        LifeEvent::find()
            .count(&app.state.db)
            .await
            .expect("count events"),
        0,
    );
}

#[tokio::test]
async fn the_session_list_narrows_by_status_and_never_shows_another_users_games() {
    let app = test_app().await;
    let (mine, _) = register(&app, "lister@example.com", PW).await;
    let (theirs, _) = register(&app, "otherlister@example.com", PW).await;

    let finished = start_duel(&app, &mine).await;
    let finished_id = session_id(&finished);
    send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{finished_id}/finish"),
            &mine,
            json!({ "winner_player_id": null }),
        ),
    )
    .await;
    let active = start_duel(&app, &mine).await;
    let active_id = session_id(&active);
    start_duel(&app, &theirs).await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/tools/mtg/life/sessions", &mine)).await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), 2, "only my games: {body:?}");
    // Newest-started first, and seats are inlined so a list row can name who played.
    assert_eq!(rows[0]["id"], active_id);
    assert_eq!(rows[0]["players"].as_array().expect("players").len(), 2);

    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/tools/mtg/life/sessions?status=active", &mine),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], active_id);

    // An unknown status is a 422 rather than a silently unfiltered list.
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/tools/mtg/life/sessions?status=maybe", &mine),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn deck_records_count_only_finished_games_and_survive_a_deleted_deck() {
    let app = test_app().await;
    let (access, _) = register(&app, "recorder@example.com", PW).await;
    let deck = create_deck(&app, &access, "Atraxa").await;

    // A finished win, a finished loss, and an abandoned game — the last must not count.
    for winner_is_mine in [true, false] {
        let detail = start_game(
            &app,
            &access,
            json!({
                "players": [{ "name": "Me", "deck_id": deck }, { "name": "Them" }],
            }),
        )
        .await;
        let id = session_id(&detail);
        let winner = player_id(&detail, if winner_is_mine { 0 } else { 1 });
        send(
            &app,
            json_with_bearer(
                "POST",
                &format!("/api/tools/mtg/life/sessions/{id}/finish"),
                &access,
                json!({ "winner_player_id": winner }),
            ),
        )
        .await;
    }
    start_game(
        &app,
        &access,
        json!({ "players": [{ "name": "Me", "deck_id": deck }] }),
    )
    .await;

    let (status, headers, body) =
        send(&app, get_with_bearer("/api/tools/mtg/life/decks", &access)).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["deck_id"], deck);
    assert_eq!(rows[0]["deck_name"], "Atraxa");
    assert_eq!(rows[0]["games"], 2, "the unfinished game must not count");
    assert_eq!(rows[0]["wins"], 1);
    assert_eq!(rows[0]["losses"], 1);
    assert_eq!(rows[0]["draws"], 0);
    assert_eq!(rows[0]["win_rate"], 0.5);
    assert!(rows[0]["last_played_at"].is_string());

    // `?deck_id=` narrows, which is what a deck's own page asks for.
    let (status, _, body) = send(
        &app,
        get_with_bearer(
            &format!("/api/tools/mtg/life/decks?deck_id={deck}"),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().expect("data").len(), 1);

    // Deleting the deck must not fail, delete history, or leave a dangling record: the seat's
    // `deck_id` is FK-less and orphan-tolerant, and the record simply stops counting it.
    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/decks/mtg/{deck}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = send(&app, get_with_bearer("/api/tools/mtg/life/decks", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().expect("data").len(), 0);

    // And the games themselves are still there, with the link reported as absent.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/tools/mtg/life/sessions", &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert!(
            row["players"][0]["deck_id"].is_null(),
            "a deleted deck reads as no deck, not a dangling id: {row:?}"
        );
        assert!(row["players"][0]["deck_name"].is_null());
    }
}

#[tokio::test]
async fn a_read_only_api_key_can_look_but_not_touch() {
    let app = test_app().await;
    let (access, _) = register(&app, "keyed@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);

    let read_only = create_key(&app, &access, "read").await;
    let read_write = create_key(&app, &access, "read_write").await;

    // A read-only key reads the game fine.
    let (status, _, _) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &read_only),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...but every write is a 403 (not a 401 — the key is valid, just not writable).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &read_only,
            json!({ "delta": -1 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/life/sessions",
            &read_only,
            json!({ "players": [{}] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A read-write key can.
    let (status, _, out) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life"),
            &read_write,
            json!({ "delta": -1 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{out:?}");
    assert_eq!(out["player"]["life"], 19);

    // A bogus key is a 401.
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/tools/mtg/life/sessions", "tcgl_deadbeef"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_rematch_survives_a_deck_deleted_since_the_game_was_played() {
    let app = test_app().await;
    let (access, _) = register(&app, "rematcher@example.com", PW).await;
    let keep = create_deck(&app, &access, "Krenko").await;
    let doomed = create_deck(&app, &access, "Atraxa").await;

    let detail = start_game(
        &app,
        &access,
        json!({
            "players": [
                { "name": "Me", "deck_id": doomed },
                { "name": "Them", "deck_id": keep },
            ],
        }),
    )
    .await;
    let id = session_id(&detail);
    let winner = player_id(&detail, 0);
    send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{id}/finish"),
            &access,
            json!({ "winner_player_id": winner }),
        ),
    )
    .await;

    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/decks/mtg/{doomed}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // A *copied* deck reference that no longer resolves is DROPPED, not fatal: deleting a deck
    // costs you its record, never your ability to play the same pod again.
    let rematch = start_game(&app, &access, json!({ "from_session_id": id })).await;
    let players = rematch["session"]["players"].as_array().expect("players");
    assert_eq!(players.len(), 2);
    assert_eq!(players[0]["name"], "Me");
    assert!(
        players[0]["deck_id"].is_null(),
        "the deleted deck's link is dropped: {players:?}"
    );
    // The surviving deck is still linked, so a rematch isn't flattened wholesale.
    assert_eq!(players[1]["deck_id"], keep);

    // An *explicit* reference to a deck the caller doesn't own stays a 404 — that's a client
    // error, not history.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/life/sessions",
            &access,
            json!({ "players": [{ "name": "Me", "deck_id": doomed }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reorder_rejects_a_repeated_seat_rather_than_leaving_a_position_hole() {
    let app = test_app().await;
    let (access, _) = register(&app, "reorderer@example.com", PW).await;
    let detail = start_game(
        &app,
        &access,
        json!({ "players": [{ "name": "A" }, { "name": "B" }, { "name": "C" }] }),
    )
    .await;
    let id = session_id(&detail);
    let a = player_id(&detail, 0);
    let b = player_id(&detail, 1);

    // A list of the right length but with a seat repeated: sorting and de-duplicating it makes it
    // compare equal to the real seat set, so the *length* check is what catches this. Left
    // unchecked, two seats would be written to the same position and one position would be left
    // empty — and `position` is what the layout maths indexes into to place a seat on the mat.
    let (status, _, out) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/reorder"),
            &access,
            json!({ "player_ids": [a, a, b] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{out:?}");

    // ...and the seats are untouched: still dense, still in their original order.
    let (status, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let players = detail["session"]["players"].as_array().expect("players");
    assert_eq!(
        players
            .iter()
            .map(|p| (p["name"].as_str(), p["position"].as_i64()))
            .collect::<Vec<_>>(),
        vec![
            (Some("A"), Some(0)),
            (Some("B"), Some(1)),
            (Some("C"), Some(2))
        ],
    );

    // A short list of the right shape is refused too, not just a repeat.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/reorder"),
            &access,
            json!({ "player_ids": [a, b] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn an_extreme_delta_is_refused_rather_than_panicking_the_handler() {
    let app = test_app().await;
    let (access, _) = register(&app, "extremist@example.com", PW).await;
    let detail = start_duel(&app, &access).await;
    let id = session_id(&detail);
    let alice = player_id(&detail, 0);
    let uri = format!("/api/tools/mtg/life/sessions/{id}/players/{alice}/life");

    // The extremes of the wire type, not just of the documented bound. A naive `abs()` bound
    // check overflows on `i32::MIN` — a panic on a request path in debug, and in release a wrap
    // straight back inside the bound, so the value slips past the check meant to reject it.
    for delta in [i32::MIN, i32::MIN + 1, i32::MAX] {
        let (status, _, out) = send(
            &app,
            json_with_bearer("POST", &uri, &access, json!({ "delta": delta })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "delta {delta} should be a 422: {out:?}"
        );
    }
    for life in [i32::MIN, i32::MAX] {
        let (status, _, out) = send(
            &app,
            json_with_bearer("POST", &uri, &access, json!({ "life": life })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "life {life} should be a 422: {out:?}"
        );
    }

    // The seat is untouched by any of it.
    let (_, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/tools/mtg/life/sessions/{id}"), &access),
    )
    .await;
    assert_eq!(detail["session"]["players"][0]["life"], 20);
    assert_eq!(detail["events"].as_array().expect("events").len(), 0);
}

#[tokio::test]
async fn a_seat_can_name_a_commander_instead_of_a_deck() {
    // Needs the seeded catalog: a commander reference is a real card id, not a free-text name.
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "commander@example.com", PW).await;
    let deck = create_deck(&app, &access, "My deck").await;

    let (status, _, cards) = send(&app, get("/api/games/mtg/cards?page_size=2")).await;
    assert_eq!(status, StatusCode::OK);
    let card = cards["data"][0]["id"]
        .as_str()
        .expect("card id")
        .to_string();
    let card_name = cards["data"][0]["name"]
        .as_str()
        .expect("card name")
        .to_string();

    // The opponent you'll never have a deck for: name what they were playing instead.
    let detail = start_game(
        &app,
        &access,
        json!({
            "players": [
                { "name": "Me", "deck_id": deck },
                { "name": "Them", "commander_card_id": card },
            ],
        }),
    )
    .await;
    let id = session_id(&detail);
    let them = player_id(&detail, 1);
    let players = detail["session"]["players"].as_array().expect("players");
    assert_eq!(players[1]["commander_card_id"], card);
    assert_eq!(players[1]["commander_name"], card_name);
    assert!(players[1]["deck_id"].is_null());
    // ...and the two links don't bleed into each other.
    assert_eq!(players[0]["deck_id"], deck);
    assert!(players[0]["commander_card_id"].is_null());

    // A seat naming both is refused rather than stored ambiguously.
    let (status, _, out) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/life/sessions",
            &access,
            json!({ "players": [{ "deck_id": deck, "commander_card_id": card }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{out:?}");

    // A card the catalog doesn't hold is a 404, like every other card id on the wire.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/tools/mtg/life/sessions",
            &access,
            json!({ "players": [{ "commander_card_id": "no-such-card" }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Editing a seat swaps one link for the other, and clears it when neither is sent.
    let (status, _, seat) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{them}"),
            &access,
            json!({ "name": "Them", "deck_id": deck }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seat:?}");
    assert_eq!(seat["deck_id"], deck);
    assert!(
        seat["commander_card_id"].is_null(),
        "the commander is unlinked: {seat:?}"
    );

    let (status, _, seat) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{them}"),
            &access,
            json!({ "name": "Them" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(seat["deck_id"].is_null());
    assert!(seat["commander_card_id"].is_null());

    // A commander link survives a rematch, like a deck link does.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/tools/mtg/life/sessions/{id}/players/{them}"),
            &access,
            json!({ "name": "Them", "commander_card_id": card }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rematch = start_game(&app, &access, json!({ "from_session_id": id })).await;
    let copied = rematch["session"]["players"].as_array().expect("players");
    assert_eq!(copied[1]["commander_card_id"], card);
    assert_eq!(copied[1]["commander_name"], card_name);

    // A commander is not a deck: it contributes nothing to the per-deck record.
    let winner = player_id(&rematch, 1);
    let rematch_id = session_id(&rematch);
    send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/tools/mtg/life/sessions/{rematch_id}/finish"),
            &access,
            json!({ "winner_player_id": winner }),
        ),
    )
    .await;
    let (status, _, body) = send(&app, get_with_bearer("/api/tools/mtg/life/decks", &access)).await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), 1, "only the deck seat has a record: {body:?}");
    assert_eq!(rows[0]["deck_id"], deck);
    assert_eq!(rows[0]["games"], 1);
    assert_eq!(rows[0]["losses"], 1);
}
