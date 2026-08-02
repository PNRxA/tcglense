//! Deck analysis (issue #596): the composition, legality, and goldfish reads on both the
//! authed deck surface and its public-sharing mirror.
//!
//! What these pin, over and above the pure-function unit tests beside each module:
//!
//! * They are **reads** — a read-only `tcgl_` key may call all three, and none of them is
//!   an existence oracle (another user's deck is a `404`, never a `403`, and a private deck
//!   stays a `404` on the public mirror).
//! * The goldfish is **stateless and reproducible over HTTP**: the same URL deals the same
//!   hand, a fresh request echoes back a seed that replays it, and a mulligan is a genuine
//!   reshuffle rather than the same order minus a card.
//! * A public deck's analysis is byte-identical to what its owner sees — the mirrors call
//!   the same core, and this is what would catch a second implementation drifting in.
//!
//! Drives the real router over the seeded dummy catalog, so decks are built from real card
//! external ids and the legality verdict reads real legality objects.

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// Grab `n` real card external ids from the seeded catalog.
async fn sample_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=40")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "listing seeded cards failed: {body:?}"
    );
    let data = body["data"].as_array().expect("cards data array");
    assert!(
        data.len() >= n,
        "need >= {n} seeded cards, got {}",
        data.len()
    );
    data.iter()
        .take(n)
        .map(|c| c["id"].as_str().expect("card id").to_string())
        .collect()
}

/// A deck with `format`, one non-command section holding `cards` copies each. Returns
/// `(deck_id, section_id)`.
async fn deck_with_cards(
    app: &TestApp,
    token: &str,
    name: &str,
    format: &str,
    cards: &[(String, i64)],
) -> (i64, i64) {
    let (status, _, deck) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg",
            token,
            json!({ "name": name, "format": format }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create deck failed: {deck:?}");
    let deck_id = deck["id"].as_i64().expect("deck id");
    // "Creatures" — a plain section, so it lands in the default library (the seeded
    // `Commander` section is a command zone and is excluded from draws).
    let section_id = deck["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|s| s["name"] == "Creatures")
        .expect("a Creatures section is seeded")["id"]
        .as_i64()
        .expect("section id");

    for (card, quantity) in cards {
        let (status, _, body) = send(
            app,
            json_with_bearer(
                "PUT",
                &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
                token,
                json!({ "quantity": quantity, "foil_quantity": 0, "section_id": section_id }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "add card failed: {body:?}");
    }
    (deck_id, section_id)
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

/// Give the user a username and share the deck, returning their handle.
async fn share(app: &TestApp, access: &str, username: &str, deck_id: i64) -> String {
    let (status, _, user) = send(
        app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            access,
            json!({ "username": username }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set username failed: {user:?}");
    let handle = user["handle"].as_str().expect("handle").to_string();
    let (status, _, _) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    handle
}

#[tokio::test]
async fn analysis_reads_require_authentication() {
    let app = test_app_with_catalog().await;
    for path in [
        "/api/decks/mtg/1/stats",
        "/api/decks/mtg/1/legality",
        "/api/decks/mtg/1/goldfish",
    ] {
        let (status, headers, _) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        // Per-user data must never be shared-cached, not even the 401.
        assert_eq!(cache_control(&headers), Some("no-store"), "{path}");
    }
}

#[tokio::test]
async fn another_users_deck_is_404_never_403() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-analysis@example.com", PW).await;
    let (bob, _) = register(&app, "bob-analysis@example.com", PW).await;
    let cards = sample_card_ids(&app, 1).await;
    let (deck_id, _) = deck_with_cards(
        &app,
        &alice,
        "Alice's deck",
        "Commander",
        &[(cards[0].clone(), 1)],
    )
    .await;

    for path in ["stats", "legality", "goldfish"] {
        let (status, _, _) = send(
            &app,
            get_with_bearer(&format!("/api/decks/mtg/{deck_id}/{path}"), &bob),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} must not confirm another user's deck exists"
        );
    }
}

#[tokio::test]
async fn a_read_only_key_may_analyse() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "readonly-analysis@example.com", PW).await;
    let key = create_key(&app, &access, "read").await;
    let cards = sample_card_ids(&app, 1).await;
    let (deck_id, _) = deck_with_cards(
        &app,
        &access,
        "Read me",
        "Commander",
        &[(cards[0].clone(), 4)],
    )
    .await;

    for path in ["stats", "legality", "goldfish"] {
        let (status, _, body) = send(
            &app,
            get_with_bearer(&format!("/api/decks/mtg/{deck_id}/{path}"), &key),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a read-only key must be able to read {path}: {body:?}"
        );
    }
}

#[tokio::test]
async fn stats_fold_the_deck_and_its_library_separately() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "stats@example.com", PW).await;
    let cards = sample_card_ids(&app, 2).await;
    let (deck_id, section_id) = deck_with_cards(
        &app,
        &access,
        "Curve check",
        "Commander",
        &[(cards[0].clone(), 4), (cards[1].clone(), 3)],
    )
    .await;

    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/stats"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stats failed: {body:?}");
    assert_eq!(body["deck"]["total_copies"], 7);
    assert_eq!(body["deck"]["unique_cards"], 2);
    assert_eq!(body["library"]["total_copies"], 7);
    // The seeded `Commander` / `Sideboard` sections are out of the default library, the
    // section the cards live in is in it.
    let default_ids = body["default_library_section_ids"]
        .as_array()
        .expect("default library sections");
    assert!(default_ids.iter().any(|id| id.as_i64() == Some(section_id)));
    // Draw odds come back as a whole curve, monotonically rising, for the most-copied card.
    let odds = &body["odds"];
    assert_eq!(odds["copies"], 4);
    let curve: Vec<f64> = odds["curve"]
        .as_array()
        .expect("curve")
        .iter()
        .map(|v| v.as_f64().expect("probability"))
        .collect();
    assert_eq!(curve.len(), 7, "the library is only seven cards deep");
    assert!(curve.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!(
        (curve[6] - 1.0).abs() < 1e-9,
        "seeing all seven finds all four"
    );

    // Selecting no sections empties the library and drops the odds entirely.
    let (status, _, body) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/stats?sections="),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["library"]["total_copies"], 0);
    assert!(body["odds"].is_null());

    // A section list that isn't numbers is the caller's mistake, not a smaller library.
    let (status, _, _) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/stats?sections=1,nope"),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn legality_is_null_for_a_format_it_does_not_track() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "legality-null@example.com", PW).await;
    let cards = sample_card_ids(&app, 1).await;
    let (deck_id, _) = deck_with_cards(
        &app,
        &access,
        "Cube draft",
        "Cube",
        &[(cards[0].clone(), 1)],
    )
    .await;

    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "legality failed: {body:?}");
    assert!(
        body["data"].is_null(),
        "an untracked format means nothing to evaluate, not an illegal deck"
    );
}

