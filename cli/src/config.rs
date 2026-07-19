//! Persistent CLI configuration: the API base URL and stored credentials.
//!
//! Lives at `$TCGLENSE_CONFIG`, else `<config-dir>/tcglense/config.json`
//! (`~/.config/tcglense/config.json` on Linux). The file may hold a long-lived
//! credential (a session refresh token or an API key), so it is written `0600`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::User;

pub const DEFAULT_BASE_URL: &str = "http://localhost:8080";

/// Stored credential. `Session` is the web/email auth path (email + password →
/// a short-lived access token plus the opaque refresh token captured from the
/// `tcglense_refresh` cookie); `ApiKey` is a `tcgl_` programmatic key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Auth {
    ApiKey {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
    Session {
        access_token: String,
        refresh_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<User>,
    },
}

impl Auth {
    /// The bearer credential to present as `Authorization: Bearer <token>`.
    pub fn bearer(&self) -> &str {
        match self {
            Auth::ApiKey { key, .. } => key,
            Auth::Session { access_token, .. } => access_token,
        }
    }

    /// A short human label for `status`/`whoami` output.
    pub fn describe(&self) -> String {
        match self {
            Auth::ApiKey { key, scope } => {
                let prefix: String = key.chars().take(13).collect();
                match scope {
                    Some(s) => format!("API key {prefix}… ({s})"),
                    None => format!("API key {prefix}…"),
                }
            }
            Auth::Session { user, .. } => match user {
                Some(u) => format!("session as {}", u.email),
                None => "session".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
}

impl Config {
    /// Resolve the config file path from an explicit override, `$TCGLENSE_CONFIG`,
    /// or the platform config directory.
    pub fn resolve_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
        if let Some(p) = explicit {
            return Ok(p);
        }
        if let Ok(env) = std::env::var("TCGLENSE_CONFIG")
            && !env.is_empty()
        {
            return Ok(PathBuf::from(env));
        }
        let dir = dirs::config_dir()
            .context("could not determine a config directory; set TCGLENSE_CONFIG")?;
        Ok(dir.join("tcglense").join("config.json"))
    }

    /// Load the config from `path`, returning defaults when the file is absent.
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let cfg = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parsing config at {}", path.display()))?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading config at {}", path.display())),
        }
    }

    /// Persist the config to `path` (creating parent dirs), restricting the file to
    /// the owner on Unix since it may hold a credential.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating config dir {}", parent.display()))?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, &json)
            .with_context(|| format!("writing config at {}", path.display()))?;
        restrict_permissions(path);
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_json() {
        let cfg = Config {
            base_url: Some("https://example.com".into()),
            auth: Some(Auth::ApiKey {
                key: "tcgl_abc123".into(),
                scope: Some("read_write".into()),
            }),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.base_url.as_deref(), Some("https://example.com"));
        match back.auth {
            Some(Auth::ApiKey { key, scope }) => {
                assert_eq!(key, "tcgl_abc123");
                assert_eq!(scope.as_deref(), Some("read_write"));
            }
            _ => panic!("expected an ApiKey credential"),
        }
    }

    #[test]
    fn session_auth_serializes_with_kind_tag() {
        let auth = Auth::Session {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            user: None,
        };
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"kind\":\"session\""), "got {json}");
        assert_eq!(auth.bearer(), "at");
    }

    #[test]
    fn empty_config_defaults_when_file_absent() {
        let path = std::path::Path::new("/nonexistent/tcglense/does-not-exist.json");
        let cfg = Config::load(path).unwrap();
        assert!(cfg.base_url.is_none());
        assert!(cfg.auth.is_none());
    }
}
