//! Per-user wish lists: authentication gating, per-user isolation, the
//! set/get/remove round trip, validation, the batch counts lookup, and the
//! `no-store` cache policy — the wish-list mirror of the collection tests (same
//! wire shapes, `/api/wishlist/...` routes, no import/sync).
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
    format!("/api/wishlist/mtg/cards/{id}")
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

/// Want one card, absolute counts, for the token's user.
async fn want_card(app: &Router, token: &str, id: &str, quantity: i64) {
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
    assert_eq!(status, StatusCode::OK, "want card failed: {body:?}");
}

#[tokio::test]
async fn wishlist_requires_authentication() {
    let app = test_app_with_catalog().await;

    // No bearer token -> 401, and per-user data must never be shared-cached.
    let (status, headers, _) = send(&app, get("/api/wishlist/mtg")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // The per-set landing and summary are per-user too: unauthenticated -> 401, no-store.
    for uri in ["/api/wishlist/mtg/sets", "/api/wishlist/mtg/summary"] {
        let (status, headers, _) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }

    // The batch counts lookup is a POST, but just as private.
    let (status, headers, _) = send(
        &app,
        json_post("/api/wishlist/mtg/counts", json!({ "ids": ["x"] })),
    )
    .await;
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
async fn set_get_and_remove_round_trip() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "wisher@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    // Fresh wish list is empty (and no-store).
    let (status, headers, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["total"], 0);
    assert!(body["data"].as_array().unwrap().is_empty());

    // Want 3 regular + 1 foil.
    let (status, _, body) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 3, "foil_quantity": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set failed: {body:?}");
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 1);

    // Single-entry read reflects the wish-list row.
    let (status, _, body) = send(&app, get_with_bearer(&card_path(id), &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["foil_quantity"], 1);

    // List carries one entry with the full card payload plus counts.
    let (status, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let entry = &body["data"][0];
    assert_eq!(entry["quantity"], 3);
    assert_eq!(entry["foil_quantity"], 1);
    assert_eq!(entry["card"]["id"], id.as_str());
    assert!(entry["card"]["name"].as_str().is_some(), "entry embeds the card");

    // Summary aggregates copies (3 + 1 = 4) over one unique card.
    let (status, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg/summary", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 4);

    // The per-set landing lists the one set that card belongs to, with wanted counts,
    // and a set-scoped summary matches the whole-wish-list one (only one set wanted).
    let set_code = entry["card"]["set_code"].as_str().expect("card set_code").to_string();
    let (status, headers, body) =
        send(&app, get_with_bearer("/api/wishlist/mtg/sets", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    let sets = body["data"].as_array().expect("sets array");
    assert_eq!(sets.len(), 1, "one wanted set");
    assert_eq!(sets[0]["code"], set_code);
    assert_eq!(sets[0]["owned_cards"], 1);
    assert_eq!(sets[0]["owned_copies"], 4);
    assert!(sets[0]["name"].as_str().is_some(), "set tile carries a name");

    let (status, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/wishlist/mtg/summary?set={set_code}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["unique_cards"], 1);
    assert_eq!(body["total_cards"], 4);
    // A set the user wants nothing in yields an empty scoped summary.
    let (_, _, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/summary?set=zzz-nope", &token),
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
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &token)).await;
    assert_eq!(body["total"], 1, "update must not create a second row");
    assert_eq!(body["data"][0]["quantity"], 5);

    // Zeroing both counts removes the card from the wish list.
    let (status, _, _) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 0, "foil_quantity": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &token)).await;
    assert_eq!(body["total"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &token)).await;
    assert_eq!(body["quantity"], 0);
    assert_eq!(body["foil_quantity"], 0);
}

#[tokio::test]
async fn wish_lists_are_isolated_per_user() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-wish@example.com", "password123").await;
    let (bob, _) = register(&app, "bob-wish@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    send(
        &app,
        json_with_bearer("PUT", &card_path(id), &alice, json!({ "quantity": 2, "foil_quantity": 0 })),
    )
    .await;

    // Bob sees nothing Alice added — a wish list is scoped to the token's user.
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &bob)).await;
    assert_eq!(body["total"], 0);
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &bob)).await;
    assert_eq!(body["quantity"], 0);

    // Alice still sees hers.
    let (_, _, body) = send(&app, get_with_bearer("/api/wishlist/mtg", &alice)).await;
    assert_eq!(body["total"], 1);
}

