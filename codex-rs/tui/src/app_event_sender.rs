//! Convenience sender for app events and common outbound TUI commands.
//!
//! This wraps the raw channel so call sites can submit typed `AppCommand`s
//! without duplicating event construction or session logging behavior.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use crate::app_command::AppCommand;
use crate::app_command::UserVerificationResponse;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::RequestId as AppServerRequestId;
use codex_app_server_protocol::ReviewTarget;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use codex_protocol::ThreadId;
use codex_protocol::request_permissions::RequestPermissionsResponse;
use tokio::sync::mpsc::UnboundedSender;

use crate::app_event::AppEvent;
use crate::session_log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SideProjectionRoute {
    pub(crate) thread_id: ThreadId,
    pub(crate) generation: uuid::Uuid,
}

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    pub app_event_tx: UnboundedSender<AppEvent>,
    pub(crate) voice_only: Arc<AtomicBool>,
    pub(crate) side_projection: Arc<OnceLock<SideProjectionRoute>>,
}

impl AppEventSender {
    pub(crate) fn new(app_event_tx: UnboundedSender<AppEvent>) -> Self {
        Self {
            app_event_tx,
            voice_only: Arc::default(),
            side_projection: Arc::default(),
        }
    }

    /// Send an event to the app event channel. If it fails, we swallow the
    /// error and log it.
    pub(crate) fn send(&self, event: AppEvent) {
        if let Some(route) = self.side_projection.get().copied() {
            if matches!(
                &event,
                AppEvent::InsertHistoryCell(_)
                    | AppEvent::ConsolidateAgentMessage { .. }
                    | AppEvent::ConsolidateProposedPlan(_)
                    | AppEvent::StartCommitAnimation
                    | AppEvent::StopCommitAnimation
                    | AppEvent::CommitRealtimeTranscriptHistory
            ) {
                let _ = self.app_event_tx.send(AppEvent::SideProjectionEvent {
                    route,
                    event: Box::new(event),
                });
                return;
            }
            // A retained typed widget is a display projection, never another controller.
            // The existing voice owner keeps its separate voice-only allowlist below.
            if !self.voice_only.load(Ordering::Relaxed) {
                return;
            }
        }
        // A parked voice owner keeps processing its session, but its UI and queued
        // typed work must not act on the visible session. Thread-scoped metadata still applies.
        if self.voice_only.load(Ordering::Relaxed)
            && !matches!(
                &event,
                AppEvent::CodexOp(
                    AppCommand::RealtimeConversationStart { .. }
                        | AppCommand::RealtimeConversationStop { .. }
                        | AppCommand::RealtimeConversationSpeech { .. }
                ) | AppEvent::RealtimeWebrtcOfferCreated { .. }
                    | AppEvent::RealtimeWebrtcConnected { .. }
                    | AppEvent::StopRealtimeConversation { .. }
                    | AppEvent::RealtimeSessionLockStopped { .. }
                    | AppEvent::RealtimeConversationStateChanged
                    | AppEvent::BackgroundVoiceError { .. }
                    | AppEvent::SyncThreadGitBranch { .. }
                    | AppEvent::RefreshRateLimits { .. }
            )
        {
            return;
        }
        // Record inbound events for high-fidelity session replay.
        // Avoid double-logging Ops; those are logged at the point of submission.
        if !matches!(event, AppEvent::CodexOp(_)) {
            session_log::log_inbound_app_event(&event);
        }
        if let Err(e) = self.app_event_tx.send(event) {
            tracing::error!("failed to send event: {e}");
        }
    }

    pub(crate) fn interrupt(&self) {
        self.send(AppEvent::CodexOp(AppCommand::interrupt()));
    }

    pub(crate) fn compact(&self) {
        self.send(AppEvent::CodexOp(AppCommand::compact()));
    }

    pub(crate) fn set_thread_name(&self, name: String) {
        self.send(AppEvent::CodexOp(AppCommand::set_thread_name(name)));
    }

    pub(crate) fn review(&self, target: ReviewTarget) {
        self.send(AppEvent::CodexOp(AppCommand::review(target)));
    }

    pub(crate) fn user_input_answer(
        &self,
        thread_id: ThreadId,
        id: String,
        response: ToolRequestUserInputResponse,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::user_input_answer(id, response),
        });
    }

    pub(crate) fn exec_approval(
        &self,
        thread_id: ThreadId,
        id: String,
        decision: CommandExecutionApprovalDecision,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::exec_approval(id, /*turn_id*/ None, decision),
        });
    }

    pub(crate) fn request_permissions_response(
        &self,
        thread_id: ThreadId,
        id: String,
        response: RequestPermissionsResponse,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::request_permissions_response(id, response),
        });
    }

    pub(crate) fn patch_approval(
        &self,
        thread_id: ThreadId,
        id: String,
        decision: FileChangeApprovalDecision,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::patch_approval(id, decision),
        });
    }

    pub(crate) fn resolve_elicitation(
        &self,
        thread_id: ThreadId,
        server_name: String,
        request_id: AppServerRequestId,
        decision: McpServerElicitationAction,
        content: Option<serde_json::Value>,
        meta: Option<serde_json::Value>,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::resolve_elicitation(server_name, request_id, decision, content, meta),
        });
    }

    pub(crate) fn resolve_user_verification(
        &self,
        thread_id: ThreadId,
        server_name: String,
        request_id: AppServerRequestId,
        response: UserVerificationResponse,
    ) {
        self.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::resolve_user_verification(server_name, request_id, response),
        });
    }
}
