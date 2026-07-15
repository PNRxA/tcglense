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
