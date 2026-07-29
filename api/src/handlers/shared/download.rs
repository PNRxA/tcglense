//! File-download responses shared by every export endpoint.
//!
//! Three surfaces now hand the browser a file rather than JSON — the collection CSV
//! ([`crate::handlers::collection::export`]), the deck list
//! ([`crate::handlers::decks::export`]), and the card-search text export
//! ([`crate::handlers::catalog::export`]) — and all three want the same two headers:
//! an explicit content type (the `String` body would otherwise default to `text/plain`)
//! and a `Content-Disposition` attachment filename so the browser saves instead of
//! rendering. Extracted here so a fourth export doesn't grow a fourth copy.
//!
//! `Cache-Control` is *not* set here: the router group the route lives in decides it
//! (`no-store` for the per-user private group, the public catalog value for the
//! CDN-cacheable catalog group).

use axum::body::Body;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

use crate::error::AppError;

/// Wrap `body` in a file-download response with the given content type.
///
/// The filename is interpolated into a quoted `Content-Disposition`, so a caller must
/// only ever build it from values it controls (a registry-checked game id, a set code
/// loaded from the catalog, a numeric id) — never from raw user input. A filename that
/// can't be a header value (a stray control character, say) is an internal error rather
/// than a silently mangled header.
fn download(body: String, filename: &str, content_type: HeaderValue) -> Result<Response, AppError> {
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|_| AppError::Internal("invalid export filename".into()))?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response())
}

/// Wrap a CSV body in a file-download response (`text/csv` + an attachment filename).
pub(crate) fn csv_download(body: String, filename: &str) -> Result<Response, AppError> {
    download(
        body,
        filename,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    )
}

/// Wrap a plain-text body in a file-download response (`text/plain` + an attachment
/// filename).
pub(crate) fn text_download(body: String, filename: &str) -> Result<Response, AppError> {
    download(
        body,
        filename,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    )
}

/// The same, for a body produced incrementally rather than built up front.
///
/// Used by the card-search export, whose result set is unbounded and so is never
/// materialised. A streaming body reports no size hint, which is also what keeps
/// [`super::super::cache::conditional_request_layer`] from buffering it back up to
/// compute an `ETag` — that layer's unknown-size guard is load-bearing here, not merely
/// defensive. The trade-off is no `Content-Length` (so no download progress bar) and no
/// validator, which is the right call for a response we refuse to hold in memory.
pub(crate) fn text_download_stream(body: Body, filename: &str) -> Result<Response, AppError> {
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|_| AppError::Internal("invalid export filename".into()))?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn csv_download_sets_both_headers() {
        let response = csv_download("a,b\n".to_string(), "tcglense-mtg-cards.csv")
            .expect("a plain filename is a valid header value");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/csv; charset=utf-8"),
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .and_then(|value| value.to_str().ok()),
            Some("attachment; filename=\"tcglense-mtg-cards.csv\""),
        );
    }

    #[test]
    fn text_download_is_plain_text() {
        let response = text_download("1 Sol Ring (LTC) 284\n".to_string(), "cards.txt")
            .expect("a plain filename is a valid header value");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; charset=utf-8"),
        );
    }

    #[test]
    fn a_filename_that_cannot_be_a_header_is_an_internal_error() {
        // Control characters can't ride in a header value; surfacing that as a 500
        // beats emitting a mangled/split `Content-Disposition`.
        let error = text_download("body".to_string(), "bad\nname.txt")
            .expect_err("a newline can't be a header value");
        assert!(matches!(error, AppError::Internal(_)));
    }
}
