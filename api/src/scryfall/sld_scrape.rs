//! Secret Lair **gallery scrape** — the runtime "fetch from source" the mirror origin runs.
//!
//! Scryfall's curated Secret Lair drop titles aren't in the bulk card API; they live only on a
//! set's gallery page, which groups the set's cards into named sections by collector number: the
//! Secret Lair Drop set (`/sets/sld`) into its "drops", and The Zeta Set (`/sets/slz`, a Secret
//! Lair release Scryfall filed as its own top-level set) into its three print treatments —
//! Photocopy / Photocopy Negatives / Color Banding — whose cards are otherwise
//! indistinguishable in the card data (every one is black-bordered, full-art, nonfoil, with no
//! promo type). [`drops::GALLERY_SETS`] names every gallery scraped; each is one page, parsed the
//! same way. `scripts/gen-sld-drops.mjs` scrapes the same pages **offline** to regenerate the committed
//! fallback (`sld_drops.json`); this is the same scrape ported to Rust so the **mirror origin** can
//! refresh its live drop tables daily from source without a human re-running the script and
//! redeploying (see [`super::sld_tasks`]). It emits JSON in the exact shape of `sld_drops.json`, so
//! it round-trips through [`super::drops::install_snapshot`] and the sealed-contents derivation
//! unchanged, and the mirror re-serves it verbatim to consumers.
//!
//! Scraping HTML is inherently brittle — a markup change yields **zero** drops, surfaced as
//! [`ScrapeError::NoDrops`] — so a broken scrape is never installed and never wipes the good table.
//! The two galleries fail independently, and are treated differently ([`resolve_set`]): the
//! **primary** set (`sld`, the one the drop store's install guard requires) failing fails the whole
//! scrape, so the origin keeps serving its last-good snapshot (falling back to the committed one);
//! a **secondary** set failing keeps that set's last-good table from the current store instead —
//! never an omission, which would flap the snapshot's content version (and with it the
//! sealed-contents derivation gate) on every transient fetch error, and never a block on the
//! primary's refresh. The parser is split from the fetch so it is unit-tested against a fixture
//! with no network.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use reqwest::header::USER_AGENT;
use serde::Serialize;

use crate::scryfall::drops;

/// The galleries scraped, in order — the drop store's registry ([`drops::GALLERY_SETS`]), which
/// also names what each set's sections are called. **The first is the primary set**: the one
/// [`drops::install_snapshot`] requires, whose failed scrape fails the whole run; every later
/// set is secondary and falls back to its last-good table (see [`resolve_set`]). `SETS` in
/// `scripts/gen-sld-drops.mjs` (the offline regeneration of the committed seed) mirrors the
/// codes — keep the two in step.
pub fn gallery_sets() -> impl Iterator<Item = &'static str> {
    drops::GALLERY_SETS.iter().map(|g| g.code)
}

/// The one set whose gallery must scrape for a snapshot to be built at all — the same set the
/// drop store's install guard checks for, so a run that would be rejected there is cut short here.
const PRIMARY_SET: &str = drops::GALLERY_SETS[0].code;

/// Game the snapshot is written for (matches the committed `sld_drops.json`).
const GAME: &str = super::GAME;

/// Pause between gallery requests, per Scryfall's request-rate guidance (50–100 ms). Two pages a
/// day hardly register, but there is no reason to fire them back to back.
const REQUEST_GAP: std::time::Duration = std::time::Duration::from_millis(100);

/// The gallery page a set's sections are scraped from.
fn gallery_url(set: &str) -> String {
    format!("https://scryfall.com/sets/{set}")
}

