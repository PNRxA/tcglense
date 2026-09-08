//! Deck analysis (issue #596): the composition, legality, bracket, roles (issue #671), and
//! goldfish reads on both the authed deck surface and its public-sharing mirror.
//!
//! What these pin, over and above the pure-function unit tests beside each module:
//!
//! * They are **reads** — a read-only `tcgl_` key may call every one of them, and none is
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
        "/api/decks/mtg/1/bracket",
        "/api/decks/mtg/1/roles",
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

    for path in [
        "stats", "legality", "bracket", "tokens", "mana", "roles", "goldfish", "combos",
    ] {
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

    for path in [
        "stats", "legality", "bracket", "tokens", "mana", "roles", "goldfish", "pricing", "combos",
    ] {
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
    for path in [
        "stats", "legality", "bracket", "tokens", "mana", "roles", "goldfish", "pricing", "combos",
    ] {
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

    let (status, _, public_bracket) = send(
        &app,
        get(&format!("/api/u/{handle}/decks/{deck_id}/bracket")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, owner_bracket) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/bracket"), &access),
    )
    .await;
    assert_eq!(public_bracket, owner_bracket);

    let (status, headers, public_hand) = send(
        &app,
        get(&format!("/api/u/{handle}/decks/{deck_id}/goldfish?seed=7")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a seeded hand IS a function of its URL, so it's fine to cache"
    );

    // …but a seedless one isn't. The server mints a random seed, so a shared cache would pin
    // whatever the first visitor rolled as *the* hand for the whole TTL.
    let mut seeds = std::collections::HashSet::new();
    for _ in 0..4 {
        let (status, headers, body) = send(
            &app,
            get(&format!("/api/u/{handle}/decks/{deck_id}/goldfish")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "a hand that isn't a function of its URL must not be shared-cached"
        );
        seeds.insert(body["seed"].as_u64().expect("seed"));
    }
    assert!(
        seeds.len() > 1,
        "a seedless hand should differ between calls"
    );
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
async fn the_bracket_is_estimated_only_for_commander() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "bracket@example.com", PW).await;
    let cards = sample_card_ids(&app, 2).await;
    let stack: Vec<(String, i64)> = cards.iter().map(|c| (c.clone(), 1)).collect();

    // The ladder is defined for Commander and no other format, so anything else answers
    // "nothing to say" rather than putting a number on a deck it doesn't describe.
    let (modern, _) = deck_with_cards(&app, &access, "Burn", "Modern", &stack).await;
    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{modern}/bracket"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bracket failed: {body:?}");
    assert!(body["data"].is_null(), "brackets are a Commander thing");

    let (edh, _) = deck_with_cards(&app, &access, "Precon", "EDH", &stack).await;
    let (status, headers, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{edh}/bracket"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bracket failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"), "per-user data");
    let estimate = &body["data"];
    assert_eq!(estimate["format_key"], "commander");
    // Seeded dummy cards carry no Game Changer flag and no land denial, so the estimate
    // sits on its floor — and says so, with the whole ladder and the caveats that make a
    // floor honest.
    assert_eq!(estimate["bracket"], 2);
    assert_eq!(estimate["label"], "Core");
    assert_eq!(estimate["ladder"].as_array().expect("ladder").len(), 5);
    assert_eq!(
        estimate["categories"].as_array().expect("categories").len(),
        4
    );
    assert!(!estimate["reasons"].as_array().expect("reasons").is_empty());
    assert!(!estimate["caveats"].as_array().expect("caveats").is_empty());

    // The public mirror, on a deck that produces a REAL estimate. The parity assertion in
    // `a_shared_deck_analyses_identically_and_privately` runs over a Modern deck, where both
    // sides are null and `null == null` would hold however wrong the mirror was — so this is
    // the one that actually pins "the same `analyse_bracket` core".
    let handle = share(&app, &access, "bracketeer", edh).await;
    let (status, headers, public_bracket) =
        send(&app, get(&format!("/api/u/{handle}/decks/{edh}/bracket"))).await;
    assert_eq!(status, StatusCode::OK, "public bracket: {public_bracket:?}");
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a public read is a pure function of its URL, so it's CDN-cacheable"
    );
    assert!(
        !public_bracket["data"].is_null(),
        "a real estimate, not null"
    );
    assert_eq!(
        public_bracket, body,
        "a shared deck and its owner's copy are the same deck"
    );
}

