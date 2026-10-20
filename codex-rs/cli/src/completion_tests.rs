use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn thread(id: &str, label: &str, unique_label: bool) -> ThreadCompletion {
    ThreadCompletion {
        id: id.into(),
        label: label.into(),
        cwd: PathBuf::from("/project"),
        unique_label,
    }
}

#[test]
fn session_context_respects_command_options() {
    let context = SessionContext::parse(&args(&[
        "codex",
        "resume",
        "--all",
        "--include-non-interactive",
    ]))
    .expect("resume context");
    assert_eq!(context.subcommand, "resume");
    assert!(context.cwd.is_none());
    assert!(context.include_non_interactive);
    let context = SessionContext::parse(&args(&["codex", "fork", "--all"])).expect("fork context");
    assert_eq!(context.subcommand, "fork");
    assert!(!context.include_non_interactive);
}

#[test]
fn prompt_and_remote_contexts_do_not_offer_local_sessions() {
    for values in [
        vec!["codex", "resume", "--last"],
        vec!["codex", "fork", "--last"],
        vec!["codex", "resume", "session-id"],
        vec!["codex", "resume", "--remote", "ws://example:1234"],
        vec!["codex", "--remote", "ws://example:1234", "resume"],
        vec!["codex", "exec", "resume"],
    ] {
        assert!(
            SessionContext::parse(&args(&values)).is_none(),
            "{values:?}"
        );
    }
}

#[test]
fn command_attaches_session_completer_to_first_positional() {
    for subcommand in ["resume", "fork"] {
        let mut command = command(
            &args(&["codex", "--", "codex", subcommand, "--all", "修复"]),
            Some(3),
            OsStr::new("fish"),
        );
        command.build();
        let command = command.find_subcommand(subcommand).expect("subcommand");
        let arg = command
            .get_positionals()
            .find(|arg| arg.get_index() == Some(1))
            .expect("first positional");
        assert_eq!(arg.get_id(), "session_id");
        assert!(arg.get::<ArgValueCompleter>().is_some());
        let prompt = command
            .get_positionals()
            .find(|arg| arg.get_index() == Some(2))
            .expect("prompt");
        assert_eq!(prompt.get_id(), "prompt");
        assert!(prompt.get::<ArgValueCompleter>().is_none());
    }
}

#[test]
fn command_checks_remote_options_after_cursor() {
    let command = command(
        &args(&[
            "codex",
            "--",
            "codex",
            "resume",
            "",
            "--remote",
            "ws://example:1234",
        ]),
        Some(2),
        OsStr::new("zsh"),
    );
    let arg = command
        .find_subcommand("resume")
        .expect("resume")
        .get_arguments()
        .find(|arg| arg.get_id() == "session_id")
        .expect("session ID");
    assert!(arg.get::<ArgValueCompleter>().is_none());
}

#[test]
fn uuid_candidates_keep_recency_order_and_clean_descriptions() {
    let candidates = candidates(
        vec![
            thread("02", "修复\n补全\t", false),
            thread("01", "older", true),
        ],
        "",
        false,
    );
    assert_eq!(candidates[0].get_value(), "02");
    assert_eq!(candidates[1].get_value(), "01");
    assert_eq!(candidates[0].get_display_order(), Some(0));
    let help = candidates[0].get_help().expect("description").to_string();
    assert!(!help.chars().any(char::is_control));
}

#[test]
fn zsh_name_prefixes_insert_unique_exact_labels() {
    let results = candidates(vec![thread("01", "修复 shell 补全", true)], "修复", true);
    assert_eq!(results[0].get_value(), "修复 shell 补全");
    assert!(candidates(vec![thread("01", "修复", false)], "修复", true).is_empty());
    assert!(candidates(vec![thread("01", "修复\n补全", true)], "修复", true).is_empty());
    assert_eq!(
        candidates(vec![thread("01", "修复", false)], "0", true)[0].get_value(),
        "01"
    );
    assert_eq!(
        candidates(vec![thread("01", "修复", false)], "修复", false)[0].get_value(),
        "01"
    );
}

#[test]
fn storage_configuration_is_read_relative_to_its_file() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    assert!(configured_sqlite_home(&path)?.is_none());
    assert!(!path.exists());
    std::fs::write(&path, "sqlite_home = 'state'\n")?;
    assert_eq!(
        configured_sqlite_home(&path)?
            .expect("sqlite home")
            .as_path(),
        dir.path().join("state")
    );
    std::fs::write(&path, "[invalid")?;
    assert!(configured_sqlite_home(&path).is_err());
    Ok(())
}