/// A failure scraping one gallery. Non-fatal at the call site (logged; the origin keeps serving
/// whatever snapshot it already had loaded). Names the set so a log line says *which* gallery
/// broke.
#[derive(Debug, thiserror::Error)]
pub enum ScrapeError {
    #[error("secret lair gallery request for '{set}' failed: {source}")]
    Http {
        set: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error(
        "secret lair gallery for '{set}' yielded no drop headers — Scryfall markup may have changed"
    )]
    NoDrops { set: &'static str },
}

/// A completed scrape: the snapshot JSON to install, plus which secondary sets it carries
/// forward unchanged because their own gallery failed — so the caller can say so in its log
/// line and its `ingest_state` detail rather than record a partially-fresh run as a clean one.
pub struct Scrape {
    pub json: String,
    pub carried_forward: Vec<&'static str>,
}

/// Fetch every gallery in [`gallery_sets`] and build the drop snapshot JSON (the shape of
/// `sld_drops.json`). Carries the configured Scryfall `User-Agent` (their API guidelines require a
/// descriptive one). Errors never panic: a failure on the primary set — network, non-2xx, or a
/// markup change that yields no drops — is returned and the caller keeps its last-good snapshot; a
/// failure on a secondary set is logged and that set keeps its last-good table
/// ([`resolve_set`]): the store's, or the committed seed's when the store has none (an
/// upgraded instance whose persisted snapshot predates the set).
pub async fn fetch_snapshot_json(
    http: &reqwest::Client,
    user_agent: &str,
) -> Result<Scrape, ScrapeError> {
    let mut sets = Vec::with_capacity(drops::GALLERY_SETS.len());
    for (i, set) in gallery_sets().enumerate() {
        if i > 0 {
            tokio::time::sleep(REQUEST_GAP).await;
        }
        let scraped = fetch_set(http, user_agent, set).await;
        let last_good = || {
            drops::table(GAME, set)
                .or_else(|| drops::seed_table(GAME, set))
                .filter(|table| !table.is_empty())
                .map(|table| table.drops().iter().map(ScrapedDrop::from).collect())
        };
        if let Some(resolved) = resolve_set(set, scraped, last_good)? {
            sets.push(resolved);
        }
    }
    let carried_forward = sets
        .iter()
        .filter(|s| s.carried_forward)
        .map(|s| s.set)
        .collect();
    Ok(Scrape {
        json: serialize_snapshot(&sets),
        carried_forward,
    })
}

/// GET one set's gallery and parse it into its sections.
async fn fetch_set(
    http: &reqwest::Client,
    user_agent: &str,
    set: &'static str,
) -> Result<Vec<ScrapedDrop>, ScrapeError> {
    let http_err = |source| ScrapeError::Http { set, source };
    let html = http
        .get(gallery_url(set))
        .header(USER_AGENT, user_agent)
        .send()
        .await
        .map_err(http_err)?
        .error_for_status()
        .map_err(http_err)?
        .text()
        .await
        .map_err(http_err)?;
    parse_set(&html, set)
}

/// Decide what the snapshot carries for one gallery set given how its scrape went. A successful
/// scrape is used as is. A failed scrape of the **primary** set fails the run (`Err`) — the drop
/// store would reject a snapshot without it anyway, and the origin keeps its last-good snapshot. A
/// failed scrape of a **secondary** set keeps that set's `last_good` table — the drops the store
/// currently holds, else the committed seed's (so an upgraded instance whose persisted snapshot
/// predates the set still carries it on the very scrape that fails) — so a transient fetch error
/// or a markup change on one gallery neither drops the set from the snapshot — which would flap
/// the content version — nor blocks the primary set's refresh. The carried-forward set is flagged
/// so the run is recorded as such. Only a secondary set with no last-good table anywhere is
/// omitted; the seed covers every registered set (pinned by
/// `the_committed_seed_covers_every_gallery_set`), so that branch is a defence, not a path.
/// Pure (the store/seed read is the caller's closure), so the policy is unit-tested without a
/// network or the global store.
fn resolve_set(
    set: &'static str,
    scraped: Result<Vec<ScrapedDrop>, ScrapeError>,
    last_good: impl FnOnce() -> Option<Vec<ScrapedDrop>>,
) -> Result<Option<ScrapedSet>, ScrapeError> {
    match scraped {
        Ok(drops) => Ok(Some(ScrapedSet {
            set,
            drops,
            carried_forward: false,
        })),
        Err(err) if set == PRIMARY_SET => Err(err),
        Err(err) => match last_good() {
            Some(drops) => {
                tracing::warn!(
                    set,
                    error = %err,
                    "secondary Secret Lair gallery scrape failed; keeping its last-good drops"
                );
                Ok(Some(ScrapedSet {
                    set,
                    drops,
                    carried_forward: true,
                }))
            }
            None => {
                tracing::warn!(
                    set,
                    error = %err,
                    "secondary Secret Lair gallery scrape failed with no last-good drops to keep; omitting the set"
                );
                Ok(None)
            }
        },
    }
}

/// One scraped drop: its slug, curated title, and the collector numbers whose cards it groups.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct ScrapedDrop {
    slug: String,
    title: String,
    collector_numbers: Vec<String>,
}

