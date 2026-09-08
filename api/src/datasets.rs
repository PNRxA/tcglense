//! Where the background card-data sync pulls each provider's **raw dataset** from.
//!
//! The big dataset files — Scryfall's `default_cards` bulk file + set list, MTGJSON's
//! `AllPrintings.json.gz`, and TCGCSV's catalog / prices / archives — can be fetched
//! either straight from the upstream services or from a **TCGLense mirror**: another
//! TCGLense instance (the public site, `tcglense.com`) re-serving those same files under
//! its `/api/mirror/*` endpoints (see [`crate::handlers::mirror`]).
//!
//! **By default a self-host reads from the mirror** ([`Config::dataset_mirror_url`],
//! default `https://tcglense.com`): it offloads the upstream providers, rides the
//! mirror's CDN, and needs no special User-Agent for the bot-walled sources. Set
//! `SYNC_FROM_UPSTREAM=true` ([`Config::sync_from_upstream`]) to fetch directly from
//! Scryfall / MTGJSON / TCGCSV instead — the posture the mirror host itself runs (it has
//! to be the one origin that talks to the real services).
//!
//! [`SyncSource`] is the single seam that turns that choice into the concrete base URLs
//! the provider clients hit; nothing else in the sync path knows about the mirror.

use crate::config::Config;

/// Path prefix the mirror-mode URL builders below target for the Scryfall dataset
/// (bulk-data catalog, aggregated set list, streamed bulk card file). Must match the
/// literal routes registered in [`crate::router`] (kept in step by these tests + the
/// mirror-route security tests).
pub const SCRYFALL_MIRROR_PREFIX: &str = "/api/mirror/scryfall";
/// Path prefix the mirror-mode URL builder targets for MTGJSON's `AllPrintings.json.gz`.
pub const MTGJSON_MIRROR_PREFIX: &str = "/api/mirror/mtgjson";
/// Path prefix the mirror-mode URL builder targets for arbitrary TCGCSV paths
/// (last-updated, groups, products, prices, and the daily price archives).
pub const TCGCSV_MIRROR_PREFIX: &str = "/api/mirror/tcgcsv";
/// Path prefix the mirror-mode URL builder targets for the Commander Spellbook combo
/// snapshot (issue #683) — the origin's compact re-serve of the upstream export, not the
/// export itself.
pub const SPELLBOOK_MIRROR_PREFIX: &str = "/api/mirror/spellbook";

/// Resolves each provider's dataset base URL to either its real upstream or a mirror.
///
/// Cheap to clone (one `bool` + one short `String`). Constructed once per sync from
/// [`Config`] and passed down into the Scryfall / MTGJSON / TCGCSV refresh paths.
#[derive(Clone, Debug)]
pub struct SyncSource {
    from_upstream: bool,
    /// Mirror origin with any trailing slash trimmed, so URL joins never double up.
    mirror_base: String,
    /// Whether the combo dataset is synced at all (`COMBOS_SYNC_ENABLED`). Carried here
    /// rather than threaded as a separate flag because "no source" is the natural way to
    /// say "don't fetch" — [`Self::spellbook_combos_url`] answers `None`.
    combos_enabled: bool,
    /// How often, in hours, the upstream export is asked for at all
    /// (`COMBOS_UPSTREAM_INTERVAL_DAYS` × 24; `0` = every tick). Only meaningful in
    /// upstream mode — a mirror consumer polls the origin every tick, which costs the
    /// source nothing.
    combos_upstream_interval_hours: u64,
}

impl SyncSource {
    /// Build from application config.
    pub fn from_config(config: &Config) -> Self {
        Self::new(config.sync_from_upstream, config.dataset_mirror_url.clone())
            .with_combos(config.combos_sync_enabled)
            .with_combos_upstream_interval(config.combos_upstream_interval_days.saturating_mul(24))
    }

    /// Construct directly (used by tests). Trims a trailing slash off `mirror_base`
    /// defensively so callers needn't (`Config` already trims its stored value).
    pub fn new(from_upstream: bool, mirror_base: impl Into<String>) -> Self {
        Self {
            from_upstream,
            mirror_base: mirror_base.into().trim_end_matches('/').to_string(),
            combos_enabled: true,
            combos_upstream_interval_hours: 0,
        }
    }

    /// Set how often (hours) the upstream export is fetched; `0` = every tick.
    pub fn with_combos_upstream_interval(mut self, hours: u64) -> Self {
        self.combos_upstream_interval_hours = hours;
        self
    }

    /// The upstream fetch cadence in hours (`0` = every tick). See
    /// [`crate::spellbook::ingest`] for how a completed import younger than this is skipped.
    pub fn combos_upstream_interval_hours(&self) -> u64 {
        self.combos_upstream_interval_hours
    }

    /// Switch the combo dataset on or off (see [`Self::spellbook_combos_url`]).
    pub fn with_combos(mut self, enabled: bool) -> Self {
        self.combos_enabled = enabled;
        self
    }

    /// Whether the sync talks to the real upstream services (else a TCGLense mirror).
    /// The Commander Spellbook ingest branches on this: the upstream export and the
    /// mirror's re-serve are different documents (see [`crate::spellbook::ingest`]).
    pub fn from_upstream(&self) -> bool {
        self.from_upstream
    }

    // ---------- Scryfall ----------

    /// The bulk-data catalog URL (small JSON describing each downloadable file).
    pub fn scryfall_bulk_data_url(&self) -> String {
        if self.from_upstream {
            crate::scryfall::BULK_DATA_URL.to_string()
        } else {
            format!("{}{SCRYFALL_MIRROR_PREFIX}/bulk-data", self.mirror_base)
        }
    }

