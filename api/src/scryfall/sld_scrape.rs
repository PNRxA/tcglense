//! Secret Lair Drop **gallery scrape** — the runtime "fetch from source" the mirror origin runs.
//!
//! Scryfall's curated Secret Lair drop titles aren't in the bulk card API; they live only on the
//! set's gallery page (`/sets/sld`), which groups the set's cards into named "drops" by collector
//! number. `scripts/gen-sld-drops.mjs` scrapes that page **offline** to regenerate the committed
//! fallback (`sld_drops.json`); this is the same scrape ported to Rust so the **mirror origin** can
//! refresh its live drop table daily from source without a human re-running the script and
//! redeploying (see [`super::sld_tasks`]). It emits JSON in the exact shape of `sld_drops.json`, so
//! it round-trips through [`super::drops::install_snapshot`] and the sealed-contents derivation
//! unchanged, and the mirror re-serves it verbatim to consumers.
//!
//! Scraping HTML is inherently brittle — a markup change yields **zero** drops, surfaced as
//! [`ScrapeError::NoDrops`] — so a broken scrape is never installed and never wipes the good table:
//! the origin keeps serving its last-good snapshot (falling back to the committed one). The parser
//! is split from the fetch so it is unit-tested against a fixture with no network.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use reqwest::header::USER_AGENT;
use serde::Serialize;

/// The Secret Lair gallery page we scrape.
const SOURCE_URL: &str = "https://scryfall.com/sets/sld";
/// Game / set the snapshot is written for (matches the committed `sld_drops.json`).
const GAME: &str = super::GAME;
const SET: &str = "sld";

/// A failure scraping the gallery. Non-fatal at the call site (logged; the origin keeps serving
/// whatever snapshot it already had loaded).
#[derive(Debug, thiserror::Error)]
pub enum ScrapeError {
    #[error("secret lair gallery request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("secret lair gallery yielded no drop headers — Scryfall markup may have changed")]
    NoDrops,
}

/// Fetch Scryfall's Secret Lair gallery and build the drop snapshot JSON (the shape of
/// `sld_drops.json`). Carries the configured Scryfall `User-Agent` (their API guidelines require a
/// descriptive one). Errors — network, non-2xx, or a markup change that yields no drops — are
/// returned, never panic; the caller keeps its last-good snapshot.
pub async fn fetch_snapshot_json(
    http: &reqwest::Client,
    user_agent: &str,
) -> Result<String, ScrapeError> {
    let html = http
        .get(SOURCE_URL)
        .header(USER_AGENT, user_agent)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    build_snapshot_json(&html)
}

/// One scraped drop: its slug, curated title, and the collector numbers whose cards it groups.
#[derive(Serialize)]
struct ScrapedDrop {
    slug: String,
    title: String,
    collector_numbers: Vec<String>,
}

/// Build the snapshot JSON from gallery HTML. Split from [`fetch_snapshot_json`] so the parse is
/// unit-testable against a fixture. [`ScrapeError::NoDrops`] when the page yields no drop headers
/// (a markup change), so the caller never installs — and thus never serves — an empty table.
pub fn build_snapshot_json(html: &str) -> Result<String, ScrapeError> {
    let drops = parse_drops(html);
    if drops.is_empty() {
        return Err(ScrapeError::NoDrops);
    }
    Ok(serialize_snapshot(&drops))
}

// Each drop is an `<h2 class="card-grid-header" id="slug">…title…</h2>` whose body is a grid of
// `/card/sld/<collector-number>/…` links, up to the next such header. These mirror the regexes in
// `scripts/gen-sld-drops.mjs` (kept in step so the runtime scrape and the offline one agree).
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<h2 class="card-grid-header" id="([^"]+)">(.*?)</h2>"#)
        .expect("valid header regex")
});
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)card-grid-header-content"\s*>(.*?)<span class="card-grid-header-dot"#)
        .expect("valid title regex")
});
static CARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"/card/sld/([^/"]+)/"#).expect("valid card regex"));
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid tag regex"));
static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid ws regex"));

