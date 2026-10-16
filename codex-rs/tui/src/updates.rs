#![cfg(not(debug_assertions))]

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell::UpdateAvailableHistoryCell;
use crate::legacy_core::config::Config;
use crate::updates_cache::VersionInfo;
use crate::updates_cache::version_filepath;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use codex_build_info::BuildInfo;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_http_client::RouteAwareClientPool;
use codex_login::default_client::default_headers;
use serde::Deserialize;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

const RELEASES_URL: &str = "https://api.github.com/repos/happy-proto/codex/releases?per_page=100";
const DOWNLOADS_URL: &str = "https://github.com/happy-proto/codex/releases/download";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(2);

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

fn upstream_alpha_version(tag: &str) -> Option<(semver::Version, u64)> {
    let version = tag.strip_prefix("fork-v")?;
    let (upstream, revision) = if let Some((upstream, revision)) = version.rsplit_once(".fork.") {
        let number = revision.parse::<u64>().ok()?;
        if number == 0 || number.to_string() != revision {
            return None;
        }
        (upstream, number)
    } else {
        (version.strip_suffix(".fork")?, 0)
    };
    let parsed = semver::Version::parse(upstream).ok()?;
    let components: Vec<_> = parsed.pre.as_str().split('.').collect();
    (parsed.build.is_empty()
        && (2..=3).contains(&components.len())
        && components[0] == "alpha"
        && components[1..]
            .iter()
            .all(|part| part.parse::<u64>().is_ok()))
    .then_some((parsed, revision))
}

pub(crate) fn start_update_check(config: &Config, events: AppEventSender) {
    if !config.check_for_update_on_startup || BuildInfo::get().is_source_build() {
        return;
    }
    let version_file = version_filepath(config);
    let refresh_file = version_file.clone();
    let factory = config.http_client_factory();
    spawn_update_check(
        version_file,
        BuildInfo::get().build_commit().to_string(),
        events,
        async move { check_for_update(&refresh_file, factory).await },
    );
}

fn spawn_update_check(
    version_file: PathBuf,
    current_commit: String,
    events: AppEventSender,
    refresh: impl Future<Output = anyhow::Result<VersionInfo>> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut info = tokio::fs::read(&version_file)
            .await
            .ok()
            .and_then(|contents| serde_json::from_slice::<VersionInfo>(&contents).ok());
        if info
            .as_ref()
            .is_none_or(|info| info.last_checked_at < Utc::now() - ChronoDuration::minutes(30))
        {
            match tokio::time::timeout(UPDATE_CHECK_TIMEOUT, refresh).await {
                Ok(Ok(latest)) => info = Some(latest),
                Ok(Err(error)) => tracing::debug!("Failed to check fork update: {error}"),
                Err(_) => tracing::debug!("Fork update check timed out"),
            }
        }
        if let Some(latest) = info.and_then(|info| info.upgrade_label(&current_commit)) {
            events.send(AppEvent::InsertHistoryCell(Box::new(
                UpdateAvailableHistoryCell::new(latest),
            )));
        }
    })
}

async fn check_for_update(
    version_file: &Path,
    factory: HttpClientFactory,
) -> anyhow::Result<VersionInfo> {
    let token = github_token().await;
    let latest =
        fetch_latest_release(factory, RELEASES_URL, DOWNLOADS_URL, token.as_deref()).await?;
    let info = VersionInfo {
        latest_version: latest.version,
        latest_commit: Some(latest.source_commit),
        last_checked_at: Utc::now(),
    };
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, serde_json::to_vec(&info)?).await?;
    Ok(info)
}

fn token_from_environment(
    gh_token: Option<String>,
    github_token: Option<String>,
) -> Option<String> {
    gh_token
        .into_iter()
        .chain(github_token)
        .map(|token| token.trim().to_string())
        .find(|token| !token.is_empty())
}

async fn github_token() -> Option<String> {
    if let Some(token) = token_from_environment(
        std::env::var("GH_TOKEN").ok(),
        std::env::var("GITHUB_TOKEN").ok(),
    ) {
        return Some(token);
    }
    let output = tokio::time::timeout(
        TOKEN_TIMEOUT,
        Command::new("gh")
            .args(["auth", "token", "--hostname", "github.com"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    token_from_environment(String::from_utf8(output.stdout).ok(), None)
}

async fn fetch_latest_release(
    factory: HttpClientFactory,
    releases_url: &str,
    downloads_url: &str,
    token: Option<&str>,
) -> anyhow::Result<ForkRelease> {
    // Keep API credentials on the original host and out of request diagnostics.
    let api = RouteAwareClientPool::new_without_redirects_or_request_logging(
        factory.clone(),
        ClientRouteClass::Other,
    )
    .with_legacy_custom_ca_fallback();
    let mut request = api.get(releases_url).headers(default_headers());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    // GitHub's /releases/latest excludes prereleases, so query the alpha list.
    let releases = request
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
    let client = RouteAwareClientPool::new(factory, ClientRouteClass::Other)
        .with_legacy_custom_ca_fallback();
    let url = format!("{downloads_url}/{tag}/fork-release.json");
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
    Ok(latest)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
