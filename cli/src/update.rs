//! Self-update from GitHub Releases, plus a passive "a new version is available"
//! check.
//!
//! `tcglense update` downloads the release asset matching this platform from the
//! `tcglense-cli` repo and replaces the running binary (via `self_update`, which is
//! blocking — so it runs on a blocking thread). A best-effort, throttled background
//! check on ordinary commands prints a one-line upgrade notice to stderr.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

/// The GitHub repository releases are published to.
pub const REPO_OWNER: &str = "PNRxA";
pub const REPO_NAME: &str = "tcglense-cli";
/// The binary name inside each release archive.
pub const BIN_NAME: &str = "tcglense";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often the passive check hits the network (once/day).
const CHECK_INTERVAL_SECS: u64 = 60 * 60 * 24;

/// Outcome of an explicit `tcglense update` run.
pub enum Outcome {
    UpToDate { current: String },
    Available { current: String, latest: String },
    Updated { version: String },
    Cancelled,
    NoAssetForPlatform { target: String, latest: String },
}

/// Run (or, with `check_only`, just report) a self-update. The blocking
/// `self_update` work runs on a dedicated thread so it never touches the async
/// runtime.
pub async fn run(check_only: bool, yes: bool, quiet: bool) -> Result<Outcome> {
    tokio::task::spawn_blocking(move || blocking_update(check_only, yes, quiet))
        .await
        .map_err(|e| anyhow!("update task panicked: {e}"))?
}

fn blocking_update(check_only: bool, yes: bool, quiet: bool) -> Result<Outcome> {
    use std::io::Write;

    let updater = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(CURRENT_VERSION)
        .no_confirm(true) // confirmation is handled below
        .show_download_progress(!quiet)
        .build()
        .map_err(|e| anyhow!("could not configure the updater: {e}"))?;

    let latest = updater.get_latest_release().map_err(|e| {
        anyhow!("could not fetch the latest release from {REPO_OWNER}/{REPO_NAME}: {e}")
    })?;

    let newer = self_update::version::bump_is_greater(CURRENT_VERSION, &latest.version)
        .unwrap_or(latest.version != CURRENT_VERSION);
    if !newer {
        return Ok(Outcome::UpToDate {
            current: CURRENT_VERSION.to_string(),
        });
    }
    if check_only {
        return Ok(Outcome::Available {
            current: CURRENT_VERSION.to_string(),
            latest: latest.version,
        });
    }

    let target = self_update::get_target();
    if !latest.has_target_asset(target) {
        return Ok(Outcome::NoAssetForPlatform {
            target: target.to_string(),
            latest: latest.version,
        });
    }

    if !yes {
        print!(
            "Update tcglense {CURRENT_VERSION} → {}? [Y/n] ",
            latest.version
        );
        std::io::stdout().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).ok();
        let answer = answer.trim();
        if !(answer.is_empty()
            || answer.eq_ignore_ascii_case("y")
            || answer.eq_ignore_ascii_case("yes"))
        {
            return Ok(Outcome::Cancelled);
        }
    }

    let status = updater
        .update()
        .map_err(|e| anyhow!("update failed: {e}"))?;
    Ok(Outcome::Updated {
        version: status.version().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Passive "new version available" check.
// ---------------------------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
struct CheckCache {
    last_check: u64,
    latest_version: Option<String>,
}

/// Best-effort, side-effect-free notice: never blocks meaningfully, never errors
/// into the foreground, and stays silent unless stderr is a terminal (so piped or
/// scripted output is untouched). Opt out with `TCGLENSE_NO_UPDATE_CHECK`.
pub async fn maybe_notify() {
    if std::env::var_os("TCGLENSE_NO_UPDATE_CHECK").is_some() {
        return;
    }
    if !std::io::stderr().is_terminal() {
        return;
    }
    let _ = notify_inner().await;
}

async fn notify_inner() -> Result<()> {
    let path = cache_path();
    let now = now_secs();
    let cache = path.as_ref().map(read_cache).unwrap_or_default();

    let latest = if now.saturating_sub(cache.last_check) < CHECK_INTERVAL_SECS {
        // Fresh enough — decide from the cache, no network.
        cache.latest_version.clone()
    } else {
        // Stale/absent — fetch with a hard cap, then record the result (even a
        // failed fetch updates last_check so we don't retry for a day).
        let fetched = tokio::time::timeout(Duration::from_secs(3), fetch_latest_tag())
            .await
            .ok()
            .and_then(|r| r.ok());
        let updated = CheckCache {
            last_check: now,
            latest_version: fetched.clone().or(cache.latest_version.clone()),
        };
        if let Some(path) = &path {
            let _ = write_cache(path, &updated);
        }
        updated.latest_version
    };

    if let Some(latest) = latest
        && self_update::version::bump_is_greater(CURRENT_VERSION, &latest).unwrap_or(false)
    {
        eprintln!("\nA new tcglense release is available: {CURRENT_VERSION} → {latest}");
        eprintln!(
            "  Run `tcglense update` to upgrade (set TCGLENSE_NO_UPDATE_CHECK=1 to silence)."
        );
    }
    Ok(())
}

async fn fetch_latest_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let client = reqwest::Client::builder()
        .user_agent(concat!("tcglense-cli/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("github returned {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await?;
    let tag = body
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("release has no tag_name"))?;
    Ok(tag.trim_start_matches('v').to_string())
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("tcglense").join("update-check.json"))
}

fn read_cache(path: &PathBuf) -> CheckCache {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_cache(path: &PathBuf, cache: &CheckCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(cache)?)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