/// The role counts (issue #671) over the seeded catalog, whose numbered cards carry one
/// role-shaped line of rules text each — so this pins that the read counts real cards, hands
/// back the printings the filter needs, and answers identically on the public mirror.
#[tokio::test]
async fn roles_count_the_deck_and_mirror_publicly() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "roles@example.com", PW).await;
    // Six consecutive seeded cards walk the whole type/rules-text cycle once: a dork, a
    // counterspell, a wrath, a draw engine, a rock, and a piece of spot removal. By id, not
    // `sample_card_ids` — the listing is name-ordered, and a name held in two sets would
    // fold into one card and break the count.
    let cards: Vec<String> = (1..=6).map(|n| format!("dummy-dmu-{n:04}")).collect();
    let stack: Vec<(String, i64)> = cards.iter().map(|c| (c.clone(), 2)).collect();
    let (deck_id, _) = deck_with_cards(&app, &access, "Roles", "Commander", &stack).await;

    let (status, headers, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/roles"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "roles failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"), "per-user data");

    let roles = body["roles"].as_array().expect("roles");
    assert_eq!(roles.len(), 8, "every role is always reported: {body:?}");
    let count = |role: &str| -> i64 {
        roles
            .iter()
            .find(|group| group["role"] == role)
            .unwrap_or_else(|| panic!("{role} missing from {body:?}"))["count"]
            .as_i64()
            .expect("count")
    };
    assert_eq!(count("ramp"), 2, "the dork and the rock: {body:?}");
    assert_eq!(count("counterspell"), 1);
    assert_eq!(count("board_wipe"), 1);
    assert_eq!(count("card_draw"), 1);
    assert_eq!(count("removal"), 1);
    assert_eq!(count("tutor"), 0);
    assert_eq!(count("recursion"), 0);
    assert_eq!(count("protection"), 0);
    assert_eq!(body["card_count"], 6);
    assert_eq!(body["unclassified_count"], 0);
    // Two copies each, so the bars a 60-card builder reads differ from the names.
    let ramp = roles
        .iter()
        .find(|group| group["role"] == "ramp")
        .expect("ramp");
    assert_eq!(ramp["copies"], 4);
    assert_eq!(ramp["cards"].as_array().expect("cards").len(), 2);
    // Every printing holding a role is in the filter map, keyed by its external id.
    let card_roles = body["card_roles"].as_object().expect("card_roles");
    assert_eq!(card_roles.len(), 6);
    for card in &cards {
        assert!(
            card_roles.contains_key(card),
            "{card} should be filterable: {card_roles:?}"
        );
    }

    // The public mirror is the same computation, and CDN-cacheable.
    let handle = share(&app, &access, "roleplayer", deck_id).await;
    let (status, headers, public) =
        send(&app, get(&format!("/api/u/{handle}/decks/{deck_id}/roles"))).await;
    assert_eq!(status, StatusCode::OK, "public roles: {public:?}");
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a public read is a pure function of its URL, so it's CDN-cacheable"
    );
    assert_eq!(
        public, body,
        "a shared deck and its owner's copy are the same deck"
    );
}

