//! HTTP for the MTGJSON sealed-contents ingest: fetch + gzip-decode + parse
//! `AllPrintings.json.gz`, honouring an HTTP `ETag` so an unchanged file is a cheap
//! `304`.
//!
//! `AllPrintings.json` is a single ~600 MB JSON document (~160 MB gzipped). We pull the
//! gzipped variant (the shared client has no `Content-Encoding: gzip` auto-decode for a
//! pre-compressed *file*, so we decode it ourselves with `flate2`) and **stream** it
//! through the decode + parse on a blocking task — the body is never buffered whole, and
//! `serde_json::from_reader` retains only the trimmed [`AllPrintings`] structs, not the
//! 600 MB tree. Both halves matter on a 1 GB instance (App Platform `basic-xs`): buffering
//! the gzipped body on top of the parsed structs was enough to breach the container's
//! memory ceiling and take the whole combined app down mid-sync. `Meta.json` bumps daily
//! from price rebuilds, so it's useless as a gate; the file's `ETag` tracks actual content
//! changes and MTGJSON honours conditional GET.

use futures_util::TryStreamExt;
use reqwest::{
    Client, StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use tokio_util::io::{StreamReader, SyncIoBridge};

use super::MtgjsonError;
use super::model::AllPrintings;

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
    base_url: &str,
    etag: Option<&str>,
) -> Result<FetchOutcome, MtgjsonError> {
    let url = format!("{base_url}/AllPrintings.json.gz");
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

    // Stream the gzipped body (~160 MB) straight through decode + parse on a blocking
    // thread, so the CPU-bound work never stalls the async runtime AND the compressed
    // body is never resident in memory — `SyncIoBridge` hands the async byte stream to
    // the blocking task as a plain `Read` (the same shape `scryfall::client::json_lines`
    // streams its bulk files in).
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    // Built here (not inside `spawn_blocking`) so the bridge captures this runtime's
    // handle unconditionally; its blocking reads happen on the blocking thread below.
    let bridge = SyncIoBridge::new(StreamReader::new(stream));
    let all = tokio::task::spawn_blocking(move || -> Result<AllPrintings, MtgjsonError> {
        let decoder = flate2::read::GzDecoder::new(bridge);
        let reader = std::io::BufReader::with_capacity(1 << 20, decoder);
        serde_json::from_reader(reader).map_err(|err| {
            // A transfer failing mid-stream surfaces here, inside the parse; keep it
            // classified as a stream error so the recorded failure doesn't read as a
            // corrupt file when the network blipped. Either way the sync fails like any
            // other error: the state row is marked and the next tick re-fetches.
            if err.classify() == serde_json::error::Category::Io {
                MtgjsonError::Io(std::io::Error::other(err.to_string()))
            } else {
                MtgjsonError::Parse(err.to_string())
            }
        })
    })
    .await
    .map_err(|err| MtgjsonError::Join(err.to_string()))??;

    Ok(FetchOutcome::Fetched {
        etag: new_etag,
        all: Box::new(all),
    })
}
