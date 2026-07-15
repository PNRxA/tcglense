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
