//! URL query/path parameter handling — a malformed value must come back as our JSON
//! `{ "error": ... }` shape, never axum's default `text/plain` parser text (which would
//! echo `serde_urlencoded` / path-deserialization internals to the client).

use super::harness::*;

#[tokio::test]
async fn malformed_query_string_is_a_json_error_not_raw_parser_text() {
    let app = test_app().await;

    // `page` is numeric; `page=abc` fails to deserialize. axum's default rejection would
    // return `text/plain` "Failed to deserialize query string: invalid digit found in
    // string"; our `Query` wrapper returns a fixed JSON error instead.
    let (status, headers, body) = send(&app, get("/api/games/mtg/cards?page=abc")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        content_type(&headers).is_some_and(|ct| ct.starts_with("application/json")),
        "content-type was {:?}",
        content_type(&headers)
    );
    let message = body["error"].as_str().expect("json error body");
    assert_eq!(message, "invalid query parameters");
    // Crucially, none of the parser internals leak through.
    assert!(
        !message.contains("deserialize") && !message.contains("digit"),
        "leaked parser text: {message}"
    );
}

#[tokio::test]
async fn malformed_path_param_is_a_json_error_not_raw_parser_text() {
    let app = test_app().await;
    // `get_import_job` extracts `AuthUser` before the path, so a valid token is needed to
    // reach (and exercise) the path deserialization.
    let (token, _) = register(&app, "params@example.com", "password123").await;

    // `job_id` is a `u64`; a non-numeric segment fails path deserialization. axum's
    // default rejection would echo "Cannot parse `job_id` with value `abc` to a `u64`"
    // (the parameter name + target type) as `text/plain`; our `Path` wrapper masks it.
    let (status, headers, body) = send(
        &app,
        get_with_bearer("/api/collection/mtg/import/jobs/abc", &token),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        content_type(&headers).is_some_and(|ct| ct.starts_with("application/json")),
        "content-type was {:?}",
        content_type(&headers)
    );
    let message = body["error"].as_str().expect("json error body");
    assert_eq!(message, "invalid path parameter");
    assert!(
        !message.contains("job_id") && !message.contains("u64") && !message.contains("parse"),
        "leaked parser text: {message}"
    );
}
