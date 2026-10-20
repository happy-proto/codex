use std::path::Path;
use std::process::Command;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;

const FIRST: &str = "11111111-1111-4111-8111-111111111111";
const SECOND: &str = "22222222-2222-4222-8222-222222222222";
const EXEC: &str = "33333333-3333-4333-8333-333333333333";

#[test]
fn fish_completion_command_installs_dynamic_entrypoint() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .args(["completion", "fish"])
        .env("CODEX_HOME", home.path())
        .env_remove("COMPLETE")
        .output()?;
    assert!(output.status.success());
    let script = String::from_utf8(output.stdout)?;
    assert_eq!(script.lines().count(), 1);
    assert_eq!(script, "COMPLETE=fish command codex | source\n");
    Ok(())
}

async fn fixture(home: &Path) -> anyhow::Result<SqliteConfig> {
    let sqlite = SqliteConfig::from_sqlite_home(AbsolutePathBuf::try_from(home)?);
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    sqlx::raw_sql(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, name TEXT, preview TEXT NOT NULL, \
         cwd TEXT NOT NULL, archived INTEGER NOT NULL, source TEXT NOT NULL, recency_at_ms INTEGER NOT NULL);",
    )
    .execute(&pool)
    .await?;
    for (id, name, cwd, archived, source, recency) in [
        (FIRST, "修复 shell 补全", "/project", 0, "cli", 30),
        (SECOND, "other project", "/other", 0, "vscode", 20),
        (EXEC, "script", "/project", 0, "exec", 40),
        ("archived", "archived", "/project", 1, "cli", 50),
        ("subagent", "child", "/project", 0, "subagent", 60),
    ] {
        sqlx::query("INSERT INTO threads VALUES (?, ?, '', ?, ?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(cwd)
            .bind(archived)
            .bind(source)
            .bind(recency)
            .execute(&pool)
            .await?;
    }
    pool.close().await;
    Ok(sqlite)
}

fn complete(home: &Path, shell: &str, words: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", home)
        .env_remove("CODEX_SQLITE_HOME")
        .env("COMPLETE", shell)
        .env("_CLAP_COMPLETE_INDEX", (words.len() - 1).to_string())
        .arg("--")
        .args(words)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

#[tokio::test]
async fn query_filters_sources_directories_and_archived_sessions_without_migrating()
-> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let sqlite = fixture(home.path()).await?;
    let before = std::fs::read(sqlite.state_db_path())?;
    let rows =
        codex_state::read_thread_completions(&sqlite, "", Some(Path::new("/project")), false)
            .await?;
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        [FIRST]
    );
    let rows = codex_state::read_thread_completions(&sqlite, "", None, true).await?;
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        [EXEC, FIRST, SECOND]
    );
    let rows = codex_state::read_thread_completions(&sqlite, "补全", None, false).await?;
    assert_eq!(rows[0].id, FIRST);
    assert!(rows[0].unique_label);
    let rows = codex_state::read_thread_completions(&sqlite, "2222", None, false).await?;
    assert_eq!(rows[0].id, SECOND);
    assert_eq!(std::fs::read(sqlite.state_db_path())?, before);
    Ok(())
}

#[tokio::test]
async fn shell_protocol_returns_ids_and_zsh_exact_names() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    fixture(home.path()).await?;
    let fish = complete(home.path(), "fish", &["codex", "resume", "--all", "修复"])?;
    assert!(fish.starts_with(&format!("{FIRST}\t")), "{fish}");
    assert!(fish.contains("修复 shell 补全"));
    let zsh = complete(home.path(), "zsh", &["codex", "resume", "--all", "修复"])?;
    assert!(zsh.starts_with("修复 shell 补全:"), "{zsh}");
    let fork = complete(home.path(), "fish", &["codex", "fork", "--all", "1111"])?;
    assert!(fork.starts_with(FIRST));
    Ok(())
}

