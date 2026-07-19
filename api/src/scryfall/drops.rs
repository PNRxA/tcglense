//! Secret Lair Drop grouping.
//!
//! Scryfall breaks the Secret Lair Drop set (`sld`) into named "drops" (e.g.
//! "Wild in Bloom") on its gallery page, but those curated titles are **not** in
//! the bulk card API we ingest — they live only in the page's collector-number
//! groupings. We match each card to its drop by `(game, set_code, collector_number)`.
//!
//! **The drop table is a swappable runtime overlay, seeded by a committed snapshot.**
//! A committed `sld_drops.json` is embedded at compile time ([`SNAPSHOT_JSON`], still
//! regenerated for the fallback by `scripts/gen-sld-drops.mjs`) and seeds the store at
//! first access, so an offline / dummy / first-boot instance always has a working table.
//! At runtime the store is *replaced* by [`install_snapshot`] with a fresher snapshot:
//!
//! * the **mirror origin** re-scrapes Scryfall's gallery daily ([`super::sld_scrape`]) and
//!   installs the result, so its drops track new releases without a human re-running the
//!   script and re-deploying; and
//! * every **other instance** pulls that snapshot from the mirror daily ([`super::sld_sync`])
//!   and installs it — so a self-host's drops stay fresh without ever scraping Scryfall itself.
//!
//! The store is a process-global `RwLock<Arc<Tables>>` (not per-`AppState`) because the read
//! path reaches it from a bare `From<card::Model>` conversion with no state in hand; the swap
//! is the same brief-lock, clone-an-`Arc` pattern the fingerprint index uses. An install that
//! doesn't cover the Secret Lair set (a broken scrape returning zero drops) is **rejected**, so
//! a bad fetch can never wipe the good table — the store keeps whatever it last held.
//!
//! A card whose collector number isn't listed (e.g. a drop newer than the loaded snapshot)
//! simply has no drop, and callers fold it into an "Other" group — so a stale snapshot
//! degrades gracefully instead of hiding cards.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The committed drop snapshot, embedded at compile time. Seeds the runtime store and is
/// the fallback for an offline / first-boot instance; [`install_snapshot`] replaces it once
/// a fresher snapshot is scraped (origin) or imported from the mirror (consumer).
const SNAPSHOT_JSON: &str = include_str!("sld_drops.json");

/// The canonical JSON an empty store serves — valid, but with no drops. Used only when the
/// embedded snapshot fails to parse (guarded against by `snapshot_parses`), so drop grouping
/// degrades to "off" rather than taking the server down.
const EMPTY_SNAPSHOT_JSON: &str = r#"{"sets":[]}"#;

#[derive(Deserialize)]
struct RawSnapshot {
    sets: Vec<RawSetDrops>,
}

#[derive(Deserialize)]
struct RawSetDrops {
    game: String,
    set: String,
    drops: Vec<RawDrop>,
}

#[derive(Deserialize)]
struct RawDrop {
    slug: String,
    title: String,
    collector_numbers: Vec<String>,
}

/// One Secret Lair drop: a curated title plus its position in Scryfall's display
/// order (the snapshot's order, newest first).
#[derive(Debug, Clone)]
pub struct Drop {
    pub slug: String,
    pub title: String,
    /// The drop's card collector numbers (in the set), the join key onto `cards`.
    pub collector_numbers: Vec<String>,
    /// 0-based index in the set's drop order, for stable section ordering.
    pub order: usize,
}

/// A set's drop lookup: the ordered drops plus collector-number and title indexes.
pub struct DropTable {
    drops: Vec<Drop>,
    by_collector: HashMap<String, usize>,
    by_title: HashMap<String, usize>,
}

impl DropTable {
    /// The drop a card belongs to, by collector number, or `None` when the
    /// snapshot doesn't list that number (a newer-than-snapshot printing).
    pub fn drop_for(&self, collector_number: &str) -> Option<&Drop> {
        self.by_collector
            .get(collector_number)
            .map(|&i| &self.drops[i])
    }