/// The mana base (issue #670): demand is read off the deck's costs, supply off the library,
/// and the public mirror is the same computation. The dummy catalog has no lands and no
/// producers, so what this can pin over HTTP is the demand side, the zero-supply verdict, and
/// that a checked non-producer is **not** reported as unchecked — the ingest stores `""` for
/// it, which is the whole point of the convention.
#[tokio::test]
async fn the_mana_base_reads_pips_against_sources() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "mana@example.com", PW).await;
    let cards = sample_card_ids(&app, 2).await;
    let (deck_id, _) = deck_with_cards(
        &app,
        &access,
        "Colours",
        "Commander",
        &[(cards[0].clone(), 3), (cards[1].clone(), 1)],
    )
    .await;

    let (status, headers, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/mana"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "mana failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"), "per-user data");
    // A Commander deck is judged against the 99-card column however few cards it holds.
    assert_eq!(body["table_size"], 99);
    assert_eq!(body["deck_size"], 4);
    assert_eq!(body["library_size"], 4);
    assert_eq!(body["land_count"], 0);
    assert_eq!(
        body["unchecked_count"], 0,
        "a seeded card was written with the empty-string convention, so it is checked"
    );
    assert!(
        body["source"]
            .as_str()
            .is_some_and(|s| s.contains("Karsten"))
    );
    assert!(!body["caveats"].as_array().expect("caveats").is_empty());

    // Every seeded card costs `{n}{C}` for one colour, so there is demand and no supply.
    let colors = body["colors"].as_array().expect("colors");
    assert!(!colors.is_empty(), "{body:?}");
    let total_pips: i64 = colors.iter().map(|c| c["pips"].as_i64().unwrap_or(0)).sum();
    assert_eq!(total_pips, 4, "one pip per copy: {body:?}");
    for color in colors {
        assert_eq!(color["sources"], 0);
        assert_eq!(color["status"], "short");
        assert!(color["sources_needed"].as_i64().is_some_and(|n| n > 0));
        assert_eq!(color["shortfall"], color["sources_needed"]);
        assert!(
            color["verdict"]
                .as_str()
                .is_some_and(|v| v.starts_with("Short ")),
            "{color:?}"
        );
        let demand = color["demand"].as_array().expect("demand");
        assert_eq!(demand.len() as i64, color["demand_count"]);
        assert!(
            demand.iter().all(|d| d["cost_key"].as_str().is_some()),
            "every counted card names the table row it was judged as"
        );
    }

    // The public mirror is the identical computation.
    let handle = share(&app, &access, "manabase", deck_id).await;
    let (status, headers, public) =
        send(&app, get(&format!("/api/u/{handle}/decks/{deck_id}/mana"))).await;
    assert_eq!(status, StatusCode::OK, "public mana: {public:?}");
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a public read is a pure function of its URL, so it's CDN-cacheable"
    );
    assert_eq!(
        public, body,
        "a shared deck and its owner's copy are the same deck"
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

#[tokio::test]
async fn a_library_too_big_to_shuffle_is_refused_not_allocated() {
    // A deck row's counts are caller-controlled (a million per finish) and a deck has no cap
    // on rows, while the shuffle materialises one slot per *copy*. Without a bound, one GET
    // — reachable unauthenticated once the owner shares the deck — could ask the server to
    // build and Fisher–Yates a multi-gigabyte vector to deal seven cards.
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "huge-deck@example.com", PW).await;
    let cards = sample_card_ids(&app, 1).await;
    let (deck_id, _) = deck_with_cards(&app, &access, "Too big", "Modern", &[]).await;

    // One row claiming a million copies.
    let (status, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let section = deck["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|s| s["name"] == "Creatures")
        .expect("Creatures")["id"]
        .as_i64()
        .expect("id");
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{}", cards[0]),
            &access,
            json!({ "quantity": 1_000_000, "foil_quantity": 0, "section_id": section }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the deck surface accepts it: {body:?}"
    );

    // The goldfish refuses rather than allocating it.
    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/goldfish"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body:?}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|e| e.contains("shuffled")),
        "the refusal should say why: {body:?}"
    );

    // Legality still answers, promptly: it counts copies rather than expanding them, so a
    // million-copy row is one fold step. (Modern's size rule is a floor, so a huge deck is
    // not a size breach — the copy limit is what catches it.)
    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["data"]["issues"][0]["status"], "over_limit");
    assert_eq!(body["data"]["issues"][0]["quantity"], 1_000_000);

    // …and so do the stats: the composition is a fold, never an expansion.
    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/stats"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["deck"]["total_copies"], 1_000_000);

    // Moving that row into the command zone of a format that has one is the path that would
    // have materialised a `&CardFacts` per copy. It now reports the count instead.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}"),
            &access,
            json!({ "name": "Too big", "format": "Commander" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let commander = deck["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|s| s["name"] == "Commander")
        .expect("Commander")["id"]
        .as_i64()
        .expect("id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{}/move", cards[0]),
            &access,
            json!({ "from_section_id": section, "to_section_id": commander }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/legality"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert!(
        body["data"]["violations"]
            .as_array()
            .expect("violations")
            .iter()
            .any(|v| v["rule"] == "command-zone"
                && v["message"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("1000000 cards in the command zone"))),
        "{body:?}"
    );
}

/// The two printings of the dummy catalog's reprinted card, dearest first: `(external id,
/// regular USD price)`. The pair is what makes "cheapest printing" testable offline.
async fn reprint_pair(app: &Router) -> Vec<(String, String)> {
    let (status, _, body) = send(
        app,
        get("/api/games/mtg/cards?name=Dummy%20Reprinted%20Relic&page_size=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "printing lookup failed: {body:?}");
    let mut pair: Vec<(String, String)> = body["data"]
        .as_array()
        .expect("printing data")
        .iter()
        .map(|c| {
            (
                c["id"].as_str().expect("id").to_string(),
                c["prices"]["usd"].as_str().expect("usd").to_string(),
            )
        })
        .collect();
    assert_eq!(pair.len(), 2, "dummy catalog must contain a reprint pair");
    pair.sort_by(|a, b| {
        b.1.parse::<f64>()
            .expect("price")
            .partial_cmp(&a.1.parse::<f64>().expect("price"))
            .expect("ordered")
    });
    pair
}

#[tokio::test]
async fn pricing_names_the_cheapest_printing_and_its_saving_at_the_rows_finish_split() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "pricing@example.com", PW).await;
    let pair = reprint_pair(&app).await;
    let (dear, dear_usd) = &pair[0];
    let (cheap, cheap_usd) = &pair[1];
    let dear_cents = (dear_usd.parse::<f64>().expect("price") * 100.0).round() as i64;
    let cheap_cents = (cheap_usd.parse::<f64>().expect("price") * 100.0).round() as i64;
    assert!(cheap_cents < dear_cents, "the pair must differ in price");

    // Two regular copies of the dear printing in the deck proper; the same card in the
    // maybeboard, which must not appear anywhere in the breakdown.
    let (deck_id, section_id) =
        deck_with_cards(&app, &access, "Priced", "Modern", &[(dear.clone(), 2)]).await;
    let (_, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    let maybeboard = detail["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|s| s["is_maybeboard"] == true)
        .expect("a maybeboard is seeded")["id"]
        .as_i64()
        .expect("id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{dear}"),
            &access,
            json!({ "quantity": 4, "foil_quantity": 0, "section_id": maybeboard }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, headers, pricing) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/pricing"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "pricing failed: {pricing:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));

    let lines = pricing["lines"].as_array().expect("lines");
    assert_eq!(
        lines.len(),
        1,
        "the maybeboard row is not listed: {lines:?}"
    );
    let line = &lines[0];
    assert_eq!(line["card"]["id"], *dear);
    assert_eq!(line["section_id"].as_i64(), Some(section_id));
    assert_eq!(line["quantity"], 2);
    assert_eq!(line["foil_quantity"], 0);
    let cents = |v: &Value| -> i64 {
        (v.as_str().expect("usd string").parse::<f64>().expect("usd") * 100.0).round() as i64
    };
    assert_eq!(cents(&line["price_usd"]), 2 * dear_cents);
    assert_eq!(line["cheapest"]["card"]["id"], *cheap);
    assert_eq!(cents(&line["cheapest"]["price_usd"]), 2 * cheap_cents);
    assert_eq!(cents(&line["saving_usd"]), 2 * (dear_cents - cheap_cents));

    // The totals agree with the deck's own summary and with each other.
    assert_eq!(pricing["total_usd"], detail["summary"]["total_value_usd"]);
    assert_eq!(cents(&pricing["total_usd"]), 2 * dear_cents);
    assert_eq!(
        cents(&pricing["saving_usd"]),
        2 * (dear_cents - cheap_cents)
    );
    assert_eq!(cents(&pricing["cheapest_total_usd"]), 2 * cheap_cents);
    assert_eq!(pricing["unpriced_count"], 0);
    assert_eq!(pricing["swappable_count"], 1);

    // Swap through the existing printing write — what the panel's button does — and the
    // breakdown now reports the row as already the cheapest: a zero saving, never null.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{dear}/printing"),
            &access,
            json!({ "new_card_id": cheap, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "swap failed: {body:?}");
    let (_, _, pricing) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/pricing"), &access),
    )
    .await;
    let line = &pricing["lines"][0];
    assert_eq!(line["card"]["id"], *cheap);
    assert_eq!(line["cheapest"]["card"]["id"], *cheap);
    assert_eq!(line["saving_usd"], "0.00");
    assert_eq!(pricing["saving_usd"], "0.00");
    assert_eq!(pricing["swappable_count"], 0);
    assert_eq!(pricing["total_usd"], pricing["cheapest_total_usd"]);
}

#[tokio::test]
async fn pricing_lists_most_expensive_first_and_a_foil_row_is_priced_as_foil() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "pricing-order@example.com", PW).await;
    let cards = sample_card_ids(&app, 3).await;
    let (deck_id, section_id) = deck_with_cards(
        &app,
        &access,
        "Ordered",
        "Modern",
        &[(cards[0].clone(), 1), (cards[1].clone(), 1)],
    )
    .await;
    // A third row held as one foil copy: its price is the foil price, not the regular.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{}", cards[2]),
            &access,
            json!({ "quantity": 0, "foil_quantity": 1, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, pricing) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/pricing"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "pricing failed: {pricing:?}");
    let lines = pricing["lines"].as_array().expect("lines");
    assert_eq!(lines.len(), 3);
    let prices: Vec<f64> = lines
        .iter()
        .map(|l| {
            l["price_usd"]
                .as_str()
                .expect("priced")
                .parse()
                .expect("usd")
        })
        .collect();
    assert!(
        prices.windows(2).all(|w| w[0] >= w[1]),
        "most expensive first: {prices:?}"
    );
    let foil_line = lines
        .iter()
        .find(|l| l["card"]["id"] == cards[2])
        .expect("the foil row is listed");
    assert_eq!(
        foil_line["price_usd"], foil_line["card"]["prices"]["usd_foil"],
        "one foil copy is worth the foil price"
    );
    assert_eq!(foil_line["cheapest"]["card"]["id"], cards[2]);
    assert_eq!(foil_line["saving_usd"], "0.00");
}

#[tokio::test]
async fn a_shared_decks_pricing_is_public_and_identical_to_the_owners() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "pricing-share@example.com", PW).await;
    let pair = reprint_pair(&app).await;
    let (deck_id, _) = deck_with_cards(
        &app,
        &access,
        "Shared pricing",
        "Modern",
        &[(pair[0].0.clone(), 3)],
    )
    .await;
    let handle = share(&app, &access, "pricer", deck_id).await;

    let (status, headers, public_pricing) = send(
        &app,
        get(&format!("/api/u/{handle}/decks/{deck_id}/pricing")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public pricing failed: {public_pricing:?}"
    );
    assert!(
        cache_control(&headers).is_some_and(|cc| cc.contains("max-age")),
        "a public read should be CDN-cacheable, got {:?}",
        cache_control(&headers)
    );
    let (_, _, owner_pricing) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/pricing"), &access),
    )
    .await;
    assert_eq!(
        public_pricing, owner_pricing,
        "a shared deck is the same deck"
    );
    assert_eq!(public_pricing["swappable_count"], 1);
}

#[tokio::test]
async fn a_nonfoil_copy_of_a_foil_only_printing_is_unpriced_not_zero() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "pricing-foil-only@example.com", PW).await;
    // The dummy catalog's foil-only showcase: a foil price, no regular one.
    let (status, _, body) = send(
        &app,
        get("/api/games/mtg/cards?name=Dummy%20Foil-Only%20Showcase&page_size=5"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "lookup failed: {body:?}");
    let card = &body["data"][0];
    assert!(card["prices"]["usd"].is_null() && card["prices"]["usd_foil"].is_string());
    let id = card["id"].as_str().expect("id").to_string();

    let (deck_id, _) = deck_with_cards(&app, &access, "Foil only", "Modern", &[(id, 1)]).await;
    let (status, _, pricing) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/pricing"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "pricing failed: {pricing:?}");
    let line = &pricing["lines"][0];
    assert!(
        line["price_usd"].is_null(),
        "unpriced, never 0.00: {line:?}"
    );
    assert!(line["saving_usd"].is_null());
    assert!(
        line["cheapest"].is_null(),
        "no printing is priced for a nonfoil copy: {line:?}"
    );
    assert_eq!(pricing["unpriced_count"], 1);
    assert!(pricing["total_usd"].is_null() || pricing["total_usd"] == "0.00");
}
