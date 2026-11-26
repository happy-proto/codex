use super::*;

const TEST_TOKEN: &str = "dummy-token-replace-me";

#[cfg(unix)]
fn reporter() -> Reporter {
    Reporter {
        token: TEST_TOKEN,
        reported: None,
        refresh: false,
    }
}

#[cfg(unix)]
fn target() -> Target {
    Target {
        thread_id: "01234567-abcd-4567-abcd-0123456789ab".to_string(),
        cwd: "/a path".to_string(),
    }
}

#[test]
fn clear_matches_specification_vector() {
    assert_eq!(
        sequence(TEST_TOKEN, 74877, 1795543200, None).as_deref(),
        Some(
            "\x1b]7577;pid=74877:time=1795543200:clear=1:mac=c30603369f98fef581a4098be889175b0c74f0641e49f8b25f4326cb38f97c93\x1b\\"
        ),
    );
}

#[test]
#[cfg(unix)]
fn arguments_and_directory_are_encoded_separately() {
    let sequence = sequence(TEST_TOKEN, 1, 2, Some(&target())).expect("valid target");
    assert!(sequence.starts_with(
        "\x1b]7577;pid=1:time=2:argv=Y29kZXg=,cmVzdW1l,MDEyMzQ1NjctYWJjZC00NTY3LWFiY2QtMDEyMzQ1Njc4OWFi:cwd=L2EgcGF0aA==:mac="
    ));
}

#[test]
#[cfg(unix)]
fn reporter_refreshes_and_clears_stale_targets() {
    let mut reporter = reporter();
    let mut written = Vec::new();
    let target = target();
    reporter.update(&mut written, Some(target.clone())).unwrap();
    let len = written.len();
    assert!(len > 0);
    reporter.update(&mut written, Some(target.clone())).unwrap();
    assert_eq!(written.len(), len);

    reporter.refresh(&mut written).unwrap();
    assert_eq!(written.len(), len * 2);

    let mut oversized = target;
    oversized.cwd.push_str(&"x".repeat(2048));
    reporter.update(&mut written, Some(oversized)).unwrap();
    assert!(
        std::str::from_utf8(&written[len * 2..])
            .unwrap()
            .contains(":clear=1:mac=")
    );
    let len = written.len();
    reporter.update(&mut written, None).unwrap();
    assert_eq!(written.len(), len);
}