    /// The drop whose title matches, comparing on [`normalize_title`] (case- and
    /// punctuation-insensitive). Used to map a Secret Lair *sealed product* to its
    /// drop by name so the drop's cards can populate the product's contents.
    pub fn drop_by_title(&self, normalized_title: &str) -> Option<&Drop> {
        self.by_title.get(normalized_title).map(|&i| &self.drops[i])
    }

    /// The drop with this slug (an exact, curated key — used by product overrides).
    pub fn drop_by_slug(&self, slug: &str) -> Option<&Drop> {
        self.drops.iter().find(|d| d.slug == slug)
    }

    /// Whether the snapshot lists any drops for this set.
    pub fn is_empty(&self) -> bool {
        self.drops.is_empty()
    }
}

/// A parsed, validated set of per-set drop tables plus the exact JSON they were built from
/// (re-served by the mirror endpoint) and a stable content hash of it (the mirror `ETag` +
/// the sealed-contents derivation's version key). The swappable unit installed into the store.
pub struct Tables {
    by_key: HashMap<String, Arc<DropTable>>,
    /// The canonical snapshot JSON these tables were built from — served verbatim by the
    /// mirror endpoint so consumers install the same drops.
    canonical_json: String,
    /// A stable content hash (16 hex chars) of the *drop data itself*, **not** the JSON bytes
    /// (see [`data_content_hash`]). So the pretty-printed committed seed and the mirror's compact
    /// scrape of the *same* drops share a version — a re-scrape / re-import / reboot-reseed of
    /// unchanged drops produces the same version (no spurious downstream re-derivation, and a
    /// conditional mirror fetch is a `304`), while any real drop change bumps it.
    content_version: String,
}

/// Why an install was rejected. The consumer only fetches from the mirror it was configured
/// to trust and the origin only installs its own scrape, so this is a version skew / a broken
/// scrape, not adversarial input — but it must never wipe the good table.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("drop snapshot JSON did not parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("drop snapshot has no drops for the Secret Lair set (mtg/sld) — refusing to install")]
    MissingSld,
}

impl Tables {
    /// Parse + validate a snapshot JSON into per-set tables. Rejects a snapshot that doesn't
    /// cover the Secret Lair set with a non-empty drop list ([`SnapshotError::MissingSld`]), so
    /// a broken scrape can't install an empty table over the good one.
    pub fn from_json(json: &str) -> Result<Tables, SnapshotError> {
        let snapshot: RawSnapshot = serde_json::from_str(json)?;
        let by_key = build_tables(snapshot);
        // The one set we always expect; if a fetch dropped it (markup change → zero headers),
        // refuse the install rather than blank out drop grouping site-wide.
        let has_sld = by_key
            .get(&key(super::GAME, SLD_SET_CODE))
            .is_some_and(|t| !t.is_empty());
        if !has_sld {
            return Err(SnapshotError::MissingSld);
        }
        Ok(Tables {
            content_version: data_content_hash(&by_key),
            canonical_json: json.to_string(),
            by_key,
        })
    }

    /// An empty store: no drops, valid JSON to serve. Used only as the degraded seed when the
    /// embedded snapshot itself fails to parse.
    fn empty() -> Tables {
        let by_key = HashMap::new();
        Tables {
            canonical_json: EMPTY_SNAPSHOT_JSON.to_string(),
            content_version: data_content_hash(&by_key),
            by_key,
        }
    }

    fn get(&self, game: &str, set_code: &str) -> Option<Arc<DropTable>> {
        self.by_key.get(&key(game, set_code)).cloned()
    }

    /// Total drops across every set — for the install log line.
    fn total_drops(&self) -> usize {
        self.by_key.values().map(|t| t.drops.len()).sum()
    }
}