#[tokio::test]
async fn fish_registration_matches_quoted_and_escaped_session_names() -> anyhow::Result<()> {
    let fish = match std::env::var_os("CODEX_FISH_TEST_BINARY") {
        Some(path) => std::path::PathBuf::from(path),
        None => match which::which("fish") {
            Ok(path) => path,
            Err(_) => {
                eprintln!("Fish is unavailable; set CODEX_FISH_TEST_BINARY to run the shell test");
                return Ok(());
            }
        },
    };
    let home = tempfile::tempdir()?;
    fixture(home.path()).await?;
    let binary = codex_utils_cargo_bin::cargo_bin("codex")?;
    let mut paths = vec![binary.parent().expect("CLI binary directory").to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths)?;
    for (line, expected) in [
        ("codex resume --all 修复", FIRST),
        ("codex resume --all '修复 sh", FIRST),
        ("codex resume --all \"修复 sh", FIRST),
        (r"codex resume --all 修复\ sh", FIRST),
        ("codex resume --all 1111", FIRST),
        ("codex --work", "--worktree"),
        ("codex fork --work", "--worktree"),
        ("codex --sandbox r", "read-only"),
        ("codex res", "resume"),
    ] {
        let output = Command::new(&fish)
            .args(["--no-config", "-c", "set -g fish_complete_path; \"$CODEX_COMPLETION_TEST_BINARY\" completion fish | source; complete -C \"$CODEX_COMPLETION_TEST_LINE\""])
            .env("CODEX_COMPLETION_TEST_BINARY", &binary)
            .env("PATH", &path)
            .env("CODEX_COMPLETION_TEST_LINE", line)
            .env("CODEX_HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env_remove("CODEX_SQLITE_HOME")
            .env_remove("COMPLETE")
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let candidates = String::from_utf8(output.stdout)?;
        assert!(
            candidates
                .lines()
                .any(|candidate| candidate.split('\t').next() == Some(expected)),
            "{line}: {candidates}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn names_remain_ambiguous_across_directories_and_results_are_bounded() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let sqlite = fixture(home.path()).await?;
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    sqlx::query("UPDATE threads SET name = 'same name' WHERE id IN (?, ?)")
        .bind(FIRST)
        .bind(SECOND)
        .execute(&pool)
        .await?;
    let rows =
        codex_state::read_thread_completions(&sqlite, "same", Some(Path::new("/project")), false)
            .await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].unique_label);
    for index in 0..105 {
        sqlx::query("INSERT INTO threads VALUES (?, 'bulk', '', '/project', 0, 'cli', ?)")
            .bind(format!("bulk-{index:03}"))
            .bind(index + 100)
            .execute(&pool)
            .await?;
    }
    pool.close().await;
    let rows = codex_state::read_thread_completions(&sqlite, "", None, false).await?;
    assert_eq!(rows.len(), 100);
    assert_eq!(rows[0].id, "bulk-104");
    assert_eq!(rows[99].id, "bulk-005");
    Ok(())
}

#[tokio::test]
async fn unnamed_previews_are_searchable_but_never_inserted_as_zsh_names() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let sqlite = fixture(home.path()).await?;
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    sqlx::query("UPDATE threads SET name = NULL, preview = 'unnamed preview' WHERE id = ?")
        .bind(FIRST)
        .execute(&pool)
        .await?;
    pool.close().await;
    let rows = codex_state::read_thread_completions(&sqlite, "unnamed", None, false).await?;
    assert_eq!(rows[0].label, "unnamed preview");
    assert!(!rows[0].unique_label);
    assert!(
        complete(
            home.path(),
            "fish",
            &["codex", "resume", "--all", "unnamed"]
        )?
        .starts_with(FIRST)
    );
    assert!(complete(home.path(), "zsh", &["codex", "resume", "--all", "unnamed"])?.is_empty());
    Ok(())
}

#[tokio::test]
async fn cli_storage_override_is_used_without_creating_default_database() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let storage = tempfile::tempdir()?;
    fixture(storage.path()).await?;
    let assignment = format!("sqlite_home={:?}", storage.path().to_string_lossy());
    let output = complete(
        home.path(),
        "fish",
        &["codex", "resume", "--all", "-c", &assignment, "1111"],
    )?;
    assert!(output.starts_with(FIRST), "{output}");
    assert_eq!(std::fs::read_dir(home.path())?.count(), 0);
    Ok(())
}

#[test]
fn missing_database_and_last_prompt_do_not_initialize_storage() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    assert!(complete(home.path(), "fish", &["codex", "resume", "--all", "1111"])?.is_empty());
    assert!(complete(home.path(), "fish", &["codex", "resume", "--last", "1111"])?.is_empty());
    assert_eq!(std::fs::read_dir(home.path())?.count(), 0);
    Ok(())
}
