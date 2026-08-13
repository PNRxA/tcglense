//! Collection import from an external provider: authentication gating, provider + source
//! validation, and the `no-store` cache policy. The parts that would reach out to a
//! provider (Archidekt or Moxfield) over the network are deliberately not exercised here —
//! every assertion below resolves before any upstream fetch (bad provider / unparseable
//! source), so the suite stays fully offline like the rest of `security_tests`. (The CSV
//! upload needs no network, so its tests drive the full path, including both providers'
//! export shapes.)

use super::harness::*;

#[tokio::test]
async fn link_import_requires_authentication() {
    let app = test_app_with_catalog().await;

    // The import writes to the caller's own collection, so an unauthenticated request is
    // 401 and the response must never be shared-cached. (The upload/paste/job-status
    // routes get the same treatment in their own tests below.)
    let (status, headers, _) = send(
        &app,
        json_post(
            "/api/collection/mtg/import",
            json!({ "provider": "archidekt", "source": "1042487", "mode": "replace" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

/// The saved-link and re-sync routes were removed with smart sync: nothing is remembered
/// between imports, so `/source` and `/sync` must be gone from the router entirely — not
/// merely unused by the SPA — and answer the router's catch-all 404. Pinned so a partial
/// revert can't quietly restore a half-wired surface.
#[tokio::test]
async fn saved_source_and_sync_routes_are_gone() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "no-resync@example.com", "password123").await;

    let removed = [
        get_with_bearer("/api/collection/mtg/source", &token),
        json_with_bearer(
            "PUT",
            "/api/collection/mtg/source",
            &token,
            json!({ "provider": "archidekt", "source": "1042487" }),
        ),
        json_with_bearer("POST", "/api/collection/mtg/sync", &token, json!({})),
    ];
    for req in removed {
        let (status, _, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "route should not exist");
    }
}

#[tokio::test]
async fn import_rejects_a_bad_provider_and_an_unparseable_source() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "picky@example.com", "password123").await;

    // Unknown provider -> 422, resolved before any job is spawned.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/mtg/import",
            &token,
            json!({ "provider": "deckbox", "source": "1042487", "mode": "replace" }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown provider: {body:?}"
    );

    // Known provider but a source with no id in it -> 422 (resolves before any fetch).
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

    // (Moxfield source parsing isn't retested here — its live import is disabled, so a
    // Moxfield import is refused before the source is even parsed; see
    // `moxfield_link_import_is_temporarily_disabled`.)
}

#[tokio::test]
async fn moxfield_link_import_is_temporarily_disabled() {
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
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "moxfield import disabled: {body:?}"
    );
    assert!(
        body["error"].as_str().is_some_and(|e| e.contains("CSV")),
        "the error points the user at the CSV upload: {body:?}"
    );
}

#[tokio::test]
async fn link_import_unknown_game_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "wrong-game@example.com", "password123").await;

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/collection/pokemon/import",
            &token,
            json!({ "provider": "archidekt", "source": "1042487", "mode": "overwrite" }),
        ),
    )
    .await;
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
        .body(Body::from(
            "Scryfall ID,Finish,Quantity\ndummy-dmb-0001,Normal,1\n",
        ))
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
    let (status, _, _) = send(
        &app,
        csv_upload("/api/collection/mtg/import/csv", &token, csv),
    )
    .await;
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
    assert_eq!(
        body["provider"], "moxfield",
        "the shape was sniffed as Moxfield"
    );
    assert_eq!(
        body["matched_cards"], 2,
        "the uppercase Edition still matched"
    );
    assert_eq!(body["unmatched_cards"], 1);
    assert_eq!(body["foil_copies"], 2);
    assert_eq!(body["regular_copies"], 3);

    let (status, _, list) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["total"], 2,
        "the proxy and the unknown card were not imported"
    );
}

// ---- Pasted text (POST .../import/text) ----
//
// The paste endpoint is the same offline parse + reconcile as the upload, reached by
// content rather than by file (issue #572 — Mythic Tools is a phone app). These cover the
// boundaries that are its own: the route exists and is auth-gated like every other
// per-user route, an empty paste can't ride a Replace into a wipe, and the format sniffing
// really does accept both a card list and a pasted CSV.

