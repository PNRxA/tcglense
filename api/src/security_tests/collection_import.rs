//! Collection import / sync from an external provider: authentication gating, the
//! saved-link lifecycle (GET/PUT/DELETE), provider + source validation, and the
//! `no-store` cache policy. The parts that would reach out to Archidekt over the
//! network are deliberately not exercised here — every assertion below resolves before
//! any upstream fetch (bad provider / unparseable source / missing saved link), so the
//! suite stays fully offline like the rest of `security_tests`.

use super::harness::*;

fn delete_with_bearer(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn import_and_source_routes_require_authentication() {
    let app = test_app_with_catalog().await;

    // Every route in the group is per-user, so unauthenticated access is 401 and the
    // response must never be shared-cached.
    let unauthed = [
        get("/api/collection/mtg/source"),
        json_post(
            "/api/collection/mtg/import",
            json!({ "provider": "archidekt", "source": "1042487", "mode": "replace" }),
        ),
        json_post("/api/collection/mtg/sync", json!({})),
        Request::builder()
            .method("PUT")
            .uri("/api/collection/mtg/source")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "provider": "archidekt", "source": "1042487" }).to_string(),
            ))
            .unwrap(),
        Request::builder()
            .method("DELETE")
            .uri("/api/collection/mtg/source")
            .body(Body::empty())
            .unwrap(),
    ];

    for req in unauthed {
        let (status, headers, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(cache_control(&headers), Some("no-store"));
    }
}

#[tokio::test]
async fn saved_source_lifecycle() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "syncer@example.com", "password123").await;

    // Nothing saved yet -> null (and no-store).
    let (status, headers, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(body.is_null(), "no saved source should serialize as null: {body:?}");

    // Save a link from a full Archidekt URL -> the numeric id is extracted.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "archidekt", "source": "https://archidekt.com/collection/v2/1042487" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save failed: {body:?}");
    assert_eq!(body["provider"], "archidekt");
    assert_eq!(body["external_id"], "1042487");
    assert_eq!(body["url"], "https://archidekt.com/collection/v2/1042487");
    assert!(body["last_synced_at"].is_null(), "a freshly-saved link has never synced");

    // Read it back.
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["external_id"], "1042487");

    // Re-saving a different id upserts the single row (no duplicate).
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "archidekt", "source": "999" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["external_id"], "999");

    // Forget it -> 204, then it reads back as null again (delete is idempotent).
    let (status, _, _) = send(&app, delete_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, delete_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null());
}

#[tokio::test]
async fn save_and_import_reject_bad_provider_and_source() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "picky@example.com", "password123").await;

    // Unknown provider -> 422.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "moxfield", "source": "1042487" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown provider: {body:?}");

    // Known provider but a source with no id in it -> 422 (resolves before any fetch).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "archidekt", "source": "not-a-collection" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Same guards on the one-off import endpoint.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/import",
            &token,
            json!({ "provider": "moxfield", "source": "1042487", "mode": "replace" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/import",
            &token,
            json!({ "provider": "archidekt", "source": "garbage", "mode": "overwrite" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn sync_without_a_saved_source_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "eager@example.com", "password123").await;

    let (status, headers, _) = send(
        &app,
        json_with_bearer("POST", "/api/collection/mtg/sync", &token, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn unknown_game_is_404_for_source_routes() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "wrong-game@example.com", "password123").await;

    let (status, _, _) = send(&app, get_with_bearer("/api/collection/pokemon/source", &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