/// Parse the gallery HTML into ordered drops (Scryfall's display order, newest first). A drop's
/// membership is the markup between its own header and the next — sliced from the header match
/// positions so it can't misalign onto a different set of `<h2>`s. A collector number is kept by
/// the first drop to list it (cross-drop collision guard), and a variant printing repeating a
/// number within one drop is deduped; a drop with no cards is dropped.
fn parse_drops(html: &str) -> Vec<ScrapedDrop> {
    let headers: Vec<Captures> = HEADER_RE.captures_iter(html).collect();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut drops = Vec::new();
    for (i, h) in headers.iter().enumerate() {
        let whole = h.get(0).expect("group 0 always present");
        let slug = h.get(1).expect("id capture").as_str().to_string();
        let header_inner = h.get(2).expect("inner capture").as_str();
        let title = TITLE_RE
            .captures(header_inner)
            .and_then(|c| c.get(1))
            .map(|m| clean_title(m.as_str()))
            .unwrap_or_else(|| clean_title(header_inner));

        let body_start = whole.end();
        let body_end = headers
            .get(i + 1)
            .map(|next| next.get(0).expect("group 0").start())
            .unwrap_or(html.len());
        let body = &html[body_start..body_end];

        let mut seen_here: HashSet<String> = HashSet::new();
        let mut collector_numbers = Vec::new();
        for m in CARD_RE.captures_iter(body) {
            let cn = decode_percent(m.get(1).expect("cn capture").as_str());
            if !seen_here.insert(cn.clone()) {
                continue; // a variant printing repeating a number within this drop
            }
            if seen.contains_key(&cn) {
                continue; // first drop to list a number keeps it
            }
            seen.insert(cn.clone(), ());
            collector_numbers.push(cn);
        }
        if !collector_numbers.is_empty() {
            drops.push(ScrapedDrop {
                slug,
                title,
                collector_numbers,
            });
        }
    }
    drops
}

/// Clean a header's inner HTML to its plain-text title: strip tags, decode entities, collapse
/// whitespace, trim. Mirrors the JS `cleanTitle`.
fn clean_title(html: &str) -> String {
    let no_tags = TAG_RE.replace_all(html, " ");
    let decoded = decode_entities(&no_tags);
    WS_RE.replace_all(&decoded, " ").trim().to_string()
}

/// Decode the HTML entities that appear in Scryfall drop titles: a small named set plus numeric
/// `&#…;` / `&#x…;` references. Mirrors the JS `decodeEntities`; an unknown named entity is left
/// verbatim.
fn decode_entities(s: &str) -> String {
    static ENTITY_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&(#x?[0-9a-fA-F]+|[a-zA-Z]+);").expect("valid entity regex"));
    ENTITY_RE
        .replace_all(s, |caps: &Captures| {
            let body = &caps[1];
            if let Some(rest) = body.strip_prefix('#') {
                let code = match rest.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => rest.parse::<u32>().ok(),
                };
                return code
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| caps[0].to_string());
            }
            named_entity(body)
                .map(str::to_string)
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

/// The named HTML entities the JS scraper decodes (the ones that actually occur in drop titles).
fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "mdash" => "\u{2014}",
        "ndash" => "\u{2013}",
        "hellip" => "\u{2026}",
        "rsquo" => "\u{2019}",
        "lsquo" => "\u{2018}",
        "ldquo" => "\u{201c}",
        "rdquo" => "\u{201d}",
        _ => return None,
    })
}

/// Percent-decode a URL path segment. Scryfall percent-encodes non-ASCII collector numbers (e.g.
/// the foil-star `★` as `%E2%98%85`), so this mirrors the JS scraper's `decodeURIComponent`. An
/// invalid `%`-sequence is passed through unchanged rather than dropped.
fn decode_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The snapshot wrapper serialized to JSON (the shape of `sld_drops.json`, whose extra `//`
/// comment keys the drop-store parser ignores). Deterministic field/collection order, so a
/// re-scrape of unchanged drops serves byte-identical JSON — nice for a warm CDN. The drop store's
/// content version hashes the drop *data*, not these bytes (see `drops::data_content_hash`), so the
/// version stays stable across representations (the compact scrape vs the pretty committed seed)
/// regardless — that's what prevents a spurious downstream re-derivation on reboot.
#[derive(Serialize)]
struct Snapshot<'a> {
    #[serde(rename = "//")]
    note: &'a str,
    sets: Vec<SnapshotSet<'a>>,
}

#[derive(Serialize)]
struct SnapshotSet<'a> {
    game: &'a str,
    set: &'a str,
    drops: &'a [ScrapedDrop],
}