#[tokio::test]
async fn legality_reports_the_deck_wide_verdict() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "legality@example.com", PW).await;
    let cards = sample_card_ids(&app, 1).await;
    // Two copies of one card in a singleton format, and nowhere near 100 cards.
    let (deck_id, _) = deck_with_cards(
        &app,
        &access,
        "Not singleton",
        "EDH",
        &[(cards[0].clone(), 2)],
    )
    .await;

    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "legality failed: {body:?}");
    let legality = &body["data"];
    assert_eq!(legality["format_key"], "commander");
    assert_eq!(legality["format_label"], "Commander");
    assert_eq!(legality["legal"], false, "two copies breaks singleton");
    assert_eq!(legality["issues"][0]["status"], "over_limit");
    assert_eq!(legality["issues"][0]["quantity"], 2);
    assert_eq!(legality["card_statuses"][&cards[0]], "over_limit");
    // The command zone is empty and the deck is short — both are *warnings*, because an
    // unfinished deck must never be reported as illegal on that account alone.
    let violations = legality["violations"].as_array().expect("violations");
    assert!(violations.iter().any(|v| v["rule"] == "deck-size"));
    assert!(violations.iter().any(|v| v["rule"] == "command-zone"));
    assert!(violations.iter().all(|v| v["severity"] == "warning"));
}

