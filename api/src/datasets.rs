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

/// Resolves each provider's dataset base URL to either its real upstream or a mirror.
///
/// Cheap to clone (one `bool` + one short `String`). Constructed once per sync from
/// [`Config`] and passed down into the Scryfall / MTGJSON / TCGCSV refresh paths.
#[derive(Clone, Debug)]
pub struct SyncSource {
    from_upstream: bool,
    /// Mirror origin with any trailing slash trimmed, so URL joins never double up.
    mirror_base: String,
}

impl SyncSource {
    /// Build from application config.
    pub fn from_config(config: &Config) -> Self {
        Self::new(config.sync_from_upstream, config.dataset_mirror_url.clone())
    }

    /// Construct directly (used by tests). Trims a trailing slash off `mirror_base`
    /// defensively so callers needn't (`Config` already trims its stored value).
    pub fn new(from_upstream: bool, mirror_base: impl Into<String>) -> Self {
        Self {
            from_upstream,
            mirror_base: mirror_base.into().trim_end_matches('/').to_string(),
        }
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
    }
}