impl From<&drops::Drop> for ScrapedDrop {
    /// A drop the store (or the seed) already holds, re-emitted as scraped data — the
    /// carry-forward of a secondary set whose gallery failed to scrape.
    fn from(drop: &drops::Drop) -> Self {
        Self {
            slug: drop.slug.clone(),
            title: drop.title.clone(),
            collector_numbers: drop.collector_numbers.clone(),
        }
    }
}

/// One gallery set's sections, in the set's display order: freshly scraped, or — when
/// `carried_forward` — a secondary set's last-good drops re-emitted because its own gallery
/// failed this run.
#[derive(Debug, Clone, PartialEq)]
struct ScrapedSet {
    set: &'static str,
    drops: Vec<ScrapedDrop>,
    carried_forward: bool,
}

/// Parse one set's gallery HTML into its sections. Split from [`fetch_set`] so the parse is
/// unit-testable against a fixture. [`ScrapeError::NoDrops`] when the page yields no drop headers
/// (a markup change), so the caller never installs — and thus never serves — an empty table.
fn parse_set(html: &str, set: &'static str) -> Result<Vec<ScrapedDrop>, ScrapeError> {
    let drops = parse_drops(html, set);
    if drops.is_empty() {
        return Err(ScrapeError::NoDrops { set });
    }
    Ok(drops)
}