/// The wish list and the collection are separate holdings: owning a card doesn't put
/// it on the wish list, and wanting one doesn't add it to the collection.
#[tokio::test]
async fn wishlist_and_collection_are_independent() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "both@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];

    // Own 4 copies; want 1.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/collection/mtg/cards/{id}"),
            &token,
            json!({ "quantity": 4, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    want_card(&app, &token, id, 1).await;

    // Each side reads back only its own counts.
    let (_, _, body) = send(&app, get_with_bearer(&card_path(id), &token)).await;
    assert_eq!(body["quantity"], 1, "wish list holds its own count");
    let (_, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/collection/mtg/cards/{id}"), &token),
    )
    .await;
    assert_eq!(body["quantity"], 4, "collection is untouched by the wish list");

    // Removing the wish-list row leaves the collection holding alone.
    let (_, _, _) = send(
        &app,
        json_with_bearer("PUT", &card_path(id), &token, json!({ "quantity": 0, "foil_quantity": 0 })),
    )
    .await;
    let (_, _, body) = send(
        &app,
        get_with_bearer(&format!("/api/collection/mtg/cards/{id}"), &token),
    )
    .await;
    assert_eq!(body["quantity"], 4);
}

#[tokio::test]
async fn unknown_game_or_card_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "nf-wish@example.com", "password123").await;

    let (status, _, _) = send(&app, get_with_bearer("/api/wishlist/pokemon", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game is 404");

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/wishlist/mtg/cards/does-not-exist",
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
    // (`dmb`), so `include_related` should fold both into one wanted listing — the
    // wish-list mirror of the catalog's include-related view.
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "grouped-wish@example.com", "password123").await;

    let base_id = first_set_card_id(&app, "dmb").await;
    let token_id = first_set_card_id(&app, "tdmb").await;
    want_card(&app, &token, &base_id, 1).await;
    want_card(&app, &token, &token_id, 1).await;

    // A single-set scope sees only that set's wanted card.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg?set=dmb", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1, "plain set scope stays a single set");
    assert_eq!(body["data"][0]["card"]["set_code"], "dmb");

    // Folding in related sets spans the whole group (base + its token sub-set).
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg?set=dmb&include_related=true", &token),
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
        get_with_bearer("/api/wishlist/mtg?set=tdmb&include_related=true", &token),
    )
    .await;
    assert_eq!(body["total"], 2, "grouped view is the same from a sub-set");
}

#[tokio::test]
async fn wishlist_drops_route_gates_and_404s() {
    let app = test_app_with_catalog().await;

    // Per-user, so unauthenticated -> 401 and never shared-cached.
    let (status, headers, _) = send(&app, get("/api/wishlist/mtg/sets/dmb/drops")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (token, _) = register(&app, "drops-wish@example.com", "password123").await;

    // A set with no Secret Lair drop snapshot is a 404 (browse it flat instead), and an
    // error response is no-store.
    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/sets/dmb/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-drop set: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Unknown game/set are 404 too.
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/wishlist/pokemon/sets/dmb/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown game");
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/wishlist/mtg/sets/zzz-nope/drops", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown set");
}

#[tokio::test]
async fn out_of_bounds_quantities_are_rejected() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "neg-wish@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;

    // Negative and oversized counts are both a 422 (the shared per-card bounds).
    for body in [
        json!({ "quantity": -1, "foil_quantity": 0 }),
        json!({ "quantity": 0, "foil_quantity": 1_000_001 }),
    ] {
        let (status, _, _) = send(
            &app,
            json_with_bearer("PUT", &card_path(&ids[0]), &token, body),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

/// `POST .../counts` returns only the wish-listed subset of the requested ids (an
/// unwanted card is absent, not a zero entry), tolerates an empty list, and refuses
/// an oversized batch.
#[tokio::test]
async fn counts_batch_returns_wanted_cards_only_and_caps_the_id_list() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "counts-wish@example.com", "password123").await;
    let ids = sample_card_ids(&app, 2).await;
    let (wanted, unwanted) = (&ids[0], &ids[1]);
    want_card(&app, &token, wanted, 2).await;

    // Only the wanted card appears in the map; the unwanted one is simply absent.
    let (status, headers, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/wishlist/mtg/counts",
            &token,
            json!({ "ids": [wanted, unwanted] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "counts failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["data"][wanted]["quantity"], 2);
    assert!(body["data"].get(unwanted).is_none(), "unwanted id must be absent");

    // An empty id list is an empty map, not an error.
    let (status, _, body) = send(
        &app,
        json_with_bearer("POST", "/api/wishlist/mtg/counts", &token, json!({ "ids": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].as_object().is_some_and(|m| m.is_empty()));

    // More than the server cap (500 ids) is a 422, before any lookup runs.
    let too_many: Vec<String> = (0..501).map(|i| format!("id-{i}")).collect();
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/wishlist/mtg/counts",
            &token,
            json!({ "ids": too_many }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn unknown_sort_or_dir_is_rejected() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "sort-wish@example.com", "password123").await;

    // An unrecognised sort key or direction is a 422, consistent with a malformed `q`.
    for uri in ["/api/wishlist/mtg?sort=nonsense", "/api/wishlist/mtg?dir=sideways"] {
        let (status, _, body) = send(&app, get_with_bearer(uri, &token)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}: {body:?}");
    }
}
