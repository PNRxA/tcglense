//! Request-body handling — correct status + JSON error shape.

use super::harness::*;

#[tokio::test]
async fn malformed_bodies_map_to_correct_status_with_json_errors() {
    let app = test_app().await;

    // Syntactically invalid JSON -> 400.
    let (bad_json, _, bad_json_body) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{ not valid json"))
            .unwrap(),
    )
    .await;
    assert_eq!(bad_json, StatusCode::BAD_REQUEST);
    assert!(bad_json_body["error"].as_str().is_some());

    // Missing Content-Type -> 415.
    let (no_ct, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(no_ct, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Wrong Content-Type -> 415.
    let (wrong_ct, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(CONTENT_TYPE, "text/plain")
            .body(Body::from("hello"))
            .unwrap(),
    )
    .await;
    assert_eq!(wrong_ct, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Valid JSON, wrong schema (missing password) -> 422.
    let (schema, _, schema_body) = send(
        &app,
        json_post("/api/auth/login", json!({ "email": "a@b.com" })),
    )
    .await;
    assert_eq!(schema, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(schema_body["error"].as_str().is_some());
}