#[tokio::test]
async fn a_goldfish_hand_is_reproducible_from_its_url() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "goldfish@example.com", PW).await;
    let cards = sample_card_ids(&app, 8).await;
    let stack: Vec<(String, i64)> = cards.iter().map(|c| (c.clone(), 3)).collect();
    let (deck_id, _) = deck_with_cards(&app, &access, "Goldfish", "Modern", &stack).await;

    let names = |body: &Value| -> Vec<String> {
        body["hand"]
            .as_array()
            .expect("hand")
            .iter()
            .map(|c| c["id"].as_str().expect("card id").to_string())
            .collect()
    };

    // A seedless request deals a hand and tells you the seed that replays it.
    let (status, _, first) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/goldfish"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "goldfish failed: {first:?}");
    assert_eq!(first["hand"].as_array().expect("hand").len(), 7);
    assert_eq!(first["library_total"], 24);
    assert_eq!(first["library_size"], 17);
    assert_eq!(first["to_bottom"], 0);
    let seed = first["seed"].as_u64().expect("seed");

    let (status, _, replay) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/goldfish?seed={seed}"),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&replay), names(&first), "the seed replays the hand");

    // A mulligan reshuffles: same seed, different hand, and a card owed to the bottom.
    let (status, _, mulled) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/goldfish?seed={seed}&mulligans=1"),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mulled["hand"].as_array().expect("hand").len(), 7);
    assert_eq!(mulled["to_bottom"], 1);
    assert_ne!(names(&mulled), names(&first));

    // Bottoming a card from that hand moves it out, and the draw step refills.
    let bottom = names(&mulled)[0].clone();
    let (status, _, kept) = send(
        &app,
        get_with_bearer(
            &format!(
                "/api/decks/mtg/{deck_id}/goldfish?seed={seed}&mulligans=1&bottom={bottom}&draws=2"
            ),
            &access,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bottoming failed: {kept:?}");
    assert_eq!(kept["to_bottom"], 0);
    assert_eq!(kept["bottomed"].as_array().expect("bottomed").len(), 1);
    assert_eq!(kept["bottomed"][0]["id"], bottom);
    assert_eq!(kept["draws"], 2);
    assert_eq!(kept["hand"].as_array().expect("hand").len(), 8);
}

#[tokio::test]
async fn goldfish_rejects_impossible_requests() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "goldfish-bad@example.com", PW).await;
    let cards = sample_card_ids(&app, 4).await;
    let stack: Vec<(String, i64)> = cards.iter().map(|c| (c.clone(), 4)).collect();
    let (deck_id, _) = deck_with_cards(&app, &access, "Bad asks", "Modern", &stack).await;

    let base = format!("/api/decks/mtg/{deck_id}/goldfish");
    for query in [
        "?mulligans=99".to_string(),
        "?opening=999".to_string(),
        "?draws=99999".to_string(),
        // More cards bottomed than mulligans taken.
        format!("?seed=1&mulligans=0&bottom={}", cards[0]),
        // A card that isn't in the hand (nothing with this id exists at all).
        "?seed=1&mulligans=1&bottom=not-a-card".to_string(),
    ] {
        let (status, _, body) =
            send(&app, get_with_bearer(&format!("{base}{query}"), &access)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query} should be refused: {body:?}"
        );
    }
}

#[tokio::test]
async fn a_shared_deck_analyses_identically_and_privately() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "sharer-analysis@example.com", PW).await;
    let cards = sample_card_ids(&app, 3).await;
    let stack: Vec<(String, i64)> = cards.iter().map(|c| (c.clone(), 2)).collect();
    let (deck_id, _) = deck_with_cards(&app, &access, "Shared", "Modern", &stack).await;

    // Private: the public mirrors are a 404, and never CDN-pinned.
    for path in ["stats", "legality", "goldfish"] {
        let (status, headers, _) = send(
            &app,
            get(&format!("/api/u/nobody-0001/decks/{deck_id}/{path}")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(cache_control(&headers), Some("no-store"), "{path}");
    }

    let handle = share(&app, &access, "sharer", deck_id).await;

    // Shared: the mirror answers, is CDN-cacheable, and matches the owner's own read.
    let (status, headers, public_stats) =
        send(&app, get(&format!("/api/u/{handle}/decks/{deck_id}/stats"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public stats failed: {public_stats:?}"
    );
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a public read should be CDN-cacheable, got {:?}",
        cache_control(&headers)
    );
    let (_, _, owner_stats) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/stats"), &access),
    )
    .await;
    assert_eq!(public_stats, owner_stats, "a shared deck is the same deck");

    let (status, _, public_legality) = send(
        &app,
        get(&format!("/api/u/{handle}/decks/{deck_id}/legality")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, owner_legality) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(public_legality, owner_legality);

    let (status, _, public_hand) = send(
        &app,
        get(&format!("/api/u/{handle}/decks/{deck_id}/goldfish?seed=7")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, owner_hand) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/goldfish?seed=7"),
            &access,
        ),
    )
    .await;
    assert_eq!(
        public_hand, owner_hand,
        "one seed, one deck, one hand — whoever asks"
    );
}

#[tokio::test]
async fn the_format_vocabulary_is_public_and_cacheable() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send(&app, get("/api/games/mtg/formats")).await;
    assert_eq!(status, StatusCode::OK, "formats failed: {body:?}");
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "the format table is static and public"
    );
    let data = body["data"].as_array().expect("formats");
    assert_eq!(data[0]["key"], "standard");
    let commander = data
        .iter()
        .find(|f| f["key"] == "commander")
        .expect("commander is tracked");
    assert_eq!(commander["label"], "Commander");
    assert_eq!(commander["group"], "commander");
    assert_eq!(commander["popular"], true);
    let aliases = commander["aliases"].as_array().expect("aliases");
    assert!(aliases.iter().any(|a| a == "edh"));

    let (status, _, _) = send(&app, get("/api/games/nope/formats")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
