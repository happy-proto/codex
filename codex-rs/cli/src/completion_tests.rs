use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn thread(id: &str, label: &str, unique_label: bool) -> ThreadCompletion {
    ThreadCompletion {
        id: id.into(),
        label: label.into(),
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
fn empty_query_shows_only_ids_in_recency_order() {
    let candidates = candidates(
        vec![
            thread("02", "修复\n补全\t", false),
            thread("01", "older", true),
        ],
        "",
    );
    assert_eq!(candidates[0].get_value(), "02");
    assert_eq!(candidates[1].get_value(), "01");
    assert_eq!(candidates[0].get_display_order(), Some(0));
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.get_help().is_none())
    );
}

#[test]
fn substrings_show_and_insert_only_the_matched_field() {
    for query in ["shell", "SHELL", "补全"] {
        let results = candidates(vec![thread("01", "修复 Shell 补全", true)], query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].get_value(), "修复 Shell 补全");
        assert!(results[0].get_help().is_none());
    }
    for query in ["c461ac05b237", "C461AC05B237", "6c70-7e92"] {
        let id = "01a1582d-6c70-7e92-b3fa-c461ac05b237";
        let results = candidates(vec![thread(id, "title", false)], query);
        assert_eq!(results[0].get_value(), id);
        assert!(results[0].get_help().is_none());
    }
    // ID wins when both fields match.
    assert_eq!(
        candidates(vec![thread("01", "title 01", true)], "01")[0].get_value(),
        "01"
    );
}

#[test]
fn unsafe_or_unmatched_titles_are_not_inserted() {
    assert!(candidates(vec![thread("01", "修复", false)], "修复").is_empty());
    assert!(candidates(vec![thread("01", "修复\n补全", true)], "修复").is_empty());
    assert!(candidates(vec![thread("01", "other", true)], "absent").is_empty());
    assert!(candidates(vec![thread("01", "--all", true)], "all").is_empty());
    let title = "01a1582d-6c70-7e92-b3fa-c461ac05b237";
    assert!(candidates(vec![thread("01", title, true)], "c461ac05b237").is_empty());
}

#[test]
fn bash_and_powershell_registration_quote_names_without_changing_display_values()
-> std::io::Result<()> {
    let mut script = Vec::new();
    QuotingShell(&Bash).write_registration("COMPLETE", "codex", "codex", "codex", &mut script)?;
    let script = String::from_utf8(script).expect("UTF-8 registration");
    assert!(script.contains("complete -o filenames -o nospace -o bashdefault"));
    let mut script = Vec::new();
    QuotingShell(&Powershell).write_registration(
        "COMPLETE",
        "codex",
        "codex",
        "codex",
        &mut script,
    )?;
    let script = String::from_utf8(script).expect("UTF-8 registration");
    assert!(script.contains(r#"::new(("'" + $cmd.Replace("'", "''") + "'"), $cmd,"#));
    Ok(())
}

#[test]
fn zsh_registration_does_not_reapply_prefix_filtering() -> std::io::Result<()> {
    let mut script = Vec::new();
    ZshWithSubstringMatches.write_registration(
        "COMPLETE",
        "codex",
        "codex",
        "codex",
        &mut script,
    )?;
    let script = String::from_utf8(script).expect("UTF-8 registration");
    assert!(script.contains("_describe -V 'values' other -U"));
    // A title ending in '/' also takes clap's directory branch.
    assert!(script.contains("_describe -V 'values' dirs -U -S '/' -r '/'"));
    assert!(script.contains(ZSH_MATCH_HIGHLIGHT));
    Ok(())
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
