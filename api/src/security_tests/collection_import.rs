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
    assert_eq!(body["smart"], false, "smart sync defaults off when omitted");

    // Read it back.
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["external_id"], "1042487");
    assert_eq!(body["smart"], false);

    // Opting into smart sync persists on the saved link.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "archidekt", "source": "1042487", "smart": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["smart"], true, "smart preference is stored");

    // Re-saving a different id upserts the single row (no duplicate); omitting `smart`
    // resets it to the default (off).
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
    assert_eq!(body["smart"], false, "omitting smart on re-save clears it");

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

#[tokio::test]
async fn import_job_status_requires_auth_and_unknown_job_is_404() {
    let app = test_app_with_catalog().await;

    // No token -> 401 (and no-store).
    let (status, headers, _) = send(&app, get("/api/collection/mtg/import/jobs/1")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Authenticated but no such job -> 404 (job ids don't leak across users either).
    let (token, _) = register(&app, "poller@example.com", "password123").await;
    let (status, _, _) = send(
        &app,
        get_with_bearer("/api/collection/mtg/import/jobs/123456", &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- CSV upload (POST .../import/csv) ----
//
// The CSV import runs entirely offline (parse + reconcile, no upstream fetch), so unlike
// the URL import these tests can drive the full path — including a successful import — in
// process. The focus is the upload's security boundaries: auth gating, the `no-store`
// cache policy, and every validation failure returning our JSON error with the right
// status (never a partial import against untrusted input).

/// A `POST .../import/csv` with a raw text/csv body and a bearer token.
fn csv_upload(uri: &str, token: &str, body: &'static str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "text/csv")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn csv_import_requires_authentication() {
    let app = test_app_with_catalog().await;
    // No token -> 401, and a per-user route must never be shared-cached.
    let req = Request::builder()
        .method("POST")
        .uri("/api/collection/mtg/import/csv?mode=overwrite")
        .header(CONTENT_TYPE, "text/csv")
        .body(Body::from("Scryfall ID,Finish,Quantity\ndummy-dmb-0001,Normal,1\n"))
        .unwrap();
    let (status, headers, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn csv_import_rejects_a_missing_or_bad_mode() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "csv-mode@example.com", "password123").await;
    let csv = "Scryfall ID,Finish,Quantity\ndummy-dmb-0001,Normal,1\n";

    // No mode query param at all.
    let (status, _, _) = send(&app, csv_upload("/api/collection/mtg/import/csv", &token, csv)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // An unrecognised mode.
    let (status, _, _) = send(
        &app,
        csv_upload("/api/collection/mtg/import/csv?mode=wipe", &token, csv),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn csv_import_rejects_a_csv_missing_a_required_column_or_empty_body() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "csv-cols@example.com", "password123").await;

    // Missing the Finish column -> 422 (and no-store).
    let (status, headers, body) = send(
        &app,
        csv_upload(
            "/api/collection/mtg/import/csv?mode=overwrite",
            &token,
            "Scryfall ID,Quantity\ndummy-dmb-0001,1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(body["error"].is_string(), "error is JSON: {body:?}");

    // An empty upload -> 422 (never a silent no-op that a Replace could ride into a wipe).
    let (status, _, _) = send(
        &app,
        csv_upload("/api/collection/mtg/import/csv?mode=replace", &token, ""),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn csv_import_unknown_game_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "csv-game@example.com", "password123").await;
    let (status, _, _) = send(
        &app,
        csv_upload(
            "/api/collection/pokemon/import/csv?mode=overwrite",
            &token,
            "Scryfall ID,Finish,Quantity\ndummy-dmb-0001,Normal,1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn csv_import_reconciles_against_the_catalog_and_returns_a_summary() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "csv-ok@example.com", "password123").await;

    // Two real dummy-catalog ids (foil + regular) plus one that isn't in the catalog.
    let csv = "Quantity,Name,Finish,Scryfall ID\n\
               2,\"Card, One\",Foil,dummy-dmb-0001\n\
               3,Card Two,Normal,dummy-dmb-0002\n\
               1,Ghost,Normal,ffffffff-ffff-ffff-ffff-ffffffffffff\n";
    let (status, headers, body) = send(
        &app,
        csv_upload("/api/collection/mtg/import/csv?mode=overwrite", &token, csv),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["provider"], "archidekt");
    assert_eq!(body["matched_cards"], 2);
    assert_eq!(body["unmatched_cards"], 1);
    assert_eq!(body["regular_copies"], 3);
    assert_eq!(body["foil_copies"], 2);

    // The holdings really landed: the collection now lists two owned cards.
    let (status, _, list) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total"], 2);
}
