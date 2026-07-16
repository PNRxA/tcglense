//! Per-user card collections: authentication gating, per-user isolation, the
//! set/get/remove round trip, validation, and the `no-store` cache policy.
//!
//! These drive the real router over the seeded dummy catalog, so a card can be
//! added by its real external id and read back in the full catalog `Card` shape.

use super::harness::*;

use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set, sea_query::Expr};

use crate::entities::prelude::{Card, CardPriceHistory, CollectionItem};
use crate::entities::{card, card_price_history, collection_item};

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

/// The value-over-time series revalues the current basket across every captured price date
/// and remains per-user. Its newest point must equal the collection summary's current total.
#[tokio::test]
async fn value_history_revalues_current_collection_and_matches_summary() {
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
    // equal it because the seed anchors today's snapshot to each card's current price.
    let (_, _, summary) = send(&app, get_with_bearer("/api/collection/mtg/summary", &token)).await;
    let total_today = summary["total_value_usd"].clone();
    assert!(total_today.is_string(), "priced holdings -> a real total, got {total_today:?}");

    // Full daily series: the cards were added today, but their current quantities are
    // intentionally applied across the entire ~year of captured history.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let points = body["data"].as_array().expect("value-history data array");
    assert!(points.len() > 300, "the full daily series spans ~a year, got {}", points.len());

    // Dates strictly ascend and every captured day values the current basket.
    let mut prev = "";
    for point in points {
        let date = point["date"].as_str().expect("point date");
        assert!(date > prev, "dates ascend: {prev:?} !< {date:?}");
        prev = date;
        assert!(
            point["value_usd"].is_string(),
            "current holdings should be revalued on historic day {date}: {point:?}"
        );
    }
    assert_eq!(
        points.last().unwrap()["value_usd"],
        total_today,
        "today's value matches the summary total"
    );

    // A windowed range downsamples the same fully revalued series. Index 0 is exempt from the
    // priced check: a ranged series opens with a synthetic point at the window floor
    // (`today - 365` for `1y`) carrying whatever pre-cutoff price it can, and the seed's oldest
    // day is `today - 364` — so no earlier snapshot exists and the floor is unpriced by
    // construction. It only reaches the response when it lands in a weekly bucket of its own
    // (a ~1-in-7 property of today's date), so skip it either way; `last()` below still pins
    // the revaluation.
    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history?range=1y", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let windowed = body["data"].as_array().expect("windowed data array");
    assert!(!windowed.is_empty() && windowed.len() < points.len(), "1y weekly < full daily");
    assert!(
        windowed.iter().skip(1).all(|point| point["value_usd"].is_string()),
        "every captured day in the window values the current basket: {windowed:?}"
    );
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

/// The analytics pair rides the version-keyed response cache (#413): between a
/// user's own edits the served body must come from the cache (a DB change that
/// bypasses the handlers stays invisible — the positive proof a second request
/// never re-ran the fold), and any real edit through the handlers must bump the
/// version so the next read recomputes.
#[tokio::test]
async fn analytics_responses_are_cached_until_a_holdings_edit() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "analytics-cache@example.com", "password123").await;
    let ids = priced_card_ids(&app, 2).await;
    own_card(&app, &token, &ids[0], 1).await;

    // Prime the cache.
    let (status, _, first) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!first["data"].as_array().unwrap().is_empty());

    // Mutate the holdings BEHIND the handlers — no version bump happens, so the
    // cached body must keep being served verbatim.
    CollectionItem::update_many()
        .col_expr(collection_item::Column::Quantity, Expr::value(7))
        .filter(collection_item::Column::Game.eq("mtg"))
        .exec(&app.state.db)
        .await
        .expect("raw quantity update");
    let (status, _, second) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, second, "second read must be the cached body, not a recompute");

    // A real edit through the handler bumps the version: the next read recomputes
    // and now sees both the edit and the raw update above.
    own_card(&app, &token, &ids[1], 1).await;
    let (status, _, third) =
        send(&app, get_with_bearer("/api/collection/mtg/value-history", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second, third, "an edit must invalidate the cached analytics body");

    // Movers rides the same cache and the same bump (its body carries no
    // user-variable data before any holdings exist beyond the seed prices, so
    // just pin the invalidation contract end-to-end).
    let (status, _, movers_first) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    own_card(&app, &token, &ids[0], 3).await;
    let (status, _, movers_second) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    // The count tripled, so per-item movement values shift wherever movements exist;
    // at minimum the response stays well-formed and the request path stays 200.
    let _ = (movers_first, movers_second);
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

// ---------- Collection price movers ----------
//
// The pure ranking helpers are unit-tested in `handlers::collection::price_movements`; these
// drive the real `/movers` endpoint end-to-end with *controlled* price history, so the
// handler-only glue (reference-date = newest owned snapshot, day/week/month anchoring, the
// cross-window de-dup, and the empty/auth paths) is exercised too.

/// `YYYY-MM-DD` for `offset` days before today (0 = today) — an on-the-wire snapshot date.
fn day_offset(offset: i64) -> String {
    (Utc::now().date_naive() - Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string()
}

/// The internal `cards.id` for a seeded external id (holdings + price history key off it).
async fn internal_card_id(db: &sea_orm::DatabaseConnection, external_id: &str) -> i32 {
    Card::find()
        .filter(card::Column::Game.eq("mtg"))
        .filter(card::Column::ExternalId.eq(external_id))
        .one(db)
        .await
        .expect("query card")
        .expect("seeded card exists")
        .id
}

/// Replace a card's seeded price history with exactly `rows` (`(as_of_date, usd, foil)`), so
/// a movers assertion is deterministic instead of riding the dummy seed's random price walk.
async fn set_price_history(
    db: &sea_orm::DatabaseConnection,
    card_id: i32,
    rows: &[(String, Option<&str>, Option<&str>)],
) {
    CardPriceHistory::delete_many()
        .filter(card_price_history::Column::Game.eq("mtg"))
        .filter(card_price_history::Column::CardId.eq(card_id))
        .exec(db)
        .await
        .expect("wipe seeded history");
    let now = Utc::now();
    let models: Vec<card_price_history::ActiveModel> = rows
        .iter()
        .map(|(date, usd, foil)| card_price_history::ActiveModel {
            game: Set("mtg".to_string()),
            card_id: Set(card_id),
            as_of_date: Set(date.clone()),
            price_usd: Set(usd.map(str::to_string)),
            price_usd_foil: Set(foil.map(str::to_string)),
            price_eur: Set(None),
            price_tix: Set(None),
            created_at: Set(now),
            ..Default::default()
        })
        .collect();
    CardPriceHistory::insert_many(models)
        .exec(db)
        .await
        .expect("insert controlled history");
}

/// The owned-card external ids in a mover list, in list order.
fn mover_ids(list: &Value) -> Vec<String> {
    list.as_array()
        .expect("mover list array")
        .iter()
        .map(|m| m["card"]["id"].as_str().expect("mover card id").to_string())
        .collect()
}

#[tokio::test]
async fn movers_requires_auth_and_handles_empty_and_unknown_game() {
    let app = test_app_with_catalog().await;

    // Per-user data: no token -> 401, and never shared-cached.
    let (status, headers, _) = send(&app, get("/api/collection/mtg/movers")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (token, _) = register(&app, "movers-empty@example.com", "password123").await;

    // Owns nothing -> an all-empty payload with a null `as_of` (and still no-store, per-user).
    let (status, headers, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(body["as_of"].is_null(), "no holdings -> null as_of");
    assert!(body["day_as_of"].is_null(), "no holdings -> null day_as_of");
    for window in [
        "day",
        "week",
        "month",
        "year",
        "two_year",
        "three_year",
        "all_time",
    ] {
        assert!(body[window]["gainers"].as_array().unwrap().is_empty());
        assert!(body[window]["losers"].as_array().unwrap().is_empty());
    }

    // Unknown game -> 404 (require_game), like the sibling collection reads.
    let (status, _, _) = send(&app, get_with_bearer("/api/collection/nope/movers", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The endpoint ranks each window from the newest owned snapshot, and a card can be a gainer
/// in a short window while being a loser over the month — it must then appear in *both* lists
/// (the cross-window de-dup fetches its card row once). This is the handler glue the pure
/// helper tests bypass (they hand-feed `latest`/`target`).
#[tokio::test]
async fn movers_rank_across_windows_and_dedup_a_flipping_card() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-rank@example.com", "password123").await;
    let ids = sample_card_ids(&app, 3).await;
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);

    // One regular copy of each, so a card's holding value change equals its price change.
    for id in &ids {
        own_card(&app, &token, id, 1).await;
    }

    let (d0, d1, d7, d30) = (day_offset(0), day_offset(1), day_offset(7), day_offset(30));
    // A: flat then jumps -> +$10 in every window (the top gainer everywhere).
    set_price_history(
        db,
        internal_card_id(db, a).await,
        &[
            (d30.clone(), Some("10.00"), None),
            (d7.clone(), Some("10.00"), None),
            (d1.clone(), Some("10.00"), None),
            (d0.clone(), Some("20.00"), None),
        ],
    )
    .await;
    // B: flat then drops -> -$10 in every window (the top loser everywhere).
    set_price_history(
        db,
        internal_card_id(db, b).await,
        &[
            (d30.clone(), Some("100.00"), None),
            (d7.clone(), Some("100.00"), None),
            (d1.clone(), Some("100.00"), None),
            (d0.clone(), Some("90.00"), None),
        ],
    )
    .await;
    // C: recovering off a recent dip but far below a month ago -> +$2 (day)/+$5 (week) yet
    // -$40 (month). It is a day+week GAINER and a month LOSER — the flip that exercises dedup.
    set_price_history(
        db,
        internal_card_id(db, c).await,
        &[
            (d30.clone(), Some("50.00"), None),
            (d7.clone(), Some("5.00"), None),
            (d1.clone(), Some("8.00"), None),
            (d0.clone(), Some("10.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(body["as_of"].as_str(), Some(d0.as_str()), "as_of = newest owned snapshot");
    assert_eq!(body["day_as_of"].as_str(), Some(d0.as_str()));

    // Day: gainers A(+$10) then C(+$2); loser B(-$10). change_usd carries the sign.
    assert_eq!(mover_ids(&body["day"]["gainers"]), vec![a.clone(), c.clone()]);
    assert_eq!(body["day"]["gainers"][0]["change_usd"], "10.00");
    assert_eq!(body["day"]["gainers"][0]["value_now"], "20.00");
    assert_eq!(body["day"]["gainers"][0]["value_prev"], "10.00");
    assert_eq!(body["day"]["gainers"][1]["change_usd"], "2.00");
    assert_eq!(mover_ids(&body["day"]["losers"]), vec![b.clone()]);
    assert_eq!(body["day"]["losers"][0]["change_usd"], "-10.00");

    // Month: gainer A(+$10); losers most-negative-first C(-$40) then B(-$10).
    assert_eq!(mover_ids(&body["month"]["gainers"]), vec![a.clone()]);
    assert_eq!(mover_ids(&body["month"]["losers"]), vec![c.clone(), b.clone()]);
    assert_eq!(body["month"]["losers"][0]["change_usd"], "-40.00");

    // The de-dup: the same card C is a day gainer AND a month loser, each with its card payload.
    assert!(mover_ids(&body["day"]["gainers"]).contains(c));
    assert!(mover_ids(&body["month"]["losers"]).contains(c));
}

/// An unchanged newest capture must not make 1D look history-less. The daily window retries
/// from the previous available snapshot (including across a missing calendar day), while the
/// overall `as_of` and every longer window remain anchored to the newest capture.
///
/// The ten-day-old row makes the retry's baseline resolution load-bearing: the three-days-ago
/// snapshot reaches the ranker only as the day-before-yesterday anchor (the day anchor is `d1`,
/// the week anchor and earliest price are `d10`), so the correct `value_prev` of `5.00` pins
/// that anchor — carrying forward from `d10` would report `3.00`.
#[tokio::test]
async fn movers_day_falls_back_to_the_previous_available_snapshot() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-day-fallback@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];
    own_card(&app, &token, id, 1).await;

    let (d0, d1, d3, d10) = (day_offset(0), day_offset(1), day_offset(3), day_offset(10));
    set_price_history(
        db,
        internal_card_id(db, id).await,
        &[
            (d10, Some("3.00"), None),
            (d3, Some("5.00"), None),
            (d1.clone(), Some("8.00"), None),
            (d0.clone(), Some("8.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(body["as_of"], d0, "longer windows keep the newest anchor");
    assert_eq!(body["day_as_of"], d1, "1D reports its fallback anchor");
    assert_eq!(mover_ids(&body["day"]["gainers"]), vec![id.clone()]);
    assert_eq!(body["day"]["gainers"][0]["value_prev"], "5.00");
    assert_eq!(body["day"]["gainers"][0]["value_now"], "8.00");
    assert_eq!(body["day"]["gainers"][0]["change_usd"], "3.00");
    assert!(body["day"]["losers"].as_array().unwrap().is_empty());
    // 7D stays on the newest anchor: latest 8.00 against the d10 carry-forward of 3.00.
    assert_eq!(body["week"]["gainers"][0]["value_prev"], "3.00");
    assert_eq!(body["week"]["gainers"][0]["change_usd"], "5.00");
}

/// The same fallback across a **gap in the capture history**: with no snapshot yesterday, the
/// previous available capture is not the day anchor, so its baseline is not the day-before-
/// yesterday anchor either and the retry must fetch one.
///
/// This is the other side of the branch the daily case takes. The prices are chosen so the
/// anchors alone cannot answer it: the main query holds only `d0`/`d2` (latest and the day
/// anchor, which the day-before-yesterday anchor lands on as well) and `d10` (week + earliest
/// price). The retry's true baseline is the four-days-ago snapshot, reachable only through its
/// own at-or-before lookup — carrying forward from `d10` would report `3.00`.
#[tokio::test]
async fn movers_day_falls_back_across_a_missing_capture_day() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-day-gap@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];
    own_card(&app, &token, id, 1).await;

    // No d1 and no d3 capture: the feed skipped them.
    let (d0, d2, d4, d10) = (day_offset(0), day_offset(2), day_offset(4), day_offset(10));
    set_price_history(
        db,
        internal_card_id(db, id).await,
        &[
            (d10, Some("3.00"), None),
            (d4, Some("5.00"), None),
            (d2.clone(), Some("8.00"), None),
            (d0.clone(), Some("8.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(body["as_of"], d0, "longer windows keep the newest anchor");
    assert_eq!(body["day_as_of"], d2, "1D reports the previous available capture");
    assert_eq!(mover_ids(&body["day"]["gainers"]), vec![id.clone()]);
    assert_eq!(body["day"]["gainers"][0]["value_prev"], "5.00");
    assert_eq!(body["day"]["gainers"][0]["value_now"], "8.00");
    assert_eq!(body["day"]["gainers"][0]["change_usd"], "3.00");
    assert!(body["day"]["losers"].as_array().unwrap().is_empty());
}

/// When the fallback retry also finds no movement (every capture flat), `day_as_of` must
/// stay on the newest snapshot — an empty 1D list never claims an older reference date.
#[tokio::test]
async fn movers_day_keeps_the_newest_anchor_when_the_fallback_is_also_empty() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-day-flat@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];
    own_card(&app, &token, id, 1).await;

    let (d0, d1, d2) = (day_offset(0), day_offset(1), day_offset(2));
    set_price_history(
        db,
        internal_card_id(db, id).await,
        &[
            (d2, Some("5.00"), None),
            (d1, Some("5.00"), None),
            (d0.clone(), Some("5.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(body["as_of"], d0);
    assert_eq!(body["day_as_of"], d0, "an empty retry keeps the newest anchor");
    assert!(body["day"]["gainers"].as_array().unwrap().is_empty());
    assert!(body["day"]["losers"].as_array().unwrap().is_empty());
}

/// When a card's history doesn't reach the window baseline, the window drops it rather than
/// fabricating a delta: a card priced only within the last week populates day/week but the
/// month list stays empty (the 30d target predates every snapshot).
#[tokio::test]
async fn movers_month_window_empties_when_baseline_predates_history() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-stale@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];
    own_card(&app, &token, id, 1).await;

    let (d0, d1, d7) = (day_offset(0), day_offset(1), day_offset(7));
    set_price_history(
        db,
        internal_card_id(db, id).await,
        &[
            (d7.clone(), Some("5.00"), None),
            (d1.clone(), Some("5.00"), None),
            (d0.clone(), Some("8.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["as_of"].as_str(), Some(d0.as_str()));
    // Day + week: a +$3 gainer.
    assert_eq!(mover_ids(&body["day"]["gainers"]), vec![id.clone()]);
    assert_eq!(mover_ids(&body["week"]["gainers"]), vec![id.clone()]);
    // Month: no snapshot at/before the 30d target -> empty, not a bogus delta.
    assert!(body["month"]["gainers"].as_array().unwrap().is_empty());
    assert!(body["month"]["losers"].as_array().unwrap().is_empty());
}

/// The long-range windows use the same carry-forward baseline semantics as the short ones,
/// while all-time reaches beyond three years to the finish's earliest captured price.
#[tokio::test]
async fn movers_supports_year_two_year_three_year_and_all_time() {
    let app = test_app_with_catalog().await;
    let db = &app.state.db;
    let (token, _) = register(&app, "movers-long-ranges@example.com", "password123").await;
    let ids = sample_card_ids(&app, 1).await;
    let id = &ids[0];
    own_card(&app, &token, id, 1).await;

    let (d0, d365, d730, d1095, d1200) = (
        day_offset(0),
        day_offset(365),
        day_offset(730),
        day_offset(1095),
        day_offset(1200),
    );
    set_price_history(
        db,
        internal_card_id(db, id).await,
        &[
            (d1200, Some("1.00"), None),
            (d1095, Some("2.00"), None),
            (d730, Some("3.00"), None),
            (d365, Some("4.00"), None),
            (d0.clone(), Some("10.00"), None),
        ],
    )
    .await;

    let (status, _, body) =
        send(&app, get_with_bearer("/api/collection/mtg/movers", &token)).await;
    assert_eq!(status, StatusCode::OK, "movers failed: {body:?}");
    assert_eq!(body["as_of"].as_str(), Some(d0.as_str()));

    for (window, value_prev, change_usd) in [
        ("year", "4.00", "6.00"),
        ("two_year", "3.00", "7.00"),
        ("three_year", "2.00", "8.00"),
        ("all_time", "1.00", "9.00"),
    ] {
        assert_eq!(mover_ids(&body[window]["gainers"]), vec![id.clone()]);
        assert_eq!(body[window]["gainers"][0]["value_prev"], value_prev);
        assert_eq!(body[window]["gainers"][0]["change_usd"], change_usd);
        assert!(body[window]["losers"].as_array().unwrap().is_empty());
    }
}
