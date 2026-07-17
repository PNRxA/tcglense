//! Infrastructure probe contract: liveness is process-only, while readiness checks
//! the database and never exposes dependency details or cacheable responses.

use super::harness::*;

#[tokio::test]
async fn readiness_round_trips_the_database_and_is_no_store() {
    let app = test_app().await;
    let (status, headers, body) = send(&app, get("/api/ready")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ready" }));
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn database_failure_only_fails_readiness_with_a_generic_response() {
    let app = test_app().await;
    app.state
        .db
        .clone()
        .close()
        .await
        .expect("close the test database");

    let (status, headers, body) = send(&app, get("/api/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, json!({ "status": "unavailable" }));
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Liveness deliberately does not touch the database: a healthy process remains
    // live while the orchestrator drains it and waits for the dependency to recover.
    let (status, headers, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn maintenance_mode_keeps_liveness_up_and_rejects_everything_else() {
    let mut state = test_state().await;
    state.config = std::sync::Arc::new(crate::config::Config {
        maintenance_mode: true,
        // A configured combined-image fallback must be blocked too.
        web_root: Some(std::env::temp_dir()),
        ..state.config.as_ref().clone()
    });
    let app = crate::build_router(state);

    let (status, headers, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (status, headers, body) = send(&app, get("/api/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, json!({ "status": "maintenance" }));
    assert_eq!(cache_control(&headers), Some("no-store"));

    let (status, headers, body) = send(&app, get("/api/config")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["maintenance_mode"], true);
    assert_eq!(cache_control(&headers), Some("no-store"));

    for uri in ["/api/games", "/api/not-a-real-route", "/collection/mtg"] {
        let (status, headers, body) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert_eq!(
            body,
            json!({
                "error": "service is under maintenance",
                "code": "maintenance",
            }),
            "{uri}"
        );
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }
}

#[tokio::test]
async fn startup_gate_keeps_liveness_up_and_drains_until_migrations_complete() {
    use std::sync::atomic::Ordering;

    // `main.rs` binds the listener before running the boot migrations and closes this
    // gate for that window, so `/api/health` answers the platform health check while a
    // long migration runs. Reproduce that pre-migration window over the real router.
    let state = test_state().await;
    state.migrations_complete.store(false, Ordering::SeqCst);
    let app = crate::build_router(state.clone());

    // Liveness stays up so the orchestrator neither restarts the process nor fails the
    // deploy while migrations are still applying.
    let (status, headers, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
    assert_eq!(cache_control(&headers), Some("no-store"));

    // The cached SPA's boot-time config probe stays reachable so it can show a screen.
    let (status, _, _) = send(&app, get("/api/config")).await;
    assert_eq!(status, StatusCode::OK);

    // Readiness drains explicitly so a load balancer waits for the schema.
    let (status, headers, body) = send(&app, get("/api/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, json!({ "status": "starting" }));
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Every other request — real routes and unknown paths alike — is a non-cacheable
    // 503 so no handler runs a query against a half-migrated schema.
    for uri in ["/api/games", "/api/not-a-real-route"] {
        let (status, headers, body) = send(&app, get(uri)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert_eq!(
            body,
            json!({ "error": "service is starting", "code": "starting" }),
            "{uri}"
        );
        assert_eq!(cache_control(&headers), Some("no-store"), "{uri}");
    }

    // Once migrations complete the gate opens and normal routing resumes (shared Arc,
    // so the flip is visible to the router built above).
    state.migrations_complete.store(true, Ordering::SeqCst);
    let (status, _, _) = send(&app, get("/api/games")).await;
    assert_eq!(status, StatusCode::OK);
}
