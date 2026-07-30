//! Thin HTTP helpers over a shared [`reqwest::Client`] for the Scryfall API.
//!
//! Per Scryfall's API guidelines every request carries a descriptive
//! `User-Agent` (set on the shared client at build time) and an explicit
//! `Accept` header. The `gzip` feature on the client transparently requests and
//! decompresses gzip-encoded responses, including the large bulk download.

use async_compression::tokio::bufread::GzipDecoder;
use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use reqwest::{Client, header};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader, Lines};
use tokio_util::io::StreamReader;

use super::ingest::IngestError;
use super::model::{BulkData, BulkDataList, ScryfallSet, SetList};
use crate::datasets::SyncSource;

const ACCEPT_JSON: &str = "application/json";

/// Read-buffer size for the bulk line readers — big enough that a multi-hundred-MB
/// stream isn't dominated by syscall-sized reads.
const READ_BUFFER: usize = 64 * 1024;

/// Fetch the bulk-data catalog (small JSON describing each downloadable file) from
/// `url` — the upstream catalog or its mirror, per the dataset source.
pub async fn bulk_data(client: &Client, url: &str) -> Result<Vec<BulkData>, IngestError> {
    let list: BulkDataList = client
        .get(url)
        .header(header::ACCEPT, ACCEPT_JSON)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(list.data)
}

/// Fetch every set, following pagination (`has_more` / `next_page`) from the first
/// page at `url`. The mirror folds all pages into one `has_more: false` response, so
/// this loop runs exactly once against it.
pub async fn all_sets(client: &Client, url: &str) -> Result<Vec<ScryfallSet>, IngestError> {
    let mut sets = Vec::new();
    let mut url = url.to_string();
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

/// Open the bulk download as a byte stream of whatever the origin serves — **possibly
/// gzip**, since the bulk files are pre-compressed *files* (`application/gzip`, no
/// `Content-Encoding`), which the client's transparent gzip therefore leaves alone.
/// Feed the stream to [`json_lines`] to get dataset lines out of it. The error type is
/// normalised to [`std::io::Error`] so the stream can drive a
/// [`tokio_util::io::StreamReader`].
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

/// The line reader [`json_lines`] hands back. Boxed because the gzip and plain paths are
/// different reader types decided at runtime; borrowed (`'a`) so a caller can wrap the
/// stream in a closure over borrowed state first — that's how the card import counts
/// wire bytes for its progress bar.
pub(super) type BulkLines<'a> = Lines<Box<dyn AsyncBufRead + Send + Unpin + 'a>>;

/// First byte of the gzip magic number (`1f 8b`).
const GZIP_MAGIC_FIRST: u8 = 0x1f;

/// Buffer the download and hand back its lines, inflating a gzipped body on the way.
///
/// The single seam every bulk dataset (cards, rulings, art tags) reads through, in either
/// dataset mode: upstream serves gzipped JSONL, and the mirror proxies those same bytes
/// through untouched, so both look identical here.
///
/// Compression is **sniffed from the body**, not from the URL suffix or `Content-Type`:
/// that keeps the older plain-JSON-array files working, tolerates a mirror or CDN that
/// relabels the body, and can't be fooled by a `.gz` name. One byte is enough to decide —
/// JSON's first non-space byte can never be a control character — which also means no
/// short-read retry loop, since [`AsyncBufReadExt::fill_buf`] only refills an empty
/// buffer and could otherwise spin on a 1-byte first chunk.
pub(super) async fn json_lines<'a, S>(stream: S) -> Result<BulkLines<'a>, IngestError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin + 'a,
{
    let mut reader = BufReader::with_capacity(READ_BUFFER, StreamReader::new(stream));
    // An empty body sniffs as "not gzip" and yields zero lines — every caller already
    // treats an empty dataset as a failure to retry, not as data.
    let gzipped = reader.fill_buf().await?.first() == Some(&GZIP_MAGIC_FIRST);

    let reader: Box<dyn AsyncBufRead + Send + Unpin + 'a> = if gzipped {
        let mut decoder = GzipDecoder::new(reader);
        // Concatenated members are still one logical file; without this the decode would
        // stop silently at the first member's end and truncate the dataset.
        decoder.multiple_members(true);
        Box::new(BufReader::with_capacity(READ_BUFFER, decoder))
    } else {
        Box::new(reader)
    };
    Ok(reader.lines())
}