    /// The `/sets` list URL. The mirror folds all pages into one response, so the
    /// consumer's pagination loop terminates after a single request either way.
    pub fn scryfall_sets_url(&self) -> String {
        if self.from_upstream {
            crate::scryfall::SETS_URL.to_string()
        } else {
            format!("{}{SCRYFALL_MIRROR_PREFIX}/sets", self.mirror_base)
        }
    }

    /// In **mirror** mode, the URL to stream the bulk `kind` file from (overriding the
    /// catalog's embedded upstream `download_uri`, which points at Scryfall's own CDN).
    /// `None` in upstream mode, where the caller follows the real `download_uri`.
    pub fn scryfall_file_url(&self, kind: &str) -> Option<String> {
        (!self.from_upstream)
            .then(|| format!("{}{SCRYFALL_MIRROR_PREFIX}/file/{kind}", self.mirror_base))
    }

    // ---------- MTGJSON ----------

    /// Base URL the MTGJSON client joins `/AllPrintings.json.gz` onto.
    pub fn mtgjson_base_url(&self) -> String {
        if self.from_upstream {
            crate::mtgjson::BASE_URL.to_string()
        } else {
            format!("{}{MTGJSON_MIRROR_PREFIX}", self.mirror_base)
        }
    }

    // ---------- Commander Spellbook ----------

    /// Where the combo dataset (issue #683) is pulled from: upstream's gzipped bulk
    /// export, or the mirror's compact JSONL snapshot of it. `None` when the dataset is
    /// switched off (`COMBOS_SYNC_ENABLED=false`) — the ingest then skips without touching
    /// `ingest_state`.
    pub fn spellbook_combos_url(&self) -> Option<String> {
        if !self.combos_enabled {
            return None;
        }
        Some(if self.from_upstream {
            crate::spellbook::VARIANTS_URL.to_string()
        } else {
            format!("{}{SPELLBOOK_MIRROR_PREFIX}/combos", self.mirror_base)
        })
    }

    // ---------- TCGCSV ----------

    /// Base URL the TCGCSV client joins each path (`/last-updated.txt`,
    /// `/tcgplayer/{cat}/groups`, `/archive/…`, …) onto.
    pub fn tcgcsv_base_url(&self) -> String {
        if self.from_upstream {
            crate::tcgcsv::BASE_URL.to_string()
        } else {
            format!("{}{TCGCSV_MIRROR_PREFIX}", self.mirror_base)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_mode_uses_the_real_provider_urls() {
        let s = SyncSource::new(true, "https://tcglense.com");
        assert_eq!(s.scryfall_bulk_data_url(), crate::scryfall::BULK_DATA_URL);
        assert_eq!(s.scryfall_sets_url(), crate::scryfall::SETS_URL);
        // Upstream follows the catalog's own download_uri, so no override URL.
        assert_eq!(s.scryfall_file_url("default_cards"), None);
        assert_eq!(s.mtgjson_base_url(), crate::mtgjson::BASE_URL);
        assert_eq!(s.tcgcsv_base_url(), crate::tcgcsv::BASE_URL);
        assert_eq!(
            s.spellbook_combos_url().as_deref(),
            Some(crate::spellbook::VARIANTS_URL)
        );
        assert!(s.from_upstream());
    }

    #[test]
    fn the_upstream_combo_cadence_is_days_times_twenty_four() {
        let config = Config {
            combos_upstream_interval_days: 30,
            ..crate::test_support::test_config()
        };
        assert_eq!(
            SyncSource::from_config(&config).combos_upstream_interval_hours(),
            720
        );
        assert_eq!(
            SyncSource::new(true, "x").combos_upstream_interval_hours(),
            0
        );
    }

    #[test]
    fn combos_can_be_switched_off_in_either_mode() {
        let upstream = SyncSource::new(true, "https://tcglense.com").with_combos(false);
        assert_eq!(upstream.spellbook_combos_url(), None);
        let mirror = SyncSource::new(false, "https://tcglense.com").with_combos(false);
        assert_eq!(mirror.spellbook_combos_url(), None);
        // The switch touches nothing else.
        assert_eq!(
            mirror.tcgcsv_base_url(),
            "https://tcglense.com/api/mirror/tcgcsv"
        );
    }

    #[test]
    fn mirror_mode_points_every_dataset_at_the_mirror() {
        // A trailing slash on the base is trimmed so joins never double up.
        let s = SyncSource::new(false, "https://tcglense.com/");
        assert_eq!(
            s.scryfall_bulk_data_url(),
            "https://tcglense.com/api/mirror/scryfall/bulk-data"
        );
        assert_eq!(
            s.scryfall_sets_url(),
            "https://tcglense.com/api/mirror/scryfall/sets"
        );
        assert_eq!(
            s.scryfall_file_url("default_cards").as_deref(),
            Some("https://tcglense.com/api/mirror/scryfall/file/default_cards")
        );
        assert_eq!(
            s.mtgjson_base_url(),
            "https://tcglense.com/api/mirror/mtgjson"
        );
        assert_eq!(
            s.tcgcsv_base_url(),
            "https://tcglense.com/api/mirror/tcgcsv"
        );
        assert_eq!(
            s.spellbook_combos_url().as_deref(),
            Some("https://tcglense.com/api/mirror/spellbook/combos")
        );
        assert!(!s.from_upstream());
    }
}
