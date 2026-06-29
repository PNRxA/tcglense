//! Thin HTTP helpers over a shared [`reqwest::Client`] for the Scryfall API.
//!
//! Per Scryfall's API guidelines every request carries a descriptive
//! `User-Agent` (set on the shared client at build time) and an explicit
//! `Accept` header. The `gzip` feature on the client transparently requests and
//! decompresses gzip-encoded responses, including the large bulk download.

use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use reqwest::{Client, header};

use super::ingest::IngestError;
use super::model::{BulkData, BulkDataList, ScryfallSet, SetList};
use super::{BULK_DATA_URL, SETS_URL};

const ACCEPT_JSON: &str = "application/json";

/// Fetch the bulk-data catalog (small JSON describing each downloadable file).
pub async fn bulk_data(client: &Client) -> Result<Vec<BulkData>, IngestError> {
    let list: BulkDataList = client
        .get(BULK_DATA_URL)
        .header(header::ACCEPT, ACCEPT_JSON)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(list.data)
}

/// Fetch every set, following pagination (`has_more` / `next_page`).
pub async fn all_sets(client: &Client) -> Result<Vec<ScryfallSet>, IngestError> {
    let mut sets = Vec::new();
    let mut url = SETS_URL.to_string();
    loop {
        let page: SetList = client
            .get(&url)
            .header(header::ACCEPT, ACCEPT_JSON)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        sets.extend(page.data);
        match (page.has_more, page.next_page) {
            (true, Some(next)) => url = next,
            _ => break,
        }
    }
    Ok(sets)
}

/// Open the bulk download as a byte stream. The client decompresses gzip for us,
/// so the yielded bytes are the raw JSON array. The error type is normalised to
/// [`std::io::Error`] so the stream can drive a [`tokio_util::io::StreamReader`].
pub async fn download_stream(
    client: &Client,
    url: &str,
) -> Result<impl Stream<Item = Result<Bytes, std::io::Error>>, IngestError> {
    let response = client
        .get(url)
        .header(header::ACCEPT, ACCEPT_JSON)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes_stream().map_err(std::io::Error::other))
}