/// Where to stream `kind`'s bulk file from: the mirror when that's the dataset source,
/// otherwise the location the catalog entry itself advertises.
pub(super) fn file_url(
    source: &SyncSource,
    kind: &str,
    entry: &BulkData,
) -> Result<String, IngestError> {
    if let Some(mirrored) = source.scryfall_file_url(kind) {
        return Ok(mirrored);
    }
    entry.file_url().map(str::to_string).ok_or_else(|| {
        IngestError::Other(format!(
            "scryfall bulk dataset '{kind}' advertises no download url"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// Wrap fixed bytes as the stream `json_lines` consumes, deliberately chopped into
    /// small chunks so lines (and gzip members) land across chunk boundaries the way they
    /// do on the wire.
    fn stream_of(bytes: Vec<u8>) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Unpin {
        let chunks: Vec<Result<Bytes, std::io::Error>> = bytes
            .chunks(8)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        futures_util::stream::iter(chunks)
    }

    fn gzip(body: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(body.as_bytes()).expect("write");
        encoder.finish().expect("finish")
    }

    async fn lines_of(bytes: Vec<u8>) -> Vec<String> {
        let mut lines = json_lines(stream_of(bytes)).await.expect("open");
        let mut out = Vec::new();
        while let Some(line) = lines.next_line().await.expect("read") {
            out.push(line);
        }
        out
    }

    /// Today's upstream shape: gzipped JSONL, one bare object per line.
    #[tokio::test]
    async fn inflates_gzipped_jsonl() {
        let body = "{\"slug\":\"a\"}\n{\"slug\":\"b\"}\n";
        assert_eq!(
            lines_of(gzip(body)).await,
            ["{\"slug\":\"a\"}", "{\"slug\":\"b\"}"]
        );
    }

    /// A gzip stream can arrive as concatenated members; all of them belong to the file.
    #[tokio::test]
    async fn inflates_every_gzip_member() {
        let mut bytes = gzip("{\"slug\":\"a\"}\n");
        bytes.extend(gzip("{\"slug\":\"b\"}\n"));
        assert_eq!(
            lines_of(bytes).await,
            ["{\"slug\":\"a\"}", "{\"slug\":\"b\"}"]
        );
    }

    /// The pre-2026-07 plain JSON array still reads: bytes pass through untouched and the
    /// callers' bracket/comma trimming does the rest.
    #[tokio::test]
    async fn passes_plain_json_through() {
        let body = "[\n{\"slug\":\"a\"},\n{\"slug\":\"b\"}\n]\n";
        assert_eq!(
            lines_of(body.as_bytes().to_vec()).await,
            ["[", "{\"slug\":\"a\"},", "{\"slug\":\"b\"}", "]"]
        );
    }

    /// An empty body is no lines, not an error — the callers' own "0 rows" guards turn that
    /// into a retried import rather than a wiped table.
    #[tokio::test]
    async fn empty_body_yields_no_lines() {
        assert!(lines_of(Vec::new()).await.is_empty());
    }

    /// Live contract canary — **not run by CI** (network). Run it by hand after a provider
    /// bump or when an import starts failing with a decode error:
    /// `cargo test -- --ignored live_bulk_catalog`. It drives the real path end to end:
    /// parse the upstream catalog, then stream the first lines of the smallest dataset. A
    /// field rename like 2026-07's `download_uri` → `jsonl_download_uri` fails it here,
    /// where the message is obvious, instead of as "network error contacting the card-data
    /// source" on every self-host's import.
    #[tokio::test]
    #[ignore = "hits api.scryfall.com; run manually"]
    async fn live_bulk_catalog_parses_and_streams() {
        let client = Client::builder()
            .user_agent("TCGLense/contract-canary")
            .build()
            .expect("client");
        let entry = bulk_data(&client, crate::scryfall::BULK_DATA_URL)
            .await
            .expect("catalog parses")
            .into_iter()
            .find(|b| b.kind == crate::scryfall::DATASET_ART_TAGS)
            .expect("art_tags dataset present");
        let url = entry.file_url().expect("entry advertises a file");
        assert!(entry.transfer_size().is_some_and(|n| n > 0));

        let stream = download_stream(&client, url).await.expect("download opens");
        let mut lines = json_lines(stream).await.expect("body decodes");
        let first = lines
            .next_line()
            .await
            .expect("read")
            .expect("at least one line");
        let line = first.trim().trim_end_matches(',');
        let tag: crate::scryfall::model::ScryfallArtTag =
            serde_json::from_str(line).expect("first line is a tag object");
        assert!(tag.slug.is_some_and(|slug: String| !slug.is_empty()));
    }

    #[test]
    fn file_url_prefers_the_mirror_then_the_entry() {
        let entry = BulkData {
            kind: "art_tags".to_string(),
            updated_at: "2026-07-30T09:01:23.670+00:00".to_string(),
            download_uri: None,
            jsonl_download_uri: Some("https://data.scryfall.io/art-tags/x.jsonl.gz".to_string()),
            size: None,
            compressed_size: Some(12_359_045),
        };

        let mirror = SyncSource::new(false, "https://tcglense.com");
        assert_eq!(
            file_url(&mirror, "art_tags", &entry).expect("mirror url"),
            "https://tcglense.com/api/mirror/scryfall/file/art_tags"
        );

        let upstream = SyncSource::new(true, "https://tcglense.com");
        assert_eq!(
            file_url(&upstream, "art_tags", &entry).expect("upstream url"),
            "https://data.scryfall.io/art-tags/x.jsonl.gz"
        );

        // An entry advertising no file at all is an error, not a silent empty import.
        let urlless = BulkData {
            jsonl_download_uri: None,
            ..entry
        };
        assert!(file_url(&upstream, "art_tags", &urlless).is_err());
    }
}