/// A `POST .../import/text` with a raw text/plain body and a bearer token.
fn text_paste(uri: &str, token: &str, body: &'static str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn text_import_requires_authentication() {
    let app = test_app_with_catalog().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/collection/mtg/import/text?mode=overwrite")
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from("1 Card One (dmb) 1\n"))
        .unwrap();
    let (status, headers, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn text_import_rejects_a_bad_mode_an_empty_paste_or_unreadable_text() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "text-bad@example.com", "password123").await;

    // No mode query param at all.
    let (status, _, _) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text",
            &token,
            "1 Card One (dmb) 1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nothing pasted -> 422, never a silent no-op a Replace could ride into a wipe.
    let (status, _, _) = send(
        &app,
        text_paste("/api/collection/mtg/import/text?mode=replace", &token, ""),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Text with no card-shaped lines is refused with our JSON error, not read as an
    // empty collection.
    let (status, headers, body) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text?mode=replace",
            &token,
            "hello there\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(body["error"].is_string(), "error is JSON: {body:?}");
}

#[tokio::test]
async fn text_import_unknown_game_is_404() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "text-game@example.com", "password123").await;
    let (status, _, _) = send(
        &app,
        text_paste(
            "/api/collection/pokemon/import/text?mode=overwrite",
            &token,
            "1 Card One (dmb) 1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn text_import_reads_a_pasted_card_list_end_to_end() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "text-ok@example.com", "password123").await;

    // A Mythic Tools-shaped TXT export: a comment, a section header a collection has no
    // use for, a foil marker, the `3x` quantity spelling, and a line naming nothing.
    let text = "# Trade binder\n\
                Mainboard\n\
                2 Card One (dmb) 1 *F*\n\
                3x Card Two (DMB) 2\n\
                1 Ghost (zzz) 999\n";
    let (status, headers, body) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text?mode=overwrite",
            &token,
            text,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import failed: {body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(body["provider"], "mythictools");
    assert_eq!(body["matched_cards"], 2);
    assert_eq!(body["unmatched_cards"], 1);
    assert_eq!(body["foil_copies"], 2);
    assert_eq!(body["regular_copies"], 3);

    let (status, _, list) = send(&app, get_with_bearer("/api/collection/mtg", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total"], 2);
}

#[tokio::test]
async fn text_import_also_accepts_a_pasted_mythic_tools_csv() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "text-csv@example.com", "password123").await;

    // Pasting the CSV export instead of the TXT one must work — the user shouldn't have
    // to know which of their app's export formats they copied. "Amount" identifies the
    // shape even though a Scryfall ID column is present too.
    let csv = "Amount,Name,Set Code,Set Name,Collector Number,Condition,Finish,Language,\
               Extra Info,Assigned Price,Notes,Scryfall ID\n\
               2,Card One,dmb,Dummy,1,NM,foil,en,,,,dummy-dmb-0001\n\
               3,Card Two,dmb,Dummy,2,NM,Nonfoil,en,,,,\n";
    let (status, _, body) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text?mode=overwrite",
            &token,
            csv,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import failed: {body:?}");
    assert_eq!(body["provider"], "mythictools");
    assert_eq!(body["matched_cards"], 2);
    assert_eq!(
        body["regular_copies"], 3,
        "\"Nonfoil\" is a regular copy, not an unrecognised finish"
    );
    assert_eq!(body["foil_copies"], 2);
}

/// Mint an API key with the given scope for an account, via the session-only key route.
async fn create_api_key(app: &TestApp, access: &str, scope: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            serde_json::json!({ "name": format!("{scope} key"), "scope": scope }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {body:?}");
    body["key"].as_str().expect("plaintext key").to_string()
}

#[tokio::test]
async fn text_import_is_a_write_so_a_read_only_api_key_is_403() {
    // The paste route is a bulk write — in `replace` mode it can remove holdings — so it
    // must sit behind `WritableUser` like every other mutation, not behind plain auth.
    // 403 (valid credential, insufficient scope), never 401.
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "text-scope@example.com", "password123").await;
    let key = create_api_key(&app, &access, "read").await;
    let (status, headers, body) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text?mode=replace",
            &key,
            "2 Card One (dmb) 1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));

    // A writable key on the same account is accepted, so the 403 above is really the
    // scope gate and not the route being broken for keys altogether.
    let writable = create_api_key(&app, &access, "read_write").await;
    let (status, _, body) = send(
        &app,
        text_paste(
            "/api/collection/mtg/import/text?mode=overwrite",
            &writable,
            "2 Card One (dmb) 1\n",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["matched_cards"], 1);
}
