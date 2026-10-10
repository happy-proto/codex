use crate::legacy_core::config::Config;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct VersionInfo {
    pub(crate) latest_version: String,
    #[serde(default)]
    pub(crate) latest_commit: Option<String>,
    // ISO-8601 timestamp (RFC3339)
    pub(crate) last_checked_at: DateTime<Utc>,
}

const VERSION_FILENAME: &str = "fork-version.json";

impl VersionInfo {
    pub(crate) fn upgrade_label(&self, current_commit: &str) -> Option<String> {
        let commit = self.latest_commit.as_deref()?;
        (commit.len() == 40
            && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            && commit != current_commit)
            .then(|| format!("{} ({})", self.latest_version, &commit[..8]))
    }
}

pub(crate) fn version_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(VERSION_FILENAME).into_path_buf()
}

#[cfg(test)]
#[path = "updates_cache_tests.rs"]
mod tests;
