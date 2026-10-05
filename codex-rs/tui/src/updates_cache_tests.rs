use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[test]
fn same_version_fork_rebuild_is_identified_by_commit() {
    let info = VersionInfo {
        latest_version: "0.162.0-alpha.14.fork".to_string(),
        latest_commit: Some("a".repeat(40)),
        last_checked_at: DateTime::<Utc>::UNIX_EPOCH,
        dismissed_version: None,
    };
    assert_eq!(info.upgrade_label(&"a".repeat(40)), None);
    assert_eq!(
        info.upgrade_label(&"b".repeat(40)),
        Some("0.162.0-alpha.14.fork (aaaaaaaa)".to_string())
    );
}

#[test]
fn malformed_cached_commit_does_not_offer_an_update() {
    let info = VersionInfo {
        latest_version: "0.162.0-alpha.14.fork".to_string(),
        latest_commit: Some("é".repeat(20)),
        last_checked_at: DateTime::<Utc>::UNIX_EPOCH,
        dismissed_version: None,
    };
    assert_eq!(info.upgrade_label("dev"), None);
}

#[tokio::test]
async fn dismiss_version_creates_cache_file_when_missing() {
    let codex_home = tempdir().expect("temp codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    let version_file = version_filepath(&config);

    dismiss_version(&config, "999.0.0")
        .await
        .expect("dismiss version");

    let info = read_version_info(&version_file).expect("read version info");
    assert_eq!(info.last_checked_at, DateTime::<Utc>::UNIX_EPOCH);
    assert_eq!(
        (
            info.latest_version.as_str(),
            info.dismissed_version.as_deref()
        ),
        ("999.0.0", Some("999.0.0"))
    );
}
