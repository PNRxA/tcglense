//! Per-user card collections: authentication gating, per-user isolation, the
//! set/get/remove round trip, validation, and the `no-store` cache policy.
//!
//! These drive the real router over the seeded dummy catalog, so a card can be
//! added by its real external id and read back in the full catalog `Card` shape.

use super::harness::*;

/// Grab `n` real card external ids from the seeded catalog.
async fn sample_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(status, StatusCode::OK, "listing seeded cards failed: {body:?}");
    let data = body["data"].as_array().expect("cards data array");
    assert!(data.len() >= n, "need >= {n} seeded cards, got {}", data.len());
    data.iter()
        .take(n)
        .map(|c| c["id"].as_str().expect("card id").to_string())
        .collect()
}

fn card_path(id: &str) -> String {
    format!("/api/collection/mtg/cards/{id}")
}

/// The first card external id in a given seeded set (by collector number).
async fn first_set_card_id(app: &Router, set_code: &str) -> String {
    let (status, _, body) = send(
        app,
        get(&format!("/api/games/mtg/sets/{set_code}/cards?page_size=1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "listing set {set_code} failed: {body:?}");
    body["data"][0]["id"].as_str().expect("card id").to_string()
}

/// Grab `n` real card external ids that carry a USD price in the seeded catalog, so a
/// valuation over them is a real number rather than null.
async fn priced_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=50")).await;
    assert_eq!(status, StatusCode::OK, "listing seeded cards failed: {body:?}");
    let ids: Vec<String> = body["data"]
        .as_array()
        .expect("cards data array")
        .iter()
        .filter(|c| c["prices"]["usd"].as_str().is_some())
        .take(n)
        .map(|c| c["id"].as_str().expect("card id").to_string())
        .collect();
    assert!(ids.len() >= n, "need >= {n} priced seeded cards, got {}", ids.len());
    ids
}

/// Own one card, absolute counts, for the token's user.
async fn own_card(app: &Router, token: &str, id: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &card_path(id),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own card failed: {body:?}");
}

#[tokio::test]
async fn collection_requires_authentication() {
    let app = test_app_with_catalog().await;

    // No bearer token -> 401, and per-user data must never be shared-cached.
    let (status, headers, _) = send(&app, get("/api/collection/mtg")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // The per-set landing is per-user too: unauthenticated -> 401, no-store.
    let (status, headers, _) = send(&app, get("/api/collection/mtg/sets")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    let ids = sample_card_ids(&app, 1).await;
    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("PUT")
            .uri(card_path(&ids[0]))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "quantity": 1, "foil_quantity": 0 }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn value_history_requires_authentication() {
    let app = test_app_with_catalog().await;

    // Per-user data, so no token -> 401 and never shared-cached.
    let (status, headers, _) = send(&app, get("/api/collection/mtg/value-history")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

/// The value-over-time series is add-date-clamped and per-user: the seeded catalog carries
/// a year of daily prices, but a holding added *today* contributes only to today's point —
/// which must equal the collection summary's current total.
#[tokio::test]
async fn value_history_clamps_to_add_date_and_matches_summary() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "history@example.com", "password123").await;

    // Empty collection -> an empty series (and no-store, per-user).
    let (status, headers, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(body["data"].as_array().unwrap().is_empty(), "empty collection has no series");

    // Own a few priced cards today.
    let ids = priced_card_ids(&app, 3).await;
    for id in &ids {
        own_card(&app, &token, id, 2).await;
    }

    // The summary's current total is the yardstick: the newest history point (today) must
    // equal it, since a just-added holding is add-date-clamped into today and the seed
    // anchors today's snapshot to each card's current price.
    let (_, _, summary) = send(&app, get_with_bearer("/api/collection/mtg/summary", &token)).await;
    let total_today = summary["total_value_usd"].clone();
    assert!(total_today.is_string(), "priced holdings -> a real total, got {total_today:?}");

    // Full daily series: ~a year of history exists, but every day before today predates the
    // holdings' add-date, so only the newest point carries a value — the add-date clamp.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let points = body["data"].as_array().expect("value-history data array");
    assert!(points.len() > 300, "the full daily series spans ~a year, got {}", points.len());

    // Dates strictly ascend; only the final point (today) is priced, the rest are null.
    let mut prev = "";
    for (i, point) in points.iter().enumerate() {
        let date = point["date"].as_str().expect("point date");
        assert!(date > prev, "dates ascend: {prev:?} !< {date:?}");
        prev = date;
        if i + 1 < points.len() {
            assert!(
                point["value_usd"].is_null(),
                "day {date} predates every holding, so it contributes nothing"
            );
        }
    }
    assert_eq!(
        points.last().unwrap()["value_usd"],
        total_today,
        "today's value matches the summary total"
    );

    // A windowed range downsamples but keeps the same clamp: last point priced, rest null.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history?range=1y", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let windowed = body["data"].as_array().expect("windowed data array");
    assert!(!windowed.is_empty() && windowed.len() < points.len(), "1y weekly < full daily");
    assert_eq!(windowed.last().unwrap()["value_usd"], total_today);

    // An unknown range is a 422, like the per-card price chart.
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/mtg/value-history?range=week", &token),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A second user sees only their own (empty) history.
    let (bob, _) = register(&app, "bob-history@example.com", "password123").await;
    let (_, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &bob)).await;
    assert!(body["data"].as_array().unwrap().is_empty(), "isolation: bob owns nothing");
}

#[tokio::test]
async fn set_get_and_remove_round_trip() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "collector@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    // Fresh collection is empty (and no-store).
    let (status, headers, body) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["total"], 0);
    assert!(body["data"].as_array().unwrap().is_empty());

    // Add 3 regular + 1 foil.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 3, "foil_quantity": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set failed: {body:?}");
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 1);

    // Single-entry read reflects the holding.
    let (status, _, body) = send(&app, get_with_bearer(&card_path(id), &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 1);

    // List carries one entry with the full card payload plus counts.
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let entry = &body["data"][0];
    assert_eq!(entry["quantity"], 3);
    assert_eq!(entry["foil_quantity"], 1);
    assert_eq!(entry["card"]["id"], id.as_str());
    assert!(entry["card"]["name"].as_str().is_some(), "entry embeds the card");

    // Summary aggregates copies (3 + 1 = 4) over one unique card.
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/summary", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 4);

    // The per-set landing lists the one set that card belongs to, with owned counts,
    // and a set-scoped summary matches the whole-collection one (only one set owned).
    let set_code = entry["card"]["set_code"].as_str().expect("card set_code").to_string();
    let (status, headers, body) =
        send(&app, get_with_bearer("/api/collection/mtg/sets", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    let sets = body["data"].as_array().expect("sets array");
    assert_eq!(sets.len(), 1, "one owned set");
    assert_eq!(sets[0]["code"], set_code);
    assert_eq!(sets[0]["owned_cards"], 1);
    assert_eq!(sets[0]["owned_copies"], 4);
    assert!(sets[0]["name"].as_str().is_some(), "set tile carries a name");

    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/collection/mtg/summary?set={set_code}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 4);
    // A set the user owns nothing in yields an empty scoped summary.
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/summary?set=zzz-nope", &token),
    )
    .await;
    assert_eq!(body["unique_cards"], 0);
    assert_eq!(body["total_cards"], 0);

    // Updating the same card upserts (no duplicate row).
    let (_, _, body) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 5, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(body["quantity"], 5);
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(body["total"], 1, "update must not create a second row");
    assert_eq!(body["data"][0]["quantity"], 5);

    // Zeroing both counts removes the card from the collection.
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 0, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(body["total"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &token)).await;
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);
}

#[tokio::test]
async fn collections_are_isolated_per_user() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice@example.com", "password123").await;
    let (bob, _) = register(&app, "bob@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    send(
        &app,
        json_with_bearer("PUT", &card_path(id), &alice, json!({ "quantity": 2, "foil_quantity": 0 })),
    )
    .await;

    // Bob sees nothing Alice added — ownership is scoped to the token's user.
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &bob)).await;
    assert_eq!(body["total"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &bob)).await;
    assert_eq!(body["quantity"], 0);

    // Alice still sees hers.
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg", &alice)).await;
    assert_eq!(body["total"], 1);
}

