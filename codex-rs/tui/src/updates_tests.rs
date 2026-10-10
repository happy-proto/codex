use super::*;
use codex_http_client::OutboundProxyPolicy;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn latest_info(commit: &str) -> VersionInfo {
    VersionInfo {
        latest_version: "0.162.0-alpha.20.fork".to_string(),
        latest_commit: Some(commit.to_string()),
        last_checked_at: Utc::now(),
    }
}

fn warning_text(event: AppEvent) -> String {
    let AppEvent::InsertHistoryCell(cell) = event else {
        panic!("expected a non-modal history warning");
    };
    assert!(cell.as_any().is::<UpdateAvailableHistoryCell>());
    cell.raw_lines()
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

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

#[test]
fn environment_tokens_prefer_gh_and_skip_empty_values() {
    assert_eq!(
        token_from_environment(Some(" gh-token \n".into()), Some("github-token".into())),
        Some("gh-token".into())
    );
    assert_eq!(
        token_from_environment(Some(" \n".into()), Some("github-token".into())),
        Some("github-token".into())
    );
    assert_eq!(token_from_environment(None, Some("".into())), None);
}

#[tokio::test]
async fn slow_refresh_returns_immediately_and_notifies_once_after_completion() {
    let home = tempdir().unwrap();
    let (events, mut receiver) = mpsc::unbounded_channel();
    let (started, started_rx) = oneshot::channel();
    let (finish, finish_rx) = oneshot::channel();
    let task = spawn_update_check(
        home.path().join("fork-version.json"),
        "b".repeat(40),
        AppEventSender::new(events),
        async move {
            started.send(()).unwrap();
            finish_rx.await.unwrap();
            Ok(latest_info(&"a".repeat(40)))
        },
    );
    started_rx.await.unwrap();
    assert!(!task.is_finished());
    assert!(receiver.try_recv().is_err());
    finish.send(()).unwrap();
    task.await.unwrap();
    let message = warning_text(receiver.try_recv().unwrap());
    assert!(message.contains("0.162.0-alpha.20.fork (aaaaaaaa)"));
    assert!(message.contains("Run codex update"));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn fresh_cache_avoids_network_and_suppresses_the_current_build() {
    let home = tempdir().unwrap();
    let version_file = home.path().join("fork-version.json");
    let commit = "a".repeat(40);
    tokio::fs::write(
        &version_file,
        serde_json::to_vec(&latest_info(&commit)).unwrap(),
    )
    .await
    .unwrap();
    for (current, expected_warning) in [(&commit, false), (&"b".repeat(40), true)] {
        let (events, mut receiver) = mpsc::unbounded_channel();
        spawn_update_check(
            version_file.clone(),
            current.to_string(),
            AppEventSender::new(events),
            async { panic!("fresh cache must not look up a token or query the network") },
        )
        .await
        .unwrap();
        assert_eq!(receiver.try_recv().is_ok(), expected_warning);
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn failed_refresh_is_silent_when_no_update_is_known() {
    let home = tempdir().unwrap();
    let (events, mut receiver) = mpsc::unbounded_channel();
    spawn_update_check(
        home.path().join("fork-version.json"),
        "b".repeat(40),
        AppEventSender::new(events),
        async { anyhow::bail!("unavailable") },
    )
    .await
    .unwrap();
    assert!(receiver.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn timed_out_refresh_uses_the_cache_without_blocking_the_app() {
    let home = tempdir().unwrap();
    let version_file = home.path().join("fork-version.json");
    let mut cached = latest_info(&"a".repeat(40));
    cached.last_checked_at = Utc::now() - ChronoDuration::hours(1);
    tokio::fs::write(&version_file, serde_json::to_vec(&cached).unwrap())
        .await
        .unwrap();
    let (events, mut receiver) = mpsc::unbounded_channel();
    let (started, started_rx) = oneshot::channel();
    let task = spawn_update_check(
        version_file,
        "b".repeat(40),
        AppEventSender::new(events),
        async move {
            started.send(()).unwrap();
            std::future::pending().await
        },
    );
    started_rx.await.unwrap();
    assert!(!task.is_finished());
    assert!(receiver.try_recv().is_err());
    tokio::time::advance(UPDATE_CHECK_TIMEOUT).await;
    task.await.unwrap();
    assert!(warning_text(receiver.try_recv().unwrap()).contains("aaaaaaaa"));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn release_lookup_scopes_tokens_to_the_api_and_supports_anonymous_fallback() {
    for token in [Some("test-gh-token"), None] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"tag_name": "fork-v0.162.0-alpha.18.1.fork", "draft": false},
                {"tag_name": "fork-v0.162.0-alpha.20.fork", "draft": false},
                {"tag_name": "fork-v0.162.0-alpha.21.fork", "draft": true},
            ])))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/download/fork-v0.162.0-alpha.20.fork/fork-release.json",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "version": "0.162.0-alpha.20.fork",
                "source_commit": "a".repeat(40),
            })))
            .expect(1)
            .mount(&server)
            .await;
        let release = fetch_latest_release(
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            &format!("{}/api/releases?per_page=100", server.uri()),
            &format!("{}/download", server.uri()),
            token,
        )
        .await
        .unwrap();
        assert_eq!(release.source_commit, "a".repeat(40));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .map(|value| value.to_str().unwrap()),
            token.map(|_| "Bearer test-gh-token")
        );
        assert!(requests[1].headers.get("authorization").is_none());
    }
}

#[tokio::test]
async fn authenticated_api_does_not_follow_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/releases"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/leak", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(
        fetch_latest_release(
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            &format!("{}/api/releases", server.uri()),
            &format!("{}/download", server.uri()),
            Some("test-gh-token"),
        )
        .await
        .is_err()
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/api/releases");
}
