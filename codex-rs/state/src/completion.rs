//! Bounded, read-only metadata queries for shell completion.

use std::path::Path;
use std::time::Duration;

use sqlx::Row;

use crate::SqliteConfig;

#[derive(Debug, PartialEq, Eq)]
pub struct ThreadCompletion {
    pub id: String,
    pub label: String,
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
SELECT t.id, COALESCE(trim(t.name), '') AS label,
       CASE WHEN ? = '' OR instr(lower(t.id), lower(?)) > 0
                 OR NULLIF(trim(t.name), '') IS NULL OR t.source NOT IN ('cli', 'vscode')
       THEN 0 ELSE NOT EXISTS (
           SELECT 1 FROM threads other
           WHERE other.archived = 0 AND other.attached_thread_parent_id IS NULL
             AND other.id <> t.id
             AND COALESCE(NULLIF(trim(other.name), ''), trim(other.preview)) = trim(t.name)
       ) END AS unique_label
FROM threads t
WHERE t.archived = 0
  AND t.attached_thread_parent_id IS NULL
  AND (t.source IN ('cli', 'vscode') OR (? AND t.source IN ('exec', 'mcp')))
  AND (? IS NULL OR t.cwd = ?)
  AND (instr(lower(t.id), lower(?)) > 0
       OR (instr(lower(t.name), lower(?)) > 0 AND unique_label))
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
                unique_label: row.try_get("unique_label")?,
            })
        })
        .collect()
}
