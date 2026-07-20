//! Per-user price alerts (issue #525): authentication gating, **session-only** access (an
//! API key — even read_write — is rejected, because the channel settings hold delivery
//! credentials), per-user ownership isolation (another user's alert id is a 404, never a
//! 403), the create/list round trip, input validation, and the SSRF gate on the Discord
//! webhook URL.
//!
//! Drives the real router over the seeded dummy catalog, so alerts can be created against
//! real card external ids.

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// Grab one real card external id from the seeded catalog.
async fn sample_card_id(app: &Router) -> String {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "listing seeded cards failed: {body:?}"
    );
    body["data"][0]["id"]
        .as_str()
        .expect("a seeded card id")
        .to_string()
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
async fn alerts_require_authentication() {
    let app = test_app_with_catalog().await;
    // No bearer -> 401, and per-user data must never be shared-cached.
    let (status, headers, _) = send(&app, get("/api/alerts")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn alerts_are_session_only_and_reject_api_keys() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-session@example.com", PW).await;
    // Even a read_write key can't reach the alerts surface — it's SessionUser-gated so a
    // leaked key can neither read a user's notification settings nor redirect them.
    let key = create_key(&app, &access, "read_write").await;
    let (status, _, _) = send(&app, get_with_bearer("/api/alerts", &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = send(&app, get_with_bearer("/api/alerts/channels", &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_and_list_round_trips_an_alert() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-crud@example.com", PW).await;
    let card = sample_card_id(&app).await;

    let (status, headers, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts",
            &access,
            json!({
                "game": "mtg",
                "target_kind": "card",
                "external_id": card,
                "finish": "nonfoil",
                "direction": "below",
                "threshold": "5",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create alert failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["target"]["external_id"], card);
    assert_eq!(body["direction"], "below");
    // The threshold is normalised to a 2-dp decimal string.
    assert_eq!(body["threshold"], "5.00");
    assert_eq!(body["is_active"], true);
    let alert_id = body["id"].as_i64().expect("alert id");

    // The list shows it.
    let (status, _, list) = send(&app, get_with_bearer("/api/alerts", &access)).await;
    assert_eq!(status, StatusCode::OK);
    let data = list["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], alert_id);

    // Pause it, then delete it.
    let (status, _, updated) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/alerts/{alert_id}"),
            &access,
            json!({ "is_active": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update failed: {updated:?}");
    assert_eq!(updated["is_active"], false);

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/alerts/{alert_id}"),
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn another_users_alert_is_404_not_403() {
    let app = test_app_with_catalog().await;
    let (owner, _) = register(&app, "alerts-owner@example.com", PW).await;
    let (other, _) = register(&app, "alerts-other@example.com", PW).await;
    let card = sample_card_id(&app).await;

    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts",
            &owner,
            json!({
                "game": "mtg",
                "target_kind": "card",
                "external_id": card,
                "finish": "nonfoil",
                "direction": "above",
                "threshold": "10",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let alert_id = body["id"].as_i64().expect("alert id");

    // The other user can neither edit nor delete it — a 404 (no existence oracle).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/alerts/{alert_id}"),
            &other,
            json!({ "threshold": "20" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/alerts/{alert_id}"),
            &other,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_validates_its_inputs() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-validate@example.com", PW).await;
    let card = sample_card_id(&app).await;

    let base = |overrides: Value| {
        let mut body = json!({
            "game": "mtg",
            "target_kind": "card",
            "external_id": card,
            "finish": "nonfoil",
            "direction": "below",
            "threshold": "5",
        });
        for (k, v) in overrides.as_object().unwrap() {
            body[k] = v.clone();
        }
        body
    };

    // Bad direction / finish / threshold / kind are all 422.
    for bad in [
        base(json!({ "direction": "sideways" })),
        base(json!({ "finish": "etched", "target_kind": "product" })),
        base(json!({ "threshold": "0" })),
        base(json!({ "threshold": "not-a-number" })),
        base(json!({ "target_kind": "planet" })),
    ] {
        let (status, _, body) =
            send(&app, json_with_bearer("POST", "/api/alerts", &access, bad)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "expected 422: {body:?}"
        );
    }

    // An unknown card external id is a 404.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts",
            &access,
            base(json!({ "external_id": "00000000-0000-0000-0000-000000000000" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn channels_reject_non_discord_webhook_urls() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-channels@example.com", PW).await;

    // An off-host webhook URL (SSRF) is refused.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/alerts/channels",
            &access,
            json!({ "discord_webhook_url": "https://evil.example.com/api/webhooks/1/a" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A real discord.com webhook is accepted and round-trips through the GET.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/alerts/channels",
            &access,
            json!({ "discord_webhook_url": "https://discord.com/api/webhooks/123/abc" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let (status, _, channels) = send(&app, get_with_bearer("/api/alerts/channels", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        channels["discord_webhook_url"],
        "https://discord.com/api/webhooks/123/abc"
    );

    // Telegram needs both halves together.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/alerts/channels",
            &access,
            json!({ "telegram_bot_token": "123:abc" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn channel_test_scopes_to_a_single_channel() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-test-scope@example.com", PW).await;

    // Save a Discord webhook only — no Telegram, and email is unavailable in tests.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/alerts/channels",
            &access,
            json!({ "discord_webhook_url": "https://discord.com/api/webhooks/123/abc" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");

    // Scoping the test to Telegram touches no channel: Discord is excluded by the scope and
    // Telegram isn't configured, so the result list is empty and nothing is sent over the
    // network (keeps the test hermetic while proving the scope filter is applied).
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts/channels/test?channel=telegram",
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        body["results"].as_array().expect("results array").len(),
        0,
        "telegram-scoped test must not run the configured Discord channel"
    );

    // Scoping to email (unavailable in tests) is likewise a no-op empty result.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts/channels/test?channel=email",
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["results"].as_array().expect("results array").len(), 0);

    // An unrecognised channel name is a 422 (before any send is attempted).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/alerts/channels/test?channel=carrier-pigeon",
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn release_opt_ins_default_off_and_round_trip() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "alerts-releases@example.com", PW).await;

    // With no settings row yet, both release opt-ins default off.
    let (status, _, channels) = send(&app, get_with_bearer("/api/alerts/channels", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(channels["sld_release_enabled"], false);
    assert_eq!(channels["set_release_enabled"], false);

    // Opt into Secret Lair drop heads-ups only; the set opt-in stays off.
    let (status, _, saved) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/alerts/channels",
            &access,
            json!({ "sld_release_enabled": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved:?}");
    assert_eq!(saved["sld_release_enabled"], true);
    assert_eq!(saved["set_release_enabled"], false);

    // The choice round-trips through the GET.
    let (status, _, channels) = send(&app, get_with_bearer("/api/alerts/channels", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(channels["sld_release_enabled"], true);
    assert_eq!(channels["set_release_enabled"], false);
}
