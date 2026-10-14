//! Fork-private final sampling throughput shared by local TUI and daemon processes.

use std::collections::HashMap;

use codex_protocol::ThreadId;
use sqlx::Row;
use sqlx::SqlitePool;

use super::StateRuntime;

pub(super) async fn initialize(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(include_str!("generation_speed.sql"))
        .execute(pool)
        .await?;
    Ok(())
}

impl StateRuntime {
    /// Persist only a complete, valid local turn before publishing its completion event.
    pub async fn save_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        output_tokens: u64,
        request_duration_us: u64,
    ) -> anyhow::Result<()> {
        let output_tokens = i64::try_from(output_tokens)?;
        let request_duration_us = i64::try_from(request_duration_us)?;
        anyhow::ensure!(output_tokens > 0 && request_duration_us > 0 && !turn_id.is_empty());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO fork_generation_speeds (thread_id, turn_id, output_tokens, request_duration_us)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(thread_id, turn_id) DO UPDATE SET
               output_tokens = excluded.output_tokens,
               request_duration_us = excluded.request_duration_us",
        )
        .bind(thread_id.to_string())
        .bind(turn_id)
        .bind(output_tokens)
        .bind(request_duration_us)
        .execute(&mut *tx)
        .await?;
        // Old histories may omit this optional metric. Bound storage and TUI replay caches.
        sqlx::query(
            "DELETE FROM fork_generation_speeds
             WHERE thread_id = ? AND rowid NOT IN (
               SELECT rowid FROM fork_generation_speeds WHERE thread_id = ?
               ORDER BY rowid DESC LIMIT 1000
             )",
        )
        .bind(thread_id.to_string())
        .bind(thread_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Read final metrics only; legacy turns and unavailable storage have no estimate.
    pub async fn generation_speeds(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<HashMap<String, f64>> {
        let rows = sqlx::query(
            "SELECT turn_id, output_tokens, request_duration_us
             FROM fork_generation_speeds WHERE thread_id = ?",
        )
        .bind(thread_id.to_string())
        .fetch_all(self.pool.as_ref())
        .await?;
        let mut speeds = HashMap::with_capacity(rows.len());
        for row in rows {
            let output_tokens: i64 = row.try_get("output_tokens")?;
            let duration_us: i64 = row.try_get("request_duration_us")?;
            if output_tokens > 0 && duration_us > 0 {
                speeds.insert(
                    row.try_get("turn_id")?,
                    output_tokens as f64 * 1_000_000.0 / duration_us as f64,
                );
            }
        }
        Ok(speeds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_support::test_thread_metadata;
    use crate::runtime::test_support::unique_temp_dir;
    use codex_utils_absolute_path::test_support::PathExt;

    #[tokio::test]
    async fn persists_isolated_turns_across_connections_and_thread_deletion() -> anyhow::Result<()>
    {
        let home = unique_temp_dir();
        let _cleanup = scopeguard::guard(home.clone(), |path| {
            let _ = std::fs::remove_dir_all(path);
        });
        let config = crate::SqliteConfig::new_for_testing(home.as_path().abs());
        let writer = StateRuntime::init(config.clone(), "test".to_string()).await?;
        let first = ThreadId::new();
        let second = ThreadId::new();
        for id in [first, second] {
            writer
                .upsert_thread(&test_thread_metadata(&home, id, home.clone()))
                .await?;
        }
        writer
            .save_generation_speed(first, "turn", 200, 4_000_000)
            .await?;
        writer
            .save_generation_speed(second, "turn", 300, 1_000_000)
            .await?;
        let reader = StateRuntime::init(config, "test".to_string()).await?;
        assert_eq!(
            reader.generation_speeds(first).await?.get("turn"),
            Some(&50.0)
        );
        assert_eq!(
            reader.generation_speeds(second).await?.get("turn"),
            Some(&300.0)
        );
        assert!(
            !reader
                .generation_speeds(first)
                .await?
                .contains_key("legacy")
        );
        writer.delete_thread(first).await?;
        assert!(reader.generation_speeds(first).await?.is_empty());
        assert_eq!(
            reader.generation_speeds(second).await?.get("turn"),
            Some(&300.0)
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_totals_and_bounds_retention() -> anyhow::Result<()> {
        let home = unique_temp_dir();
        let _cleanup = scopeguard::guard(home.clone(), |path| {
            let _ = std::fs::remove_dir_all(path);
        });
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(home.as_path().abs()),
            "test".to_string(),
        )
        .await?;
        let id = ThreadId::new();
        runtime
            .upsert_thread(&test_thread_metadata(&home, id, home.clone()))
            .await?;
        assert!(
            runtime
                .save_generation_speed(id, "zero", 0, 1)
                .await
                .is_err()
        );
        assert!(
            runtime
                .save_generation_speed(id, "zero", 1, 0)
                .await
                .is_err()
        );
        assert!(
            runtime
                .save_generation_speed(id, "overflow", u64::MAX, 1)
                .await
                .is_err()
        );
        for index in 0..1001 {
            runtime
                .save_generation_speed(id, &format!("turn-{index}"), 10, 1_000_000)
                .await?;
        }
        let speeds = runtime.generation_speeds(id).await?;
        assert_eq!(speeds.len(), 1000);
        assert!(!speeds.contains_key("turn-0"));
        assert_eq!(speeds.get("turn-1000"), Some(&10.0));
        Ok(())
    }
}
