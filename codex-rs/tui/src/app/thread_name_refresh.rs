//! Reconcile the displayed thread's name when another app-server instance renamed it.

use super::App;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_protocol::ThreadId;
use std::time::Duration;
use std::time::Instant;
use uuid::Uuid;

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct ThreadNameRefreshState {
    target: Option<(ThreadId, Uuid)>,
    next_poll: Option<Instant>,
    pending: Option<Uuid>,
}

impl ThreadNameRefreshState {
    fn deadline(&mut self, target: Option<(ThreadId, Uuid)>, now: Instant) -> Option<Instant> {
        if self.target != target {
            self.target = target;
            self.next_poll = target.map(|_| now + POLL_INTERVAL);
        }
        if self.pending.is_some() {
            return None;
        }
        self.next_poll
    }

    fn start(&mut self, now: Instant) -> Option<(Uuid, ThreadId, Uuid)> {
        let (thread_id, revision) = self.target?;
        if self.pending.is_some() || now < self.next_poll? {
            return None;
        }
        let request_id = Uuid::new_v4();
        self.pending = Some(request_id);
        self.next_poll = Some(now + POLL_INTERVAL);
        Some((request_id, thread_id, revision))
    }

    fn finish(&mut self, request_id: Uuid) -> bool {
        if self.pending != Some(request_id) {
            return false;
        }
        self.pending = None;
        true
    }
}

impl App {
    pub(super) fn thread_name_poll_deadline(&mut self) -> Option<Instant> {
        let target = (!self.reconnect.offline && self.chat_widget.displays_thread_name())
            .then(|| {
                self.chat_widget
                    .thread_id()
                    .map(|id| (id, self.chat_widget.thread_name_revision()))
            })
            .flatten();
        self.thread_name_refresh.deadline(target, Instant::now())
    }

    pub(super) fn refresh_current_thread_name(&mut self, app_server: &AppServerSession) {
        let Some((request_id, thread_id, revision)) =
            self.thread_name_refresh.start(Instant::now())
        else {
            return;
        };
        let handle = app_server.request_handle();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                READ_TIMEOUT,
                handle.request_typed::<ThreadReadResponse>(ClientRequest::ThreadRead {
                    request_id: RequestId::String(request_id.to_string()),
                    params: ThreadReadParams {
                        thread_id: thread_id.to_string(),
                        include_turns: false,
                    },
                }),
            )
            .await
            .map_err(|_| "thread name read timed out".to_string())
            .and_then(|result| result.map_err(|err| err.to_string()))
            .and_then(|response| {
                if response.thread.id != thread_id.to_string() {
                    return Err("thread name read returned a different thread".to_string());
                }
                Ok(response.thread.name)
            });
            tx.send(AppEvent::ThreadNameRefreshed {
                request_id,
                thread_id,
                revision,
                result,
            });
        });
    }

    pub(super) fn finish_thread_name_refresh(
        &mut self,
        request_id: Uuid,
        thread_id: ThreadId,
        revision: Uuid,
        result: Result<Option<String>, String>,
    ) {
        if !self.thread_name_refresh.finish(request_id) || self.reconnect.offline {
            return;
        }
        match result {
            Ok(name) => self
                .chat_widget
                .apply_refreshed_thread_name(thread_id, revision, name),
            Err(err) => tracing::debug!(%err, "could not refresh displayed thread name"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polls_every_thirty_seconds_without_overlapping_reads() {
        let now = Instant::now();
        let target = Some((ThreadId::new(), Uuid::new_v4()));
        let mut state = ThreadNameRefreshState::default();
        assert_eq!(state.deadline(target, now), Some(now + POLL_INTERVAL));
        assert!(
            state
                .start(now + POLL_INTERVAL - Duration::from_millis(1))
                .is_none()
        );
        let (request_id, _, _) = state.start(now + POLL_INTERVAL).unwrap();
        assert_eq!(state.deadline(target, now + POLL_INTERVAL), None);
        assert!(state.start(now + POLL_INTERVAL * 2).is_none());
        assert!(!state.finish(Uuid::new_v4()));
        assert!(state.finish(request_id));
        assert_eq!(
            state.deadline(target, now + POLL_INTERVAL),
            Some(now + POLL_INTERVAL * 2)
        );
    }

    #[test]
    fn completed_or_failed_reads_do_not_cause_a_retry_burst() {
        let now = Instant::now();
        let target = Some((ThreadId::new(), Uuid::new_v4()));
        let mut state = ThreadNameRefreshState::default();
        state.deadline(target, now);
        let (request_id, _, _) = state.start(now + POLL_INTERVAL).unwrap();
        assert!(state.finish(request_id));
        assert!(state.start(now + POLL_INTERVAL + READ_TIMEOUT).is_none());
        assert!(state.start(now + POLL_INTERVAL * 2).is_some());
    }

    #[test]
    fn switching_or_hiding_the_thread_resets_the_poll_target() {
        let now = Instant::now();
        let first = Some((ThreadId::new(), Uuid::new_v4()));
        let second = Some((ThreadId::new(), Uuid::new_v4()));
        let mut state = ThreadNameRefreshState::default();
        state.deadline(first, now);
        let (request_id, _, _) = state.start(now + POLL_INTERVAL).unwrap();
        assert_eq!(state.deadline(second, now + POLL_INTERVAL), None);
        assert!(state.finish(request_id));
        assert_eq!(
            state.deadline(second, now + POLL_INTERVAL),
            Some(now + POLL_INTERVAL * 2)
        );
        assert_eq!(state.deadline(None, now + POLL_INTERVAL), None);
        assert!(state.start(now + POLL_INTERVAL * 3).is_none());
    }
}
