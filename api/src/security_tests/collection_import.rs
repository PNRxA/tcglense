//! Collection import / sync from an external provider: authentication gating, the
//! saved-link lifecycle (GET/PUT/DELETE), provider + source validation, and the
//! `no-store` cache policy. The parts that would reach out to a provider (Archidekt or
//! Moxfield) over the network are deliberately not exercised here — every assertion
//! below resolves before any upstream fetch (bad provider / unparseable source /
//! missing saved link), so the suite stays fully offline like the rest of
//! `security_tests`. (The CSV upload needs no network, so its tests drive the full
//! path, including both providers' export shapes.)

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

    // (Moxfield's saved-link lifecycle isn't exercised here: its live import is temporarily
    // disabled, so saving a Moxfield link is refused — see
    // `moxfield_link_import_and_save_are_temporarily_disabled`. Its URL/id parsing is
    // covered by `collection_import::moxfield`'s unit tests.)

    // Forget it -> 204, then it reads back as null again (delete is idempotent).
    let (status, _, _) = send(&app, delete_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, delete_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null());
}

/// The saved collection link is per-user: one user can neither read, overwrite, nor
/// delete another's. The link stores an external collection id and drives
/// server-side fetches on that user's behalf, so a broadened `user_id` filter would
/// leak the linked identity or let one user hijack/wipe another's sync source.
#[tokio::test]
async fn saved_source_is_isolated_per_user() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-src@example.com", "password123").await;
    let (bob, _) = register(&app, "bob-src@example.com", "password123").await;

    // Alice saves a link.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &alice,
            json!({ "provider": "archidekt", "source": "111" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Bob has no saved link of his own — Alice's is invisible to him.
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &bob)).await;
    assert!(body.is_null(), "another user's saved link must not leak: {body:?}");

    // Bob saving his own link creates a separate row; it must not overwrite Alice's.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &bob,
            json!({ "provider": "archidekt", "source": "222" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["external_id"], "222");
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &alice)).await;
    assert_eq!(body["external_id"], "111", "bob's save must not overwrite alice's link");

    // Bob deleting his link leaves Alice's intact (delete is scoped to his own row).
    let (status, _, _) = send(&app, delete_with_bearer("/api/collection/mtg/source", &bob)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &alice)).await;
    assert_eq!(body["external_id"], "111", "bob's delete must not remove alice's link");
    let (_, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &bob)).await;
    assert!(body.is_null(), "bob's own link is gone");
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
            json!({ "provider": "deckbox", "source": "1042487" }),
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
            json!({ "provider": "deckbox", "source": "1042487", "mode": "replace" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // (Moxfield source parsing isn't retested here — its live import is disabled, so a
    // Moxfield import is refused before the source is even parsed; see
    // `moxfield_link_import_and_save_are_temporarily_disabled`.)

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
async fn moxfield_link_import_and_save_are_temporarily_disabled() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "mox-off@example.com", "password123").await;

    // A one-off Moxfield link import is refused up front (422) even with a perfectly valid
    // collection id: Moxfield's live API needs an approved User-Agent we don't have yet, so
    // its link import is temporarily turned off. The refusal is unconditional (it doesn't
    // reach the source parse or spawn a job), so this resolves offline like the rest of the
    // suite. (Moxfield CSV upload is unaffected — covered by the CSV tests below.)
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/import",
            &token,
            json!({ "provider": "moxfield", "source": "4xUdq-66IEKK6X53bhUS8Q", "mode": "merge" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "moxfield import disabled: {body:?}");
    assert!(
        body["error"].as_str().is_some_and(|e| e.contains("CSV")),
        "the error points the user at the CSV upload: {body:?}"
    );

    // Saving a Moxfield link is likewise refused — a saved link exists only to be re-synced,
    // and the re-sync is disabled.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "moxfield", "source": "https://moxfield.com/collection/4xUdq-66IEKK6X53bhUS8Q" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "moxfield save disabled: {body:?}");

    // Nothing was saved, so there's still no source on file.
    let (status, _, body) = send(&app, get_with_bearer("/api/collection/mtg/source", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null(), "a disabled provider's link is never saved: {body:?}");
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

#[tokio::test]
async fn csv_import_sniffs_a_moxfield_export_and_resolves_by_set_and_number() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "csv-mox@example.com", "password123").await;

    // A Moxfield-shaped export (no Scryfall ID column): rows resolve against the dummy
    // catalog by (Edition, Collector Number); the proxy row and the unknown set are
    // skipped, and the whole path — sniff, resolve, reconcile — runs offline.
    let csv = "Count,Tradelist Count,Name,Edition,Foil,Collector Number,Proxy\n\
               2,0,Card One,dmb,foil,1,False\n\
               3,0,Card Two,DMB,,2,False\n\
               1,0,Fake Proxy,dmb,,3,True\n\
               1,0,Ghost,zzz,,999,False\n";
    let (status, headers, body) = send(
        &app,
        csv_upload("/api/collection/mtg/import/csv?mode=overwrite", &token, csv),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["provider"], "moxfield", "the shape was sniffed as Moxfield");
    assert_eq!(body["matched_cards"], 2, "the uppercase Edition still matched");
    assert_eq!(body["unmatched_cards"], 1);
    assert_eq!(body["foil_copies"], 2);
    assert_eq!(body["regular_copies"], 3);

    let (status, _, list) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total"], 2, "the proxy and the unknown card were not imported");
}