/// The Secret Lair set code (lowercased, as cards/products store it). The set the snapshot
/// groups; the install guard requires it.
const SLD_SET_CODE: &str = "sld";

/// Canonicalise a drop/product title for case- and punctuation-insensitive matching:
/// lowercased, `&` expanded to `and`, every run of non-alphanumeric characters collapsed
/// to a single space, trimmed. So `"Garfield: Our Only Thought Is To Entertain You"` and
/// the snapshot's `"Garfield: Our Only Thought Is to Entertain You"` both normalise to
/// `"garfield our only thought is to entertain you"`.
pub fn normalize_title(title: &str) -> String {
    let expanded = title.replace('&', " and ");
    let mut out = String::with_capacity(expanded.len());
    let mut pending_space = false;
    for ch in expanded.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    out
}

/// Composite map key. A handful of entries (one per drop-grouped set), so the
/// per-lookup `format!` is immaterial.
fn key(game: &str, set_code: &str) -> String {
    format!("{game}/{set_code}")
}

/// A stable content hash (first 8 bytes of SHA-256, hex) of the *drop data itself* — each set's
/// ordered drops (slug, title, collector numbers), keyed set-order-independently — rather than the
/// raw JSON bytes. So two snapshots with the same drops hash identically **regardless of JSON
/// representation**: the pretty-printed committed seed (`sld_drops.json`, with `//` note keys) and
/// the mirror's compact scrape produce the *same* version for the *same* drops. This matters
/// because the version feeds the mtgjson/tcgcsv sealed-contents ETag gate — hashing the raw bytes
/// would make a reboot (which reseeds from the committed file) look like a change versus the
/// last-imported compact snapshot and trigger a needless full `AllPrintings` rebuild. Delimiters
/// are control bytes that can't appear in a slug/title/number, so distinct data can't collide.
fn data_content_hash(by_key: &HashMap<String, Arc<DropTable>>) -> String {
    let mut hasher = Sha256::new();
    // Sort the set keys so HashMap iteration order can't change the hash.
    let mut keys: Vec<&String> = by_key.keys().collect();
    keys.sort_unstable();
    for key in keys {
        hasher.update(key.as_bytes());
        hasher.update(b"\x00");
        // Drops in their snapshot order (significant — it's the display order).
        for drop in &by_key[key].drops {
            hasher.update(drop.slug.as_bytes());
            hasher.update(b"\x01");
            hasher.update(drop.title.as_bytes());
            hasher.update(b"\x02");
            for cn in &drop.collector_numbers {
                hasher.update(cn.as_bytes());
                hasher.update(b"\x03");
            }
            hasher.update(b"\x04");
        }
        hasher.update(b"\x05");
    }
    hex::encode(&hasher.finalize()[..8])
}

/// Build the per-set drop tables from a parsed snapshot (the pure part of [`Tables::from_json`],
/// shared with the degraded seed path).
fn build_tables(snapshot: RawSnapshot) -> HashMap<String, Arc<DropTable>> {
    let mut tables = HashMap::new();
    for set in snapshot.sets {
        let mut drops = Vec::with_capacity(set.drops.len());
        let mut by_collector = HashMap::new();
        let mut by_title = HashMap::new();
        for (order, raw) in set.drops.into_iter().enumerate() {
            for cn in &raw.collector_numbers {
                // The first drop to list a number keeps it; the snapshot is
                // generated collision-free, but guard against a future dupe
                // silently reassigning a card.
                by_collector.entry(cn.clone()).or_insert(order);
            }
            // Likewise for titles: the first drop with a given normalised title wins,
            // so a rare title collision can't silently reassign the earlier drop.
            by_title.entry(normalize_title(&raw.title)).or_insert(order);
            drops.push(Drop {
                slug: raw.slug,
                title: raw.title,
                collector_numbers: raw.collector_numbers,
                order,
            });
        }
        tables.insert(
            key(&set.game, &set.set),
            Arc::new(DropTable {
                drops,
                by_collector,
                by_title,
            }),
        );
    }
    tables
}

