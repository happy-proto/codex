//! Bounded, read-only metadata queries for shell completion.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use sqlx::Row;

use crate::SqliteConfig;

#[derive(Debug, PartialEq, Eq)]
pub struct ThreadCompletion {
    pub id: String,
    pub label: String,
    pub cwd: PathBuf,
    pub unique_label: bool,
}

/// Read active sessions without initializing, migrating, or repairing the database.
/// The caller must also bound the total time spent connecting and querying.
pub async fn read_thread_completions(
    sqlite: &SqliteConfig,
    query: &str,
    cwd: Option<&Path>,
    include_non_interactive: bool,
) -> anyhow::Result<Vec<ThreadCompletion>> {
    let pool = sqlite
        .open_read_only_pool(&sqlite.state_db_path(), Some(Duration::from_millis(20)))
        .await?;
    let rows = sqlx::query(
        r#"
SELECT t.id, COALESCE(NULLIF(trim(t.name), ''), trim(t.preview)) AS label, t.cwd,
       CASE WHEN ? = '' OR instr(t.id, ?) = 1 OR NULLIF(trim(t.name), '') IS NULL
       THEN 0 ELSE NOT EXISTS (
           SELECT 1 FROM threads other
           WHERE other.archived = 0 AND other.id <> t.id
             AND COALESCE(NULLIF(trim(other.name), ''), trim(other.preview)) =
                 COALESCE(NULLIF(trim(t.name), ''), trim(t.preview))
       ) END AS unique_label
FROM threads t
WHERE t.archived = 0
  AND (t.source IN ('cli', 'vscode') OR (? AND t.source IN ('exec', 'mcp')))
  AND (? IS NULL OR t.cwd = ?)
  AND (instr(t.id, ?) = 1
       OR instr(lower(COALESCE(NULLIF(trim(t.name), ''), trim(t.preview))), lower(?)) > 0)
ORDER BY t.recency_at_ms DESC, t.id DESC
LIMIT 100
        "#,
    )
    .bind(query)
    .bind(query)
    .bind(include_non_interactive)
    .bind(cwd.map(|path| path.to_string_lossy().into_owned()))
    .bind(cwd.map(|path| path.to_string_lossy().into_owned()))
    .bind(query)
    .bind(query)
    .fetch_all(&pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ThreadCompletion {
                id: row.try_get("id")?,
                label: row.try_get("label")?,
                cwd: PathBuf::from(row.try_get::<String, _>("cwd")?),
                unique_label: row.try_get("unique_label")?,
            })
        })
        .collect()
}
