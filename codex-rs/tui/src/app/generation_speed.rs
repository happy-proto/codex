//! Optional local throughput; remote connections never consult the local database.

use super::*;

impl App {
    fn passive_generation_speed_target(
        &self,
    ) -> Option<crate::chatwidget::LiveGenerationSpeedTarget> {
        let active = self.current_displayed_thread_id()?;
        let passive = self
            .side_threads
            .get(&active)
            .map(|side| side.parent_thread_id)
            .or_else(|| self.side_thread_for_parent(active))?;
        self.side_companions
            .get(&passive)?
            .bottom
            .workspace_generation_speed_target(passive)
    }

    pub(super) fn has_live_generation_speed_target(&self) -> bool {
        self.chat_widget.live_generation_speed_target().is_some()
            || self.passive_generation_speed_target().is_some()
    }

    pub(super) fn refresh_live_generation_speed(&mut self) {
        let state_db = self
            .state_db
            .clone()
            .filter(|_| !self.app_server_target.uses_remote_workspace() && !self.reconnect.offline);
        let Some(state_db) = state_db else {
            self.chat_widget.set_live_generation_speed(None);
            for companion in self.side_companions.values_mut() {
                companion.bottom.set_recent_generation_speed(None);
            }
            return;
        };
        let targets: Vec<_> = self
            .chat_widget
            .live_generation_speed_target()
            .into_iter()
            .chain(self.passive_generation_speed_target())
            .collect();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            for target in targets {
                let speed = tokio::time::timeout(
                    Duration::from_millis(100),
                    state_db.live_generation_speed(
                        target.thread_id,
                        &target.turn_id,
                        target.observed_at_ms,
                    ),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .flatten();
                tx.send(AppEvent::LiveGenerationSpeedUpdated { target, speed });
            }
        });
    }

    pub(super) fn finish_live_generation_speed(
        &mut self,
        target: crate::chatwidget::LiveGenerationSpeedTarget,
        speed: Option<f64>,
    ) {
        let speed = speed
            .filter(|_| !self.app_server_target.uses_remote_workspace() && !self.reconnect.offline);
        if self.chat_widget.live_generation_speed_target().as_ref() == Some(&target) {
            self.chat_widget.set_live_generation_speed(speed);
        } else if self.passive_generation_speed_target().as_ref() == Some(&target)
            && let Some(companion) = self.side_companions.get_mut(&target.thread_id)
        {
            companion.bottom.set_recent_generation_speed(speed);
        }
    }

    pub(super) async fn refresh_generation_speeds(&mut self, thread_id: ThreadId) {
        self.chat_widget.generation_speeds = self.load_generation_speeds(thread_id).await;
    }

    pub(super) async fn refresh_parked_generation_speeds(&mut self, thread_id: ThreadId) {
        if !self.side_companions.contains_key(&thread_id) {
            return;
        }
        let speeds = self.load_generation_speeds(thread_id).await;
        if let Some(widget) = self
            .side_companions
            .get_mut(&thread_id)
            .and_then(|companion| companion.projection.as_deref_mut())
            .or_else(|| {
                self.background_voice
                    .as_deref_mut()
                    .filter(|owner| owner.thread_id() == Some(thread_id))
            })
        {
            widget.generation_speeds = speeds;
        }
    }

    async fn load_generation_speeds(
        &mut self,
        thread_id: ThreadId,
    ) -> HashMap<(ThreadId, String), codex_state::GenerationSpeedRecord> {
        if self.app_server_target.uses_remote_workspace() {
            tracing::info!(%thread_id, reason = "remote_workspace", "generation speed read skipped");
            return HashMap::new();
        }
        let Some(state_db) = self.state_db.as_ref() else {
            tracing::info!(%thread_id, reason = "storage_unavailable", "generation speed read skipped");
            return HashMap::new();
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
                return speeds
                    .into_iter()
                    .map(|(turn_id, speed)| ((thread_id, turn_id), speed))
                    .collect();
            }
            Ok(Err(err)) => tracing::info!(%thread_id, %err, "generation speed read skipped"),
            Err(_) => tracing::info!(%thread_id, "generation speed read timed out"),
        }
        HashMap::new()
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
