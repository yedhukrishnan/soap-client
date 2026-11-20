//! Checking whether a newer release exists. The latest version is published as a small
//! static JSON file next to the website, so no API (or rate limit) is involved:
//!
//! ```json
//! { "version": "0.2.2", "securityReleases": ["0.2.1", "0.2.2"] }
//! ```
//!
//! `securityReleases` lists every release that fixed a security issue, so someone several
//! versions behind is told when any release they missed was a security fix.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::project::write_private;
use crate::{Error, Result};

pub const MANIFEST_URL: &str = "https://yedhu.me/soap-client/latest.json";
/// Automatic checks happen at most this often.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub security_releases: Vec<String>,
}

/// A newer release than the running one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// A release newer than the running one fixed a security issue.
    pub security: bool,
}

/// `MAJOR.MINOR.PATCH`, optionally with a leading `v`. Anything else (including
/// pre-releases) is not a version this checker understands.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().strip_prefix('v').unwrap_or(v.trim()).split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The update to offer, if the manifest names a newer version than `current`.
pub fn available(current: &str, manifest: &Manifest) -> Option<UpdateInfo> {
    let current = parse_version(current)?;
    let latest = parse_version(&manifest.version)?;
    if latest <= current {
        return None;
    }
    let security = manifest
        .security_releases
        .iter()
        .filter_map(|v| parse_version(v))
        .any(|v| v > current && v <= latest);
    Some(UpdateInfo { version: manifest.version.trim().trim_start_matches('v').to_string(), security })
}

pub async fn fetch_manifest(url: &str) -> Result<Manifest> {
    let (text, _) = crate::http::fetch_text(url, false, MAX_MANIFEST_BYTES).await?;
    serde_json::from_str(&text).map_err(|e| Error::Http(format!("unexpected update information from {url}: {e}")))
}

/// Download the manifest and compare it with the running version.
pub async fn check(url: &str, current: &str) -> Result<Option<UpdateInfo>> {
    Ok(available(current, &fetch_manifest(url).await?))
}

/// Update-check preferences, kept in their own small file in the app data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrefs {
    #[serde(default = "yes")]
    pub auto_check: bool,
    /// Unix time of the last successful automatic check.
    #[serde(default)]
    pub last_check: Option<u64>,
    /// A version the user chose to skip. Never applies to security releases.
    #[serde(default)]
    pub skipped_version: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        UpdatePrefs { auto_check: true, last_check: None, skipped_version: None }
    }
}

impl UpdatePrefs {
    /// Missing or unreadable preferences fall back to the defaults.
    pub fn load(path: &Path) -> UpdatePrefs {
        std::fs::read_to_string(path).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_private(path, &serde_json::to_string(self).map_err(|e| Error::Io(e.to_string()))?)
    }

    /// Whether an automatic check should run now. A last check in the future (clock
    /// changed) counts as due, so a wrong clock can't stop checks for good.
    pub fn due(&self, now: u64) -> bool {
        self.auto_check && self.last_check.is_none_or(|t| t > now || now - t >= CHECK_INTERVAL_SECS)
    }

    /// What an automatic check shows: nothing for a version the user skipped, unless
    /// it fixes a security issue.
    pub fn filter(&self, update: Option<UpdateInfo>) -> Option<UpdateInfo> {
        update.filter(|u| u.security || self.skipped_version.as_deref() != Some(u.version.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, security: &[&str]) -> Manifest {
        Manifest { version: version.into(), security_releases: security.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn parses_plain_versions_only() {
        assert_eq!(parse_version("0.2.10"), Some((0, 2, 10)));
        assert_eq!(parse_version("v1.0.0"), Some((1, 0, 0)));
        for bad in ["", "1.0", "1.0.0.0", "1.0.0-beta", "a.b.c", "1..0"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn offers_only_newer_versions() {
        assert_eq!(available("0.2.2", &manifest("0.2.2", &[])), None);
        assert_eq!(available("0.3.0", &manifest("0.2.2", &[])), None);
        assert_eq!(available("0.2.9", &manifest("0.2.10", &[])).unwrap().version, "0.2.10");
        assert_eq!(available("0.2.0", &manifest("v0.2.2", &[])).unwrap().version, "0.2.2");
        assert_eq!(available("0.2.0", &manifest("garbage", &[])), None);
    }

    #[test]
    fn security_counts_any_missed_security_release() {
        let m = manifest("0.2.3", &["0.2.1"]);
        assert!(available("0.2.0", &m).unwrap().security);
        assert!(!available("0.2.1", &m).unwrap().security);
        assert!(!available("0.2.2", &m).unwrap().security);
        // A listed version beyond the latest one doesn't count.
        assert!(!available("0.2.2", &manifest("0.2.3", &["0.2.4"])).unwrap().security);
    }

    #[test]
    fn automatic_checks_are_daily_and_can_be_turned_off() {
        let mut p = UpdatePrefs::default();
        assert!(p.due(1000));
        p.last_check = Some(1000);
        assert!(!p.due(1000 + CHECK_INTERVAL_SECS - 1));
        assert!(p.due(1000 + CHECK_INTERVAL_SECS));
        assert!(p.due(10), "a last check in the future counts as due");
        p.auto_check = false;
        assert!(!p.due(1000 + 10 * CHECK_INTERVAL_SECS));
    }

    #[test]
    fn skipping_never_hides_security_releases() {
        let p = UpdatePrefs { skipped_version: Some("0.2.3".into()), ..Default::default() };
        let plain = UpdateInfo { version: "0.2.3".into(), security: false };
        let security = UpdateInfo { version: "0.2.3".into(), security: true };
        assert_eq!(p.filter(Some(plain.clone())), None);
        assert_eq!(p.filter(Some(security.clone())), Some(security));
        let newer = UpdateInfo { version: "0.2.4".into(), ..plain };
        assert_eq!(p.filter(Some(newer.clone())), Some(newer));
    }

    #[test]
    fn prefs_round_trip_and_tolerate_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.json");
        assert_eq!(UpdatePrefs::load(&path), UpdatePrefs::default());
        let p = UpdatePrefs { auto_check: false, last_check: Some(5), skipped_version: Some("1.0.0".into()) };
        p.save(&path).unwrap();
        assert_eq!(UpdatePrefs::load(&path), p);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(UpdatePrefs::load(&path), UpdatePrefs::default());
    }

    #[tokio::test]
    async fn checks_against_a_published_manifest() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":"0.3.0","securityReleases":["0.2.1"]}"#))
            .mount(&server)
            .await;
        Mock::given(path("/broken.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not found</html>"))
            .mount(&server)
            .await;
        let url = format!("{}/latest.json", server.uri());
        assert_eq!(check(&url, "0.2.0").await.unwrap(), Some(UpdateInfo { version: "0.3.0".into(), security: true }));
        assert_eq!(check(&url, "0.3.0").await.unwrap(), None);
        assert!(check(&format!("{}/broken.json", server.uri()), "0.2.0").await.is_err());
        assert!(check(&format!("{}/missing.json", server.uri()), "0.2.0").await.is_err());
    }
}
