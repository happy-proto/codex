//! Optional local throughput; remote connections never consult the local database.

use super::*;

impl App {
    pub(super) async fn refresh_generation_speeds(&mut self, thread_id: ThreadId) {
        self.chat_widget.generation_speeds.clear();
        if self.app_server_target.uses_remote_workspace() {
            tracing::info!(%thread_id, reason = "remote_workspace", "generation speed read skipped");
            return;
        }
        let Some(state_db) = self.state_db.as_ref() else {
            tracing::info!(%thread_id, reason = "storage_unavailable", "generation speed read skipped");
            return;
        };
        // Optional metadata must not hold up transcript completion on a busy database.
        match tokio::time::timeout(
            Duration::from_millis(100),
            state_db.generation_speeds(thread_id),
        )
        .await
        {
            Ok(Ok(speeds)) => {
                tracing::info!(%thread_id, records = speeds.len(), "generation speed records loaded");
                self.chat_widget.generation_speeds = speeds
                    .into_iter()
                    .map(|(turn_id, speed)| ((thread_id, turn_id), speed))
                    .collect();
            }
            Ok(Err(err)) => tracing::info!(%thread_id, %err, "generation speed read skipped"),
            Err(_) => tracing::info!(%thread_id, "generation speed read timed out"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::make_test_app;
    use crate::test_support::boxed_future;
    use codex_protocol::protocol::SessionSource;

    fn completion() -> codex_state::GenerationSpeedCompletion {
        codex_state::GenerationSpeedCompletion {
            started_at: 10,
            completed_at: 14,
            duration_ms: 4000,
        }
    }

    #[tokio::test]
    async fn generation_speed_local_daemon_replay_is_isolated_and_remote_is_hidden()
    -> anyhow::Result<()> {
        let mut app = boxed_future!(make_test_app()).await;
        let db = codex_state::StateRuntime::init(app.config.sqlite_config().clone(), "test".into())
            .await?;
        let first = ThreadId::new();
        let second = ThreadId::new();
        for (thread_id, tokens) in [(first, 100), (second, 300)] {
            let metadata = codex_state::ThreadMetadataBuilder::new(
                thread_id,
                app.config
                    .codex_home
                    .join(format!("rollout-{thread_id}.jsonl"))
                    .to_path_buf(),
                chrono::Utc::now(),
                SessionSource::Cli,
            )
            .build("test");
            db.upsert_thread(&metadata).await?;
            db.save_generation_speed(thread_id, "turn", tokens, 1_000_000, completion())
                .await?;
        }
        app.state_db = Some(db);
        app.refresh_generation_speeds(first).await;
        assert_eq!(
            app.chat_widget
                .generation_speeds
                .get(&(first, "turn".into()))
                .map(codex_state::GenerationSpeedRecord::tokens_per_second),
            Some(100.0)
        );
        let endpoint = crate::RemoteAppServerEndpoint::UnixSocket {
            socket_path: codex_utils_absolute_path::AbsolutePathBuf::try_from(
                app.config.codex_home.join("daemon.sock"),
            )?,
        };
        app.app_server_target = AppServerTarget::LocalDaemon {
            endpoint: endpoint.clone(),
            allow_embedded_fallback: true,
        };
        app.refresh_generation_speeds(second).await;
        assert_eq!(app.chat_widget.generation_speeds.len(), 1);
        assert_eq!(
            app.chat_widget
                .generation_speeds
                .get(&(second, "turn".into()))
                .map(codex_state::GenerationSpeedRecord::tokens_per_second),
            Some(300.0)
        );
        app.app_server_target = AppServerTarget::Remote { endpoint };
        app.refresh_generation_speeds(first).await;
        assert!(app.chat_widget.generation_speeds.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn generation_speed_unavailable_storage_discards_cached_values() {
        let mut app = boxed_future!(make_test_app()).await;
        let thread_id = ThreadId::new();
        app.chat_widget.generation_speeds.insert(
            (thread_id, "turn".into()),
            codex_state::GenerationSpeedRecord::new(100.0, completion()),
        );
        app.refresh_generation_speeds(thread_id).await;
        assert!(app.chat_widget.generation_speeds.is_empty());
    }
}