// Each drop is an `<h2 class="card-grid-header" id="slug">…title…</h2>` whose body is a grid of
// `/card/<set>/<collector-number>/…` links, up to the next such header. These mirror the regexes
// in `scripts/gen-sld-drops.mjs` (kept in step so the runtime scrape and the offline one agree).
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<h2 class="card-grid-header" id="([^"]+)">(.*?)</h2>"#)
        .expect("valid header regex")
});
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)card-grid-header-content"\s*>(.*?)<span class="card-grid-header-dot"#)
        .expect("valid title regex")
});
/// A card link's set code and collector number. The set is captured (not baked in) so one regex
/// serves every gallery; [`parse_drops`] keeps only the links into the set being parsed, so a
/// link to another set's printing on the page can never claim a slot in this set's sections.
static CARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"/card/([^/"]+)/([^/"]+)/"#).expect("valid card regex"));
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid tag regex"));
static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid ws regex"));

/// Parse a gallery's HTML into `set`'s ordered drops (Scryfall's display order — newest first for
/// `sld`, the print treatments in page order for `slz`). A drop's membership is the markup between
/// its own header and the next — sliced from the header match positions so it can't misalign onto
/// a different set of `<h2>`s — and only the card links into `set` count. A collector number is
/// kept by the first drop to list it (cross-drop collision guard), and a variant printing
/// repeating a number within one drop is deduped; a drop with no cards is dropped.
fn parse_drops(html: &str, set: &str) -> Vec<ScrapedDrop> {
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
            if m.get(1).expect("set capture").as_str() != set {
                continue; // a link to another set's printing, not one of this set's cards
            }
            let cn = decode_percent(m.get(2).expect("cn capture").as_str());
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
/// comment keys the drop-store parser ignores). Deterministic field/collection order (the sets in
/// [`gallery_sets`] order), so a re-scrape of unchanged drops serves byte-identical JSON — nice for
/// a warm CDN. The drop store's content version hashes the drop *data*, not these bytes (see
/// `drops::data_content_hash`), so the version stays stable across representations (the compact
/// scrape vs the pretty committed seed) regardless — that's what prevents a spurious downstream
/// re-derivation on reboot.
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

fn serialize_snapshot(sets: &[ScrapedSet]) -> String {
    let snapshot = Snapshot {
        note: "GENERATED at runtime by scryfall::sld_scrape from Scryfall's Secret Lair galleries.",
        sets: sets
            .iter()
            .map(|scraped| SnapshotSet {
                game: GAME,
                set: scraped.set,
                drops: &scraped.drops,
            })
            .collect(),
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

    /// A Secret Lair Drop gallery fragment with the structure the scraper keys on: two drops, an
    /// HTML-entity title (`&amp;`) and a numeric-entity title (`&#8212;`), a variant printing
    /// repeating a number within one drop (deduped), and a number claimed by both drops (the
    /// first keeps it).
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

    /// A Zeta Set gallery fragment: the three print-treatment sections Scryfall files the set
    /// under, plus a stray link to a Secret Lair Drop printing (the kind of cross-set link a
    /// gallery page carries) that must not claim a slot in the Zeta Set's sections.
    const ZETA_FIXTURE: &str = r#"
      <div>
        <h2 class="card-grid-header" id="photocopy-cards"><span class="card-grid-header-content">Photocopy Cards<span class="card-grid-header-dot">·</span></span> 121 cards</h2>
        <a href="/card/slz/1/all-that-glitters">1</a>
        <a href="/card/sld/2658/not-this-set">a Secret Lair Drop printing</a>
        <a href="/card/slz/2/dispatch">2</a>
        <h2 class="card-grid-header" id="photocopy-negatives"><span class="card-grid-header-content">Photocopy Negatives<span class="card-grid-header-dot">·</span></span> 121 cards</h2>
        <a href="/card/slz/122/all-that-glitters">122</a>
        <h2 class="card-grid-header" id="color-banding-cards"><span class="card-grid-header-content">Color Banding Cards<span class="card-grid-header-dot">·</span></span> 121 cards</h2>
        <a href="/card/slz/243/all-that-glitters">243</a>
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

    fn drop(slug: &str, numbers: &[&str]) -> ScrapedDrop {
        ScrapedDrop {
            slug: slug.to_string(),
            title: slug.to_uppercase(),
            collector_numbers: numbers.iter().map(|n| n.to_string()).collect(),
        }
    }

    /// A freshly-scraped set (not carried forward).
    fn fresh(set: &'static str, drops: Vec<ScrapedDrop>) -> ScrapedSet {
        ScrapedSet {
            set,
            drops,
            carried_forward: false,
        }
    }

    #[test]
    fn extracts_titles_collector_numbers_and_order() {
        let drops = parse_set(FIXTURE, "sld").expect("parses the gallery");
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
    fn parses_the_zeta_set_into_its_treatment_sections() {
        let drops = parse_set(ZETA_FIXTURE, "slz").expect("parses the gallery");
        let titles: Vec<&str> = drops.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Photocopy Cards",
                "Photocopy Negatives",
                "Color Banding Cards"
            ]
        );
        // Only the links into `slz` count: the stray Secret Lair Drop link inside the first
        // section neither joins it nor breaks the section's own numbers.
        assert_eq!(drops[0].collector_numbers, ["1", "2"]);
        assert_eq!(drops[1].collector_numbers, ["122"]);
        assert_eq!(drops[2].collector_numbers, ["243"]);
    }

    #[test]
    fn a_link_into_another_set_never_claims_a_section() {
        // Parsed as `sld`, the same page yields only the section that carried an `sld` link —
        // the set filter is the link's set code, not the page.
        let drops = parse_set(ZETA_FIXTURE, "sld").expect("one sld link on the page");
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].slug, "photocopy-cards");
        assert_eq!(drops[0].collector_numbers, ["2658"]);
    }

    #[test]
    fn snapshot_of_every_gallery_is_installable_by_the_drop_store() {
        // The produced JSON round-trips through the drop store's own validating parser, so the
        // runtime scrape yields exactly what `install_snapshot` accepts — with every gallery set
        // in `GALLERY_SETS` order, each under the one game.
        let sets = vec![
            fresh("sld", parse_set(FIXTURE, "sld").unwrap()),
            fresh("slz", parse_set(ZETA_FIXTURE, "slz").unwrap()),
        ];
        let json = serialize_snapshot(&sets);
        assert!(
            crate::scryfall::drops::Tables::from_json(&json).is_ok(),
            "scraped snapshot must be accepted by the drop store's validating parser"
        );
        let parsed = parse(&json);
        let codes: Vec<(&str, &str)> = parsed
            .sets
            .iter()
            .map(|s| (s.game.as_str(), s.set.as_str()))
            .collect();
        assert_eq!(codes, [("mtg", "sld"), ("mtg", "slz")]);
        assert_eq!(parsed.sets[1].drops.len(), 3);
        assert_eq!(parsed.sets[1].drops[1].slug, "photocopy-negatives");
        assert_eq!(parsed.sets[1].drops[1].title, "Photocopy Negatives");
        assert_eq!(parsed.sets[1].drops[1].collector_numbers, ["122"]);
        assert_eq!(parsed.sets[0].drops[0].slug, "wild-in-bloom");
    }

    #[test]
    fn no_headers_is_a_no_drops_error_not_an_empty_snapshot() {
        assert!(matches!(
            parse_set("<html><body>no drops here</body></html>", "sld"),
            Err(ScrapeError::NoDrops { set: "sld" })
        ));
        // A header present but with no card links yields no drop -> still NoDrops (never installs
        // an empty table). The error names the gallery that broke.
        let headers_no_cards = r#"<h2 class="card-grid-header" id="x"><span class="card-grid-header-content">X<span class="card-grid-header-dot">·</span></span></h2>"#;
        assert!(matches!(
            parse_set(headers_no_cards, "slz"),
            Err(ScrapeError::NoDrops { set: "slz" })
        ));
    }

    #[test]
    fn identical_html_serializes_byte_identically() {
        // Determinism: two scrapes of the same pages produce the same bytes (so the same content
        // hash), which is what keeps an unchanged re-scrape from bumping the version.
        let snapshot = || {
            serialize_snapshot(&[
                fresh("sld", parse_set(FIXTURE, "sld").unwrap()),
                fresh("slz", parse_set(ZETA_FIXTURE, "slz").unwrap()),
            ])
        };
        assert_eq!(snapshot(), snapshot());
    }

    // ----- Per-set failure policy (`resolve_set`) -----

    #[test]
    fn a_primary_set_failure_fails_the_run() {
        // The drop store would reject a snapshot without `sld` anyway, so the run is cut short
        // here — and the store is never consulted for a stand-in.
        let result = resolve_set("sld", Err(ScrapeError::NoDrops { set: "sld" }), || {
            panic!("the primary set never falls back to its last-good drops")
        });
        assert!(matches!(result, Err(ScrapeError::NoDrops { set: "sld" })));
    }

    #[test]
    fn a_secondary_set_failure_keeps_its_last_good_drops() {
        let last_good = vec![drop("photocopy-cards", &["1", "2"])];
        let kept = last_good.clone();
        let resolved = resolve_set("slz", Err(ScrapeError::NoDrops { set: "slz" }), || {
            Some(kept)
        })
        .expect("a secondary failure never fails the run");
        assert_eq!(
            resolved,
            Some(ScrapedSet {
                set: "slz",
                drops: last_good,
                carried_forward: true,
            })
        );
    }

    #[test]
    fn a_secondary_set_failure_with_nothing_to_keep_is_omitted() {
        let resolved = resolve_set("slz", Err(ScrapeError::NoDrops { set: "slz" }), || None)
            .expect("a secondary failure never fails the run");
        assert_eq!(resolved, None);
    }

    #[test]
    fn a_successful_scrape_is_used_as_is() {
        let drops = vec![drop("color-banding-cards", &["243"])];
        let resolved = resolve_set("slz", Ok(drops.clone()), || {
            panic!("a successful scrape never consults the store")
        })
        .expect("ok");
        assert_eq!(resolved, Some(fresh("slz", drops)));
    }

    #[test]
    fn a_carried_forward_drop_round_trips_the_store_row() {
        // The carry-forward re-emits what the store holds, field for field.
        let stored = drops::Drop {
            slug: "photocopy-negatives".into(),
            title: "Photocopy Negatives".into(),
            collector_numbers: vec!["122".into(), "123".into()],
            order: 1,
        };
        assert_eq!(
            ScrapedDrop::from(&stored),
            ScrapedDrop {
                slug: "photocopy-negatives".into(),
                title: "Photocopy Negatives".into(),
                collector_numbers: vec!["122".into(), "123".into()],
            }
        );
    }

    // ----- The gallery set list -----

    #[test]
    fn the_primary_set_is_the_one_the_install_guard_requires() {
        // A snapshot built without `mtg/sld` would be rejected by `install_snapshot`, which is
        // why its failure fails the run before any secondary page is fetched.
        assert_eq!(PRIMARY_SET, "sld");
        assert_eq!(gallery_sets().next(), Some(PRIMARY_SET));
        assert_eq!(gallery_url("slz"), "https://scryfall.com/sets/slz");
    }

    #[test]
    fn the_committed_seed_covers_every_gallery_set() {
        // Both directions of the seed ↔ registry coupling. Forward: the offline / first-boot
        // fallback must group every set the runtime scrape groups, or a self-host that never
        // reaches the mirror shows one set by drop and another flat — and the seed is what the
        // carry-forward falls back to on an upgraded instance. Reverse: a set the seed carries
        // but the scrape doesn't walk would go stale forever without anyone noticing.
        let registered: Vec<&str> = gallery_sets().collect();
        for set in &registered {
            let table = drops::seed_table(GAME, set)
                .unwrap_or_else(|| panic!("the committed seed must cover {set}"));
            assert!(!table.drops().is_empty(), "{set} has drops in the seed");
        }
        for seeded in drops::seed_set_codes(GAME) {
            assert!(
                registered.contains(&seeded.as_str()),
                "the seed carries {seeded}, which no gallery scrape refreshes — register it"
            );
        }
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
