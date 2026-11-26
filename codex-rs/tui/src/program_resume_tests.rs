use super::*;

const TEST_TOKEN: &str = "dummy-token-replace-me";

#[cfg(unix)]
fn reporter() -> Reporter {
    Reporter {
        token: TEST_TOKEN,
        flags: Vec::new(),
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
        sequence(TEST_TOKEN, 74877, 1795543200, None, &[]).as_deref(),
        Some(
            "\x1b]7577;pid=74877:time=1795543200:clear=1:mac=c30603369f98fef581a4098be889175b0c74f0641e49f8b25f4326cb38f97c93\x1b\\"
        ),
    );
}

#[test]
#[cfg(unix)]
fn arguments_and_directory_are_encoded_separately() {
    let sequence = sequence(TEST_TOKEN, 1, 2, Some(&target()), &[]).expect("valid target");
    assert!(sequence.starts_with(
        "\x1b]7577;pid=1:time=2:argv=Y29kZXg=,cmVzdW1l,MDEyMzQ1NjctYWJjZC00NTY3LWFiY2QtMDEyMzQ1Njc4OWFi:cwd=L2EgcGF0aA==:mac="
    ));
}

#[test]
#[cfg(unix)]
fn supported_launch_flags_are_separate_resume_arguments() {
    let flags = resume_flags(
        [
            "-m",
            "model name",
            "--no-alt-screen",
            "resume",
            "01234567-abcd-4567-abcd-0123456789ab",
            "--dangerously-bypass-approvals-and-sandbox",
            "--",
            "--no-alt-screen",
        ]
        .map(str::to_string),
        true,
        true,
        Some("model name"),
    )
    .expect("launch flags can be reproduced");
    let sequence = sequence(TEST_TOKEN, 1, 2, Some(&target()), &flags).expect("valid target");
    let argv = sequence
        .split_once(":argv=")
        .unwrap()
        .1
        .split_once(":cwd=")
        .unwrap()
        .0;
    let args = argv
        .split(',')
        .map(|arg| String::from_utf8(STANDARD.decode(arg).unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            "codex",
            "resume",
            "01234567-abcd-4567-abcd-0123456789ab",
            "-m",
            "model name",
            "--no-alt-screen",
            "--dangerously-bypass-approvals-and-sandbox",
        ],
    );
}

#[test]
fn reporter_requires_the_parsed_launch_flags_to_be_reproducible() {
    assert_eq!(
        resume_flags(
            ["--".to_string(), "--no-alt-screen".to_string()],
            false,
            true,
            None
        ),
        None
    );
    assert_eq!(
        resume_flags(
            ["--model=other".to_string()],
            false,
            false,
            Some("selected")
        ),
        None
    );
    assert_eq!(
        resume_flags(
            ["--model=selected".to_string()],
            false,
            false,
            Some("selected")
        ),
        Some(vec!["--model=selected".to_string()])
    );
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
