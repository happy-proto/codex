#![cfg(not(debug_assertions))]

use crate::legacy_core::config::Config;
use crate::updates_cache::VersionInfo;
use crate::updates_cache::read_version_info;
use crate::updates_cache::version_filepath;
use chrono::Duration;
use chrono::Utc;
use codex_build_info::BuildInfo;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_http_client::RouteAwareClientPool;
use codex_login::default_client::default_headers;
use serde::Deserialize;
use std::path::Path;

pub(crate) use crate::updates_cache::dismiss_version;

const RELEASES_URL: &str = "https://api.github.com/repos/happy-proto/codex/releases?per_page=100";

#[derive(Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    draft: bool,
}

#[derive(Deserialize)]
struct ForkRelease {
    version: String,
    source_commit: String,
}

fn upstream_alpha_version(tag: &str) -> Option<semver::Version> {
    let version = tag.strip_prefix("fork-v")?.strip_suffix(".fork")?;
    let parsed = semver::Version::parse(version).ok()?;
    parsed.pre.as_str().starts_with("alpha.").then_some(parsed)
}

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup || BuildInfo::get().is_source_build() {
        return None;
    }
    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();
    if info
        .as_ref()
        .is_none_or(|info| info.last_checked_at < Utc::now() - Duration::minutes(30))
    {
        let factory = config.http_client_factory();
        tokio::spawn(async move {
            if let Err(error) = check_for_update(&version_file, factory).await {
                tracing::error!("Failed to check fork update: {error}");
            }
        });
    }
    info.and_then(|info| info.upgrade_label(BuildInfo::get().build_commit()))
}

async fn check_for_update(version_file: &Path, factory: HttpClientFactory) -> anyhow::Result<()> {
    let client =
        RouteAwareClientPool::with_chatgpt_cloudflare_cookies(factory, ClientRouteClass::Other)
            .with_legacy_custom_ca_fallback();
    // GitHub's /releases/latest excludes prereleases, so query the alpha list.
    let releases = client
        .get(RELEASES_URL)
        .headers(default_headers())
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<ReleaseInfo>>()
        .await?;
    let tag = releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let parsed = upstream_alpha_version(&release.tag_name)?;
            Some((parsed, release.tag_name))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, tag)| tag)
        .ok_or_else(|| anyhow::anyhow!("No published fork alpha release"))?;
    let url =
        format!("https://github.com/happy-proto/codex/releases/download/{tag}/fork-release.json");
    let latest = client
        .get(&url)
        .headers(default_headers())
        .send()
        .await?
        .error_for_status()?
        .json::<ForkRelease>()
        .await?;
    anyhow::ensure!(
        format!("fork-v{}", latest.version) == tag,
        "Fork manifest version mismatch"
    );
    anyhow::ensure!(
        latest.source_commit.len() == 40
            && latest
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "Invalid fork source commit"
    );
    let previous = read_version_info(version_file).ok();
    let info = VersionInfo {
        latest_version: latest.version,
        latest_commit: Some(latest.source_commit),
        last_checked_at: Utc::now(),
        dismissed_version: previous.and_then(|info| info.dismissed_version),
    };
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, serde_json::to_vec(&info)?).await?;
    Ok(())
}

pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    let latest = get_upgrade_version(config)?;
    if read_version_info(&version_filepath(config))
        .ok()
        .is_some_and(|info| info.dismissed_version.as_deref() == Some(latest.as_str()))
    {
        return None;
    }
    Some(latest)
}

#[cfg(test)]
mod tests {
    use super::upstream_alpha_version;

    #[test]
    fn alpha_hotfix_sorts_after_the_base_alpha_without_fork_marker() {
        let base = upstream_alpha_version("fork-v0.162.0-alpha.14.fork").unwrap();
        let hotfix = upstream_alpha_version("fork-v0.162.0-alpha.14.2.fork").unwrap();
        assert!(hotfix > base);
    }

    #[test]
    fn only_fork_alpha_tags_participate_in_update_selection() {
        for tag in [
            "rust-v0.162.0-alpha.14",
            "fork-v0.162.0.fork",
            "fork-v0.162.0-alpha.14",
        ] {
            assert!(upstream_alpha_version(tag).is_none());
        }
    }
}