#[tokio::test]
async fn unknown_game_or_card_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "nf@example.com", "password123").await;

    let (status, _, _) = send(&app, get_with_bearer("/api/collection/pokemon", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game is 404");

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/cards/does-not-exist",
            &token,
            json!({ "quantity": 1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown card is 404");
}

#[tokio::test]
async fn include_related_spans_the_set_group() {
    // The seeded dummy catalog has a token child set (`tdmb`) hanging off the base set
    // (`dmb`), so `include_related` should fold both into one owned listing — the
    // collection mirror of the catalog's include-related view.
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "grouped@example.com", "password123").await;

    let base_id = first_set_card_id(&app, "dmb").await;
    let token_id = first_set_card_id(&app, "tdmb").await;
    own_card(&app, &token, &base_id, 1).await;
    own_card(&app, &token, &token_id, 1).await;

    // A single-set scope sees only that set's owned card.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg?set=dmb", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1, "plain set scope stays a single set");
    assert_eq!(body["data"][0]["card"]["set_code"], "dmb");

    // Folding in related sets spans the whole group (base + its token sub-set).
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg?set=dmb&include_related=true", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2, "include_related spans the group");
    let codes: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["card"]["set_code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"dmb") && codes.contains(&"tdmb"), "got {codes:?}");

    // Entering from the sub-set resolves the same group (rooted at `dmb`).
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg?set=tdmb&include_related=true", &token),
    )
    .await;
    assert_eq!(body["total"], 2, "grouped view is the same from a sub-set");
}

#[tokio::test]
async fn collection_drops_route_gates_and_404s() {
    let app = test_app_with_catalog().await;

    // Per-user, so unauthenticated -> 401 and never shared-cached.
    let (status, headers, _) = send(&app, get("/api/collection/mtg/sets/dmb/drops")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (token, _) = register(&app, "drops@example.com", "password123").await;

    // A set with no Secret Lair drop snapshot is a 404 (browse it flat instead), and an
    // error response is no-store.
    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/sets/dmb/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-drop set: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Unknown game/set are 404 too.
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/pokemon/sets/dmb/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game");
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/mtg/sets/zzz-nope/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown set");
}

#[tokio::test]
async fn negative_quantity_is_rejected() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "neg@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &card_path(&ids[0]),
            &token,
            json!({ "quantity": -1, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// `POST .../owned` — the batch lookup behind the browse-grid ownership badges — is
/// scoped to the caller: it returns only the caller's own holdings, never another
/// user's. This endpoint fires on every public browse page for a signed-in visitor,
/// so a regression broadening its `user_id` filter would leak every user's ownership
/// to any authenticated viewer.
#[tokio::test]
async fn owned_batch_is_isolated_per_user() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-owned@example.com", "password123").await;
    let (bob, _) = register(&app, "bob-owned@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    own_card(&app, &alice, id, 3).await;

    // Bob asks for the very card Alice owns — he owns none, so it's absent from his
    // map (and per-user data is never shared-cached).
    let (status, headers, body) = send(
        &app,
        json_with_bearer("POST", "/api/collection/mtg/owned", &bob, json!({ "ids": [id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "owned failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(
        body["data"].get(id.as_str()).is_none(),
        "another user's holding must not leak: {body:?}"
    );

    // Alice sees her own count.
    let (_, _, body) = send(
        &app,
        json_with_bearer("POST", "/api/collection/mtg/owned", &alice, json!({ "ids": [id] })),
    )
    .await;
    assert_eq!(body["data"][id.as_str()]["quantity"], 3);
}

/// A write is keyed to the token's user: two users PUTting the *same* card id create
/// two distinct rows (never a clobber), and every read aggregation — the single-entry
/// read, the summary, and the owned-set landing — reflects only the caller's own
/// holdings. A regression narrowing the upsert conflict key to `(game, card_id)`, or
/// dropping the `user_id` filter from an aggregation, would silently corrupt or leak
/// one user's collection into another's.
#[tokio::test]
async fn writes_and_aggregations_are_isolated_between_users() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-write@example.com", "password123").await;
    let (bob, _) = register(&app, "bob-write@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    // Both own the SAME card, different counts, via the same path (different token).
    let (_, _, a_body) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &alice, json!({ "quantity": 2, "foil_quantity": 1 })),
    )
    .await;
    assert_eq!(a_body["quantity"], 2);
    let (_, _, b_body) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &bob, json!({ "quantity": 5, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(b_body["quantity"], 5);

    // Distinct rows: Bob's write did not overwrite Alice's holding, and vice versa.
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &alice)).await;
    assert_eq!(body["quantity"], 2, "alice's count is unchanged by bob's write");
    assert_eq!(body["foil_quantity"], 1);
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &bob)).await;
    assert_eq!(body["quantity"], 5);
    assert_eq!(body["foil_quantity"], 0);

    // Summaries are per-user: Alice 2+1=3 copies, Bob 5 copies, each one unique card.
    let (_, _, a_sum) = send(&app, get_with_bearer("/api/collection/mtg/summary", &alice)).await;
    assert_eq!(a_sum["unique_cards"], 1);
    assert_eq!(a_sum["total_cards"], 3);
    let (_, _, b_sum) = send(&app, get_with_bearer("/api/collection/mtg/summary", &bob)).await;
    assert_eq!(b_sum["unique_cards"], 1);
    assert_eq!(b_sum["total_cards"], 5);

    // The owned-set landing aggregates only the caller's copies (3 vs 5).
    let (_, _, a_sets) = send(&app, get_with_bearer("/api/collection/mtg/sets", &alice)).await;
    assert_eq!(a_sets["data"][0]["owned_copies"], 3);
    let (_, _, b_sets) = send(&app, get_with_bearer("/api/collection/mtg/sets", &bob)).await;
    assert_eq!(b_sets["data"][0]["owned_copies"], 5);
}

/// The `quantity` sort orders the owned-card list by total copies through the full HTTP
/// path — most first by default, fewest first when reversed (issue #228). The query-level
/// ordering (incl. folding in foils) is unit-tested in `handlers::collection::tests`; this
/// pins the `sort`/`dir` param plumbing end to end.
#[tokio::test]
async fn quantity_sort_orders_the_owned_list_by_copies() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "sort-qty@example.com", "password123").await;

    // Three distinct cards owned at distinct copy counts (1, 5, 3).
    let ids = sample_card_ids(&app, 3).await;
    own_card(&app, &token, &ids[0], 1).await;
    own_card(&app, &token, &ids[1], 5).await;
    own_card(&app, &token, &ids[2], 3).await;

    fn quantities(body: &serde_json::Value) -> Vec<i64> {
        body["data"]
            .as_array()
            .expect("collection data array")
            .iter()
            .map(|e| e["quantity"].as_i64().expect("quantity"))
            .collect()
    }

    // Default direction is most copies first.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg?sort=quantity", &token)).await;
    assert_eq!(status, StatusCode::OK, "quantity sort failed: {body:?}");
    assert_eq!(quantities(&body), vec![5, 3, 1]);

    // An explicit ascending direction reverses it (fewest copies first).
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg?sort=quantity&dir=asc", &token)).await;
    assert_eq!(status, StatusCode::OK, "quantity asc sort failed: {body:?}");
    assert_eq!(quantities(&body), vec![1, 3, 5]);
}

/// The `content-disposition` header value as a string, or `""` if absent.
fn content_disposition(headers: &HeaderMap) -> &str {
    headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

/// The CSV export requires auth, emits a `no-store` text/csv download per shape, and
/// round-trips a holding's finishes into one row each. Bad shapes are a 422.
#[tokio::test]
async fn export_requires_auth_and_produces_provider_csv() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "exporter@example.com", "password123").await;

    // Own one card in both finishes (2 regular + 1 foil) and another regular-only.
    let ids = sample_card_ids(&app, 2).await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &card_path(&ids[0]),
            &token,
            json!({ "quantity": 2, "foil_quantity": 1 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own card failed: {body:?}");
    own_card(&app, &token, &ids[1], 3).await;

    // Unauthenticated -> 401, and per-user data must never be shared-cached.
    let (status, headers, _) = send_text(&app, get("/api/collection/mtg/export")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Archidekt (the default shape): a no-store text/csv download with the 23-column
    // header and one row per non-empty finish bucket, each carrying its Scryfall id.
    let (status, headers, body) =
        send_text(&app, get_with_bearer("/api/collection/mtg/export", &token)).await;
    assert_eq!(status, StatusCode::OK, "export failed: {body}");
    assert_eq!(content_type(&headers), Some("text/csv; charset=utf-8"));
    assert_eq!(cache_control(&headers), Some("no-store"));
    let disposition = content_disposition(&headers);
    assert!(disposition.contains("attachment"), "disposition: {disposition}");
    assert!(
        disposition.contains("tcglense-mtg-collection-archidekt.csv"),
        "disposition: {disposition}"
    );
    let mut lines = body.lines();
    assert_eq!(
        lines.next().unwrap(),
        "Quantity,Name,Finish,Condition,Date Added,Language,Purchase Price,Tags,Edition Name,\
         Edition Code,Multiverse Id,Scryfall ID,MTGO ID,Collector Number,Mana Value,Colors,\
         Identities,Mana cost,Types,Sub-types,Super-types,Rarity,Scryfall Oracle ID"
    );
    let data: Vec<&str> = lines.collect();
    assert_eq!(data.len(), 3, "regular + foil + regular rows: {data:?}");
    assert!(data.iter().any(|r| r.contains(&ids[0])), "card A id missing: {data:?}");
    assert!(data.iter().any(|r| r.contains(&ids[1])), "card B id missing: {data:?}");
    assert!(data.iter().any(|r| r.contains(",Foil,")), "no foil row: {data:?}");

    // Moxfield shape: quote-every-field, its own 13-column header + filename.
    let (status, headers, body) = send_text(
        &app,
        get_with_bearer("/api/collection/mtg/export?format=moxfield", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "moxfield export failed: {body}");
    assert!(
        content_disposition(&headers).contains("tcglense-mtg-collection-moxfield.csv"),
        "disposition: {}",
        content_disposition(&headers)
    );
    assert_eq!(
        body.lines().next().unwrap(),
        "\"Count\",\"Tradelist Count\",\"Name\",\"Edition\",\"Condition\",\"Language\",\"Foil\",\
         \"Tags\",\"Last Modified\",\"Collector Number\",\"Alter\",\"Proxy\",\"Purchase Price\""
    );

    // An unknown shape is a 422 (never a silent default to some other format).
    let (status, _, _) = send_text(
        &app,
        get_with_bearer("/api/collection/mtg/export?format=deckbox", &token),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