fn serialize_snapshot(drops: &[ScrapedDrop]) -> String {
    let snapshot = Snapshot {
        note: "GENERATED at runtime by scryfall::sld_scrape from Scryfall's Secret Lair gallery.",
        sets: vec![SnapshotSet {
            game: GAME,
            set: SET,
            drops,
        }],
    };
    // Compact (not pretty): the bytes are only ever machine-read (parsed back by the drop store /
    // the mirror consumer), and compactness keeps the re-served mirror payload small. These plain
    // structs can't fail to serialize, and this runs off the request path (a background scrape).
    serde_json::to_string(&snapshot).expect("drop snapshot serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// A gallery fragment with the structure the scraper keys on: two drops, an HTML-entity title
    /// (`&amp;`) and a numeric-entity title (`&#8212;`), a variant printing repeating a number
    /// within one drop (deduped), and a number claimed by both drops (the first keeps it).
    const FIXTURE: &str = r#"
      <div>
        <h2 class="card-grid-header" id="wild-in-bloom"><span class="card-grid-header-content">Wild &amp; Bloom<span class="card-grid-header-dot">·</span></span> 5 cards</h2>
        <a href="/card/sld/2658/wild-a">A</a>
        <a href="/card/sld/2658/wild-a-foil">A foil (dup)</a>
        <a href="/card/sld/2659/wild-b">B</a>
        <h2 class="card-grid-header" id="inked"><span class="card-grid-header-content">Inked &#8212; Special<span class="card-grid-header-dot">·</span></span> 2 cards</h2>
        <a href="/card/sld/2659/collide">collides with wild-in-bloom</a>
        <a href="/card/sld/168/ink-a">168</a>
      </div>
    "#;

    /// A local mirror of the snapshot JSON shape, so the test asserts on parsed drops without
    /// reaching into the drop store's internals.
    #[derive(Deserialize)]
    struct Parsed {
        sets: Vec<ParsedSet>,
    }
    #[derive(Deserialize)]
    struct ParsedSet {
        game: String,
        set: String,
        drops: Vec<ParsedDrop>,
    }
    #[derive(Deserialize)]
    struct ParsedDrop {
        slug: String,
        title: String,
        collector_numbers: Vec<String>,
    }

    fn parse(json: &str) -> Parsed {
        serde_json::from_str(json).expect("snapshot JSON parses")
    }

    #[test]
    fn extracts_titles_collector_numbers_and_order() {
        let json = build_snapshot_json(FIXTURE).expect("builds a snapshot");
        let parsed = parse(&json);
        assert_eq!(parsed.sets.len(), 1);
        assert_eq!(parsed.sets[0].game, "mtg");
        assert_eq!(parsed.sets[0].set, "sld");
        let drops = &parsed.sets[0].drops;
        assert_eq!(drops.len(), 2);

        // Drop 0: entity-decoded title, variant dupe (second 2658) collapsed.
        assert_eq!(drops[0].slug, "wild-in-bloom");
        assert_eq!(drops[0].title, "Wild & Bloom");
        assert_eq!(drops[0].collector_numbers, ["2658", "2659"]);

        // Drop 1: numeric-entity title; 2659 collided with drop 0 so only 168 remains.
        assert_eq!(drops[1].slug, "inked");
        assert_eq!(drops[1].title, "Inked \u{2014} Special");
        assert_eq!(drops[1].collector_numbers, ["168"]);
    }

    #[test]
    fn build_snapshot_json_is_installable_by_the_drop_store() {
        // The produced JSON round-trips through the drop store's own validating parser, so the
        // runtime scrape yields exactly what `install_snapshot` accepts.
        let json = build_snapshot_json(FIXTURE).expect("builds");
        assert!(
            crate::scryfall::drops::Tables::from_json(&json).is_ok(),
            "scraped snapshot must be accepted by the drop store's validating parser"
        );
    }

    #[test]
    fn no_headers_is_a_no_drops_error_not_an_empty_snapshot() {
        assert!(matches!(
            build_snapshot_json("<html><body>no drops here</body></html>"),
            Err(ScrapeError::NoDrops)
        ));
        // A header present but with no card links yields no drop -> still NoDrops (never installs
        // an empty table).
        let headers_no_cards = r#"<h2 class="card-grid-header" id="x"><span class="card-grid-header-content">X<span class="card-grid-header-dot">·</span></span></h2>"#;
        assert!(matches!(
            build_snapshot_json(headers_no_cards),
            Err(ScrapeError::NoDrops)
        ));
    }

    #[test]
    fn identical_html_serializes_byte_identically() {
        // Determinism: two scrapes of the same page produce the same bytes (so the same content
        // hash), which is what keeps an unchanged re-scrape from bumping the version.
        assert_eq!(
            build_snapshot_json(FIXTURE).unwrap(),
            build_snapshot_json(FIXTURE).unwrap()
        );
    }

    #[test]
    fn decodes_entities_and_percent_escapes() {
        assert_eq!(
            decode_entities("Rock &amp; Roll &#8212; Live"),
            "Rock & Roll — Live"
        );
        assert_eq!(decode_entities("Caf&#xe9;"), "Café");
        // An unknown named entity is left untouched.
        assert_eq!(decode_entities("A &bogus; B"), "A &bogus; B");
        // Percent-decode a foil-star collector number; a stray '%' is passed through.
        assert_eq!(decode_percent("%E2%98%85"), "\u{2605}");
        assert_eq!(decode_percent("100%"), "100%");
        assert_eq!(decode_percent("42"), "42");
    }
}
