//! Fork-private final sampling throughput shared by local TUI and daemon processes.

use std::collections::HashMap;

use codex_protocol::ThreadId;
use sqlx::Row;
use sqlx::SqlitePool;

use super::StateRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationSpeedCompletion {
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenerationSpeedRecord {
    speed: f64,
    completion: GenerationSpeedCompletion,
}

impl GenerationSpeedRecord {
    pub fn new(speed: f64, completion: GenerationSpeedCompletion) -> Self {
        Self { speed, completion }
    }

    pub fn tokens_per_second(&self) -> f64 {
        self.speed
    }

    /// A surviving pre-crash row is not evidence for a new completion of the same turn.
    pub fn for_completion(
        &self,
        started_at: Option<i64>,
        completed_at: Option<i64>,
        duration_ms: Option<i64>,
    ) -> Option<f64> {
        let matches = started_at == Some(self.completion.started_at)
            && completed_at == Some(self.completion.completed_at)
            && duration_ms == Some(self.completion.duration_ms);
        if !matches {
            tracing::info!(
                event.name = "codex.generation_speed.completion_mismatch",
                expected_started_at = self.completion.started_at,
                expected_completed_at = self.completion.completed_at,
                expected_duration_ms = self.completion.duration_ms,
                ?started_at,
                ?completed_at,
                ?duration_ms,
                "generation speed completion identity mismatch"
            );
        }
        matches.then_some(self.speed)
    }
}

pub(super) async fn initialize(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(include_str!("generation_speed.sql"))
        .execute(pool)
        .await?;
    sqlx::query(include_str!("live_generation_speed.sql"))
        .execute(pool)
        .await?;
    Ok(())
}

impl StateRuntime {
    /// Reset the transient snapshot before a new local turn starts, including recovery.
    pub async fn start_live_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        run_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO fork_live_generation_speeds_v1 (thread_id, turn_id, run_id, updated_at_ms, speed)
             VALUES (?, ?, ?, ?, NULL)
             ON CONFLICT(thread_id) DO UPDATE SET
               turn_id = excluded.turn_id, run_id = excluded.run_id,
               updated_at_ms = excluded.updated_at_ms, speed = NULL",
        )
        .bind(thread_id.to_string())
        .bind(turn_id)
        .bind(run_id)
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(self.pool.as_ref())
        .await?;
        Ok(())
    }

    /// An old reporter cannot overwrite a replacement turn or a recovered run.
    pub async fn update_live_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        run_id: &str,
        speed: Option<f64>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(speed.is_none_or(|speed| speed.is_finite() && speed >= 0.0));
        sqlx::query(
            "UPDATE fork_live_generation_speeds_v1 SET speed = ?, updated_at_ms = ?
             WHERE thread_id = ? AND turn_id = ? AND run_id = ?",
        )
        .bind(speed)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(thread_id.to_string())
        .bind(turn_id)
        .bind(run_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(())
    }

    /// Only the displayed active turn may use a transient metric.
    pub async fn live_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> anyhow::Result<Option<f64>> {
        // Wall time is only a reporter heartbeat, never the rolling sampling clock.
        let now_ms = chrono::Utc::now().timestamp_millis();
        let speed = sqlx::query_scalar::<_, Option<f64>>(
            "SELECT speed FROM fork_live_generation_speeds_v1 WHERE thread_id = ? AND turn_id = ?
             AND updated_at_ms BETWEEN ? AND ?",
        )
        .bind(thread_id.to_string())
        .bind(turn_id)
        .bind(now_ms.saturating_sub(3000))
        .bind(now_ms)
        .fetch_optional(self.pool.as_ref())
        .await?
        .flatten();
        Ok(speed.filter(|speed| speed.is_finite() && *speed >= 0.0))
    }

    /// Remove any pre-crash measurement when the same turn resumes or becomes invalid.
    pub async fn clear_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM fork_generation_speeds_v1 WHERE thread_id = ? AND turn_id = ?")
            .bind(thread_id.to_string())
            .bind(turn_id)
            .execute(self.pool.as_ref())
            .await?;
        Ok(())
    }

    /// Persist only a complete, valid local turn before publishing its completion event.
    pub async fn save_generation_speed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        output_tokens: u64,
        request_duration_us: u64,
        completion: GenerationSpeedCompletion,
    ) -> anyhow::Result<()> {
        let output_tokens = i64::try_from(output_tokens)?;
        let request_duration_us = i64::try_from(request_duration_us)?;
        anyhow::ensure!(
            output_tokens > 0
                && request_duration_us > 0
                && !turn_id.is_empty()
                && completion.duration_ms >= 0
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO fork_generation_speeds_v1 (thread_id, turn_id, output_tokens, request_duration_us, started_at, completed_at, duration_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(thread_id, turn_id) DO UPDATE SET
               output_tokens = excluded.output_tokens,
               request_duration_us = excluded.request_duration_us,
               started_at = excluded.started_at,
               completed_at = excluded.completed_at,
               duration_ms = excluded.duration_ms",
        )
        .bind(thread_id.to_string())
        .bind(turn_id)
        .bind(output_tokens)
        .bind(request_duration_us)
        .bind(completion.started_at)
        .bind(completion.completed_at)
        .bind(completion.duration_ms)
        .execute(&mut *tx)
        .await?;
        // Old histories may omit this optional metric. Bound storage and TUI replay caches.
        sqlx::query(
            "DELETE FROM fork_generation_speeds_v1
             WHERE thread_id = ? AND rowid NOT IN (
               SELECT rowid FROM fork_generation_speeds_v1 WHERE thread_id = ?
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
    ) -> anyhow::Result<HashMap<String, GenerationSpeedRecord>> {
        let rows = sqlx::query(
            "SELECT turn_id, output_tokens, request_duration_us, started_at, completed_at, duration_ms
             FROM fork_generation_speeds_v1 WHERE thread_id = ?",
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
                    GenerationSpeedRecord::new(
                        output_tokens as f64 * 1_000_000.0 / duration_us as f64,
                        GenerationSpeedCompletion {
                            started_at: row.try_get("started_at")?,
                            completed_at: row.try_get("completed_at")?,
                            duration_ms: row.try_get("duration_ms")?,
                        },
                    ),
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
    async fn live_generation_speed_is_shared_isolated_and_rejects_old_reporters()
    -> anyhow::Result<()> {
        let home = unique_temp_dir();
        let _cleanup = scopeguard::guard(home.clone(), |path| {
            let _ = std::fs::remove_dir_all(path);
        });
        let config = crate::SqliteConfig::new_for_testing(home.as_path().abs());
        let writer = StateRuntime::init(config.clone(), "test".into()).await?;
        let thread_id = ThreadId::new();
        writer
            .upsert_thread(&test_thread_metadata(&home, thread_id, home.clone()))
            .await?;
        let reader = StateRuntime::init(config, "test".into()).await?;
        writer
            .start_live_generation_speed(thread_id, "turn", "old")
            .await?;
        writer
            .update_live_generation_speed(thread_id, "turn", "old", Some(42.3))
            .await?;
        assert_eq!(
            reader.live_generation_speed(thread_id, "turn").await?,
            Some(42.3)
        );
        assert_eq!(
            reader.live_generation_speed(thread_id, "other").await?,
            None
        );
        assert_eq!(
            reader
                .live_generation_speed(ThreadId::new(), "turn")
                .await?,
            None
        );
        writer
            .start_live_generation_speed(thread_id, "turn", "recovered")
            .await?;
        writer
            .update_live_generation_speed(thread_id, "turn", "old", Some(999.0))
            .await?;
        assert_eq!(reader.live_generation_speed(thread_id, "turn").await?, None);
        writer
            .update_live_generation_speed(thread_id, "turn", "recovered", Some(0.0))
            .await?;
        assert_eq!(
            reader.live_generation_speed(thread_id, "turn").await?,
            Some(0.0)
        );
        for invalid in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(
                writer
                    .update_live_generation_speed(thread_id, "turn", "recovered", Some(invalid))
                    .await
                    .is_err()
            );
        }
        writer
            .update_live_generation_speed(thread_id, "turn", "recovered", None)
            .await?;
        assert_eq!(reader.live_generation_speed(thread_id, "turn").await?, None);
        Ok(())
    }

    fn completion() -> GenerationSpeedCompletion {
        GenerationSpeedCompletion {
            started_at: 10,
            completed_at: 14,
            duration_ms: 4000,
        }
    }

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
            .save_generation_speed(first, "turn", 200, 4_000_000, completion())
            .await?;
        writer
            .save_generation_speed(second, "turn", 300, 1_000_000, completion())
            .await?;
        let reader = StateRuntime::init(config, "test".to_string()).await?;
        assert_eq!(
            reader
                .generation_speeds(first)
                .await?
                .get("turn")
                .map(GenerationSpeedRecord::tokens_per_second),
            Some(50.0)
        );
        assert_eq!(
            reader
                .generation_speeds(second)
                .await?
                .get("turn")
                .map(GenerationSpeedRecord::tokens_per_second),
            Some(300.0)
        );
        assert!(
            !reader
                .generation_speeds(first)
                .await?
                .contains_key("legacy")
        );
        writer.clear_generation_speed(first, "turn").await?;
        assert!(reader.generation_speeds(first).await?.is_empty());
        writer
            .save_generation_speed(first, "turn", 200, 4_000_000, completion())
            .await?;
        writer.delete_thread(first).await?;
        assert!(reader.generation_speeds(first).await?.is_empty());
        assert_eq!(
            reader
                .generation_speeds(second)
                .await?
                .get("turn")
                .map(GenerationSpeedRecord::tokens_per_second),
            Some(300.0)
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
                .save_generation_speed(id, "zero", 0, 1, completion())
                .await
                .is_err()
        );
        assert!(
            runtime
                .save_generation_speed(id, "zero", 1, 0, completion())
                .await
                .is_err()
        );
        assert!(
            runtime
                .save_generation_speed(id, "overflow", u64::MAX, 1, completion())
                .await
                .is_err()
        );
        for index in 0..1001 {
            runtime
                .save_generation_speed(id, &format!("turn-{index}"), 10, 1_000_000, completion())
                .await?;
        }
        let speeds = runtime.generation_speeds(id).await?;
        assert_eq!(speeds.len(), 1000);
        assert!(!speeds.contains_key("turn-0"));
        assert_eq!(
            speeds
                .get("turn-1000")
                .map(GenerationSpeedRecord::tokens_per_second),
            Some(10.0)
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_write_lock_cannot_make_old_statistics_match_a_new_completion() -> anyhow::Result<()>
    {
        let home = unique_temp_dir();
        let _cleanup = scopeguard::guard(home.clone(), |path| {
            let _ = std::fs::remove_dir_all(path);
        });
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(home.as_path().abs()),
            "test".into(),
        )
        .await?;
        let id = ThreadId::new();
        runtime
            .upsert_thread(&test_thread_metadata(&home, id, home.clone()))
            .await?;
        runtime
            .save_generation_speed(id, "turn", 200, 4_000_000, completion())
            .await?;
        let tx = runtime.pool.begin_with("BEGIN IMMEDIATE").await?;
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                runtime.clear_generation_speed(id, "turn")
            )
            .await
            .is_err()
        );
        // WAL readers can still see the committed old row while the writer is blocked.
        let speeds = runtime.generation_speeds(id).await?;
        let saved = speeds
            .get("turn")
            .expect("old row survived cleanup timeout");
        assert_eq!(
            saved.for_completion(Some(10), Some(14), Some(4000)),
            Some(50.0)
        );
        for current in [(11, 14, 4000), (10, 15, 4000), (10, 14, 4001)] {
            assert_eq!(
                saved.for_completion(Some(current.0), Some(current.1), Some(current.2)),
                None
            );
        }
        assert_eq!(saved.for_completion(None, Some(14), Some(4000)), None);
        tx.rollback().await?;
        Ok(())
    }
}
