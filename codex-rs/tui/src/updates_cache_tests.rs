use super::*;
use pretty_assertions::assert_eq;

#[test]
fn same_version_fork_rebuild_is_identified_by_commit() {
    let info = VersionInfo {
        latest_version: "0.162.0-alpha.14.fork".to_string(),
        latest_commit: Some("a".repeat(40)),
        last_checked_at: DateTime::<Utc>::UNIX_EPOCH,
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
    };
    assert_eq!(info.upgrade_label("dev"), None);
}