/// The process-global drop store, seeded lazily from the embedded snapshot. Swapped wholesale
/// by [`install_snapshot`] when a fresher snapshot is scraped/imported (the same brief-lock,
/// clone-an-`Arc` pattern the fingerprint index uses).
static STORE: LazyLock<RwLock<Arc<Tables>>> = LazyLock::new(|| {
    let seed = Tables::from_json(SNAPSHOT_JSON).unwrap_or_else(|err| {
        // A malformed committed snapshot disables drop grouping rather than taking the server
        // down; `snapshot_parses` guards the shipped file so this branch is unreachable in
        // practice.
        tracing::error!(error = %err, "failed to parse embedded sld_drops.json; drop grouping disabled");
        Tables::empty()
    });
    RwLock::new(Arc::new(seed))
});

/// A snapshot of the current store. Clones the inner `Arc` under a brief read lock so callers
/// read a stable table without holding the lock, and a concurrent [`install_snapshot`] swap
/// never disturbs an in-flight read.
fn store() -> Arc<Tables> {
    STORE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Replace the store with a freshly-fetched snapshot (the mirror origin's daily scrape or a
/// consumer's daily import). Validates first ([`Tables::from_json`]): a snapshot missing the
/// Secret Lair set is rejected and the current store is left untouched, so a broken fetch can
/// never wipe the good table. Returns the number of drops installed on success.
pub fn install_snapshot(json: &str) -> Result<usize, SnapshotError> {
    let tables = Tables::from_json(json)?;
    let count = tables.total_drops();
    *STORE.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(tables);
    Ok(count)
}

/// The current snapshot's canonical JSON paired with its content version, read from a **single**
/// store snapshot. The mirror's SLD-drops endpoint uses this so the `ETag` (from the version) and
/// the body (the JSON) always describe the *same* snapshot even if a concurrent [`install_snapshot`]
/// swaps the store — reading the JSON and the version in two separate `store()` calls could straddle
/// a swap and pair an old `ETag` with a new body.
pub fn current_snapshot() -> (String, String) {
    let store = store();
    (store.canonical_json.clone(), store.content_version.clone())
}

/// A stable content hash of the currently-loaded snapshot (16 hex chars; see [`data_content_hash`]).
/// Feeds the sealed-contents derivation's version gate, so a drop refresh re-runs the derivation.
/// (The mirror endpoint reads it together with the body via [`current_snapshot`].)
pub fn content_version() -> String {
    store().content_version.clone()
}

/// The content version the store boots on: the committed seed's ([`SNAPSHOT_JSON`]) version, computed
/// once. Built the same way [`STORE`] is seeded — the parsed committed snapshot, or the empty table
/// if it somehow fails to parse — so [`store_is_seed`] compares like with like.
fn seed_content_version() -> &'static str {
    static SEED: LazyLock<String> = LazyLock::new(|| {
        Tables::from_json(SNAPSHOT_JSON)
            .unwrap_or_else(|_| Tables::empty())
            .content_version
    });
    SEED.as_str()
}

/// Whether the drop store still holds the committed seed — i.e. no fresher snapshot has been
/// installed in this process. **True right after every restart**, because the in-memory store
/// reseeds from the committed snapshot on boot (there is no persisted snapshot to restore). The sync
/// loops ([`super::sld_tasks`]) use this to refresh at startup rather than deferring onto a stale
/// seed: a scrape/import that succeeded before the restart left only a persisted *timestamp*, not the
/// data, so honouring that timestamp would keep serving the committed fallback for up to an interval.
///
/// A freshly-installed snapshot whose drops are byte-for-byte the seed's hashes to the same version
/// and so also reads as "the seed" — harmless: the data served is identical, and the only effect is
/// one redundant refresh on the next boot.
pub fn store_is_seed() -> bool {
    content_version().as_str() == seed_content_version()
}

/// The drop table for a game's set, or `None` if that set isn't drop-grouped in the current
/// snapshot. Returns an owned `Arc` so the caller holds a stable table across `await`s even if
/// the store is swapped underneath.
pub fn table(game: &str, set_code: &str) -> Option<Arc<DropTable>> {
    store().get(game, set_code)
}

/// Whether this set is broken into Secret Lair-style drops in the current snapshot.
pub fn has_drops(game: &str, set_code: &str) -> bool {
    table(game, set_code).is_some_and(|t| !t.is_empty())
}

/// The drop a single card belongs to, if its set is drop-grouped and the current snapshot
/// lists its collector number. Returns an owned [`Drop`] clone (a per-card call from the card
/// DTO conversion, which has no store handle to borrow from); `None` for the common non-drop
/// card is free.
pub fn drop_for(game: &str, set_code: &str, collector_number: &str) -> Option<Drop> {
    table(game, set_code).and_then(|t| t.drop_for(collector_number).cloned())
}

/// Curated Secret Lair **spend-incentive** printings: promo cards handed out for reaching a
/// cart spend threshold during a superdrop (e.g. the Avatar: The Last Airbender superdrop's
/// foil Path of Ancestry, one per $199 spent) rather than included with a specific drop.
/// Scryfall tags these `sldbonus` like the per-drop bonus cards, so on their own they read as
/// ordinary chase cards; this list lets the UI call them out distinctly as spend rewards.
/// Keyed by `(game, set_code, collector_number)` — kept tiny and curated, like the SLD
/// product overrides, because there's no upstream signal that separates a spend reward from
/// an ordinary bonus card.
const SPEND_INCENTIVES: &[(&str, &str, &str)] = &[
    // Each superdrop hands out one foil promo per qualifying spend threshold. All in `sld`.
    ("mtg", "sld", "903"), // The Locust God — Secretversary 2023 superdrop
    ("mtg", "sld", "905"), // Cryptic Command — Secret Scare superdrop
    ("mtg", "sld", "906"), // Ignoble Hierarch — Equinox 2024 superdrop
    ("mtg", "sld", "907"), // Seedborn Muse — Spring 2024 superdrop
    ("mtg", "sld", "908"), // Arcane Signet ("Earth's Mightiest Emblem") — Marvel superdrop
    ("mtg", "sld", "912"), // Sol Ring — PlayStation / Twisted Metal superdrop
    ("mtg", "sld", "914"), // Path of Ancestry — Avatar: The Last Airbender superdrop
    ("mtg", "sld", "915"), // Silver Shroud Costume — Fallout / Rad superdrop
    ("mtg", "sld", "918"), // Food Token ("Lasagna") — Garfield superdrop
];

/// Whether a printing is a curated Secret Lair spend-incentive promo (see [`SPEND_INCENTIVES`]).
pub fn is_spend_incentive(game: &str, set_code: &str, collector_number: &str) -> bool {
    SPEND_INCENTIVES
        .iter()
        .any(|&(g, s, cn)| g == game && s == set_code && cn == collector_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_parses_and_covers_sld() {
        let table = table("mtg", "sld").expect("sld is drop-grouped");
        // The shipped snapshot has hundreds of drops; assert a healthy floor so a
        // truncated/corrupt regeneration is caught.
        assert!(
            table.drops.len() > 100,
            "expected many drops, got {}",
            table.drops.len()
        );
        assert!(has_drops("mtg", "sld"));
    }

    #[test]
    fn known_collector_numbers_resolve_to_their_drop() {
        assert_eq!(
            drop_for("mtg", "sld", "2658").map(|d| d.title).as_deref(),
            Some("Wild in Bloom")
        );
        assert_eq!(
            drop_for("mtg", "sld", "168").map(|d| d.title).as_deref(),
            Some("Inked")
        );
        // A collector number the snapshot doesn't list -> no drop (folds to "Other").
        assert!(drop_for("mtg", "sld", "this-cn-does-not-exist").is_none());
    }

    #[test]
    fn spend_incentives_are_recognised() {
        // Curated superdrop spend rewards are recognised — the earliest (The Locust God
        // #903), a token promo (Food Token #918), and one in between (Path of Ancestry #914).
        assert!(is_spend_incentive("mtg", "sld", "903"));
        assert!(is_spend_incentive("mtg", "sld", "914"));
        assert!(is_spend_incentive("mtg", "sld", "918"));
        // An ordinary drop card is not; nor is the number in another set or game.
        assert!(!is_spend_incentive("mtg", "sld", "2658"));
        assert!(!is_spend_incentive("mtg", "blb", "914"));
        assert!(!is_spend_incentive("pokemon", "sld", "914"));
    }

    #[test]
    fn non_drop_sets_have_no_table() {
        assert!(table("mtg", "blb").is_none());
        assert!(!has_drops("mtg", "blb"));
        // Unknown game, too.
        assert!(drop_for("pokemon", "sld", "2658").is_none());
    }

    #[test]
    fn first_snapshot_drop_is_order_zero() {
        // Drop order mirrors the snapshot's order; the first drop is 0.
        assert_eq!(drop_for("mtg", "sld", "2658").map(|d| d.order), Some(0));
    }

    #[test]
    fn drops_carry_their_collector_numbers() {
        let table = table("mtg", "sld").expect("sld is drop-grouped");
        let drop = table
            .drop_by_title(&normalize_title("Wild in Bloom"))
            .expect("drop present");
        assert_eq!(
            drop.collector_numbers,
            ["2658", "2659", "2660", "2661", "2662"]
        );
    }

    #[test]
    fn drop_by_title_is_case_and_punctuation_insensitive() {
        let table = table("mtg", "sld").expect("sld is drop-grouped");
        // A product name capitalises "To" where the snapshot writes "to"; normalisation
        // collapses the difference, and the surrounding colon becomes a space.
        let normalized = normalize_title("Garfield: Our Only Thought Is To Entertain You");
        assert_eq!(
            table.drop_by_title(&normalized).map(|d| d.slug.as_str()),
            Some("garfield-our-only-thought-is-to-entertain-you"),
        );
        // A title that matches no drop resolves to nothing.
        assert!(
            table
                .drop_by_title(&normalize_title("Not A Real Drop"))
                .is_none()
        );
    }

    #[test]
    fn drop_by_slug_resolves_curated_overrides() {
        let table = table("mtg", "sld").expect("sld is drop-grouped");
        let drop = table
            .drop_by_slug("secret-lair-presents-nuestra-magia")
            .expect("Nuestra Magia drop present");
        assert_eq!(drop.title, "Secret Lair Presents: Nuestra Magia");
        assert!(table.drop_by_slug("no-such-slug").is_none());
    }

    #[test]
    fn normalize_title_expands_ampersand_and_collapses_separators() {
        assert_eq!(normalize_title("Rock & Roll"), "rock and roll");
        assert_eq!(
            normalize_title("  Witch's  Familiar!! "),
            "witch s familiar"
        );
        assert_eq!(
            normalize_title("FINAL FANTASY: Game Over"),
            "final fantasy game over"
        );
    }

    // ----- Runtime install / validation (over local `Tables`, never the global store, so
    // these can't race the rest of the suite) -----

    /// A minimal but valid snapshot for `set` with one drop.
    fn snapshot_with(game: &str, set: &str) -> String {
        format!(
            r#"{{"sets":[{{"game":"{game}","set":"{set}","drops":[{{"slug":"a","title":"A","collector_numbers":["1","2"]}}]}}]}}"#
        )
    }

    #[test]
    fn from_json_builds_tables_for_a_valid_snapshot() {
        let tables = Tables::from_json(&snapshot_with("mtg", "sld")).expect("valid");
        let table = tables.get("mtg", "sld").expect("sld present");
        assert_eq!(table.drop_for("1").map(|d| d.title.as_str()), Some("A"));
        assert_eq!(tables.total_drops(), 1);
    }

    #[test]
    fn from_json_rejects_a_snapshot_missing_sld() {
        // A snapshot that covers some *other* set but not sld is refused, so a broken scrape
        // can never install an sld-less table over the good one.
        assert!(matches!(
            Tables::from_json(&snapshot_with("mtg", "blb")),
            Err(SnapshotError::MissingSld)
        ));
        // An sld set present but with zero drops is likewise refused (the markup-change case).
        let empty_sld = r#"{"sets":[{"game":"mtg","set":"sld","drops":[]}]}"#;
        assert!(matches!(
            Tables::from_json(empty_sld),
            Err(SnapshotError::MissingSld)
        ));
    }

    #[test]
    fn from_json_rejects_malformed_json() {
        assert!(matches!(
            Tables::from_json("{ not json"),
            Err(SnapshotError::Parse(_))
        ));
    }

    #[test]
    fn content_version_is_stable_for_equal_and_differs_for_changed() {
        let a = Tables::from_json(&snapshot_with("mtg", "sld")).expect("valid");
        let b = Tables::from_json(&snapshot_with("mtg", "sld")).expect("valid");
        assert_eq!(a.content_version, b.content_version);
        assert_eq!(a.content_version.len(), 16); // 8 bytes hex-encoded
        // The live store (seeded from the shipped snapshot) has a different, non-empty version.
        assert!(!content_version().is_empty());
        assert_ne!(a.content_version, content_version());
    }

    #[test]
    fn content_version_ignores_json_representation_but_tracks_data() {
        // The version hashes the drop *data*, not the JSON bytes — so the pretty-printed committed
        // seed (with `//` note keys) and the mirror's compact scrape of the SAME drops share a
        // version. Without this, a reboot (which reseeds from the committed file) would look like a
        // change vs the last-imported compact snapshot and trip a needless full AllPrintings rebuild.
        let pretty = r#"{
          "//": "GENERATED by gen-sld-drops.mjs",
          "//2": "a second note",
          "sets": [
            { "game": "mtg", "set": "sld", "drops": [
              { "slug": "a", "title": "A", "collector_numbers": ["1", "2"] }
            ] }
          ]
        }"#;
        let compact = r#"{"//":"GENERATED at runtime","sets":[{"game":"mtg","set":"sld","drops":[{"slug":"a","title":"A","collector_numbers":["1","2"]}]}]}"#;
        let a = Tables::from_json(pretty).expect("valid");
        let b = Tables::from_json(compact).expect("valid");
        assert_eq!(
            a.content_version, b.content_version,
            "same drops -> same version regardless of JSON formatting / note keys"
        );
        // But a real data change (an added collector number) does bump it.
        let changed = r#"{"sets":[{"game":"mtg","set":"sld","drops":[{"slug":"a","title":"A","collector_numbers":["1","2","3"]}]}]}"#;
        let c = Tables::from_json(changed).expect("valid");
        assert_ne!(
            a.content_version, c.content_version,
            "a data change bumps the version"
        );
    }

    #[test]
    fn seed_content_version_matches_the_committed_snapshot() {
        // The cached seed version is exactly the version of the committed snapshot the store boots
        // on — deterministic, independent of the mutable global store.
        let seed = Tables::from_json(SNAPSHOT_JSON).expect("committed snapshot parses");
        assert_eq!(seed_content_version(), seed.content_version);
    }

    #[test]
    fn store_is_seed_when_serving_the_committed_snapshot() {
        // The global store boots on the committed seed and nothing in the suite installs a runtime
        // snapshot over it, so the live version matches the seed's and `store_is_seed` reports true.
        assert_eq!(content_version().as_str(), seed_content_version());
        assert!(store_is_seed());
    }
}
