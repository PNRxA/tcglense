//! HTTP for the MTGJSON sealed-contents ingest: fetch + gzip-decode + parse
//! `AllPrintings.json.gz`, honouring an HTTP `ETag` so an unchanged file is a cheap
//! `304`.
//!
//! `AllPrintings.json` is a single ~600 MB JSON document (~160 MB gzipped). We pull the
//! gzipped variant (the shared client has no `Content-Encoding: gzip` auto-decode for a
//! pre-compressed *file*, so we decode it ourselves with `flate2`) and parse it in a
//! blocking task — `serde_json::from_reader` streams the decode + parse, so only the
//! trimmed [`AllPrintings`] structs are retained, not the whole 600 MB tree. `Meta.json`
//! bumps daily from price rebuilds, so it's useless as a gate; the file's `ETag` tracks
//! actual content changes and MTGJSON honours conditional GET.

use reqwest::{
    Client, StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};

use super::model::AllPrintings;
use super::{BASE_URL, MtgjsonError};

/// The result of a conditional fetch: either the server said "unchanged" (`304`) or we
/// downloaded + parsed a fresh copy (with its new `ETag`, when present).
pub enum FetchOutcome {
    /// The `ETag` matched — nothing to re-ingest.
    Unchanged,
    /// A fresh `AllPrintings`, plus the `ETag` to store for next time.
    Fetched {
        etag: Option<String>,
        all: Box<AllPrintings>,
    },
}

/// Conditionally fetch + parse `AllPrintings.json.gz`. When `etag` is `Some`, sends
/// `If-None-Match`; a `304` returns [`FetchOutcome::Unchanged`] without downloading the
/// body. Otherwise streams the gzip body, decodes + parses it off the async runtime (a
/// blocking task), and returns the trimmed structs plus the response `ETag`.
pub async fn fetch_all_printings(
    client: &Client,
    etag: Option<&str>,
) -> Result<FetchOutcome, MtgjsonError> {
    let url = format!("{BASE_URL}/AllPrintings.json.gz");
    let mut request = client.get(&url);
    if let Some(tag) = etag {
        request = request.header(IF_NONE_MATCH, tag);
    }
    let response = request.send().await?;
    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::Unchanged);
    }
    let response = response.error_for_status()?;
    let new_etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // Buffer the gzipped body (~160 MB), then decode + parse on a blocking thread so the
    // CPU-bound work never stalls the async runtime. `from_reader` streams the decode, so
    // only the trimmed structs are retained (not the ~600 MB decompressed document).
    let bytes = response.bytes().await?;
    let all = tokio::task::spawn_blocking(move || -> Result<AllPrintings, MtgjsonError> {
        let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
        let reader = std::io::BufReader::with_capacity(1 << 20, decoder);
        serde_json::from_reader(reader).map_err(|err| MtgjsonError::Parse(err.to_string()))
    })
    .await
    .map_err(|err| MtgjsonError::Join(err.to_string()))??;

    Ok(FetchOutcome::Fetched {
        etag: new_etag,
        all: Box::new(all),
    })
}
