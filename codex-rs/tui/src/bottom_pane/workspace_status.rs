//! Compact, persistent task state for both halves of a docked conversation.
//! Its clock belongs to the thread, so changing input focus cannot restart it.

use std::time::Duration;
use std::time::Instant;

use ratatui::text::Line;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkspacePhase {
    #[default]
    Idle,
    Working,
    NeedsInput,
    NeedsApproval,
    Completed,
    Failed,
    Interrupted,
    Closed,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkspaceStatus {
    pub(crate) turn_id: Option<String>,
    pub(crate) phase: WorkspacePhase,
    started_at: Option<Instant>,
    elapsed: Duration,
}

impl WorkspaceStatus {
    pub(crate) fn start(&mut self, turn_id: &str, elapsed: Duration) {
        if self.turn_id.as_deref() == Some(turn_id) {
            return;
        }
        self.turn_id = Some(turn_id.to_owned());
        self.elapsed = elapsed;
        self.started_at = None;
        self.phase = WorkspacePhase::Idle;
        self.set_phase(WorkspacePhase::Working);
    }

    pub(crate) fn set_phase(&mut self, phase: WorkspacePhase) {
        if phase == self.phase {
            return;
        }
        let now = Instant::now();
        if let Some(started) = self.started_at.take() {
            self.elapsed += now.saturating_duration_since(started);
        }
        if phase == WorkspacePhase::Working {
            self.started_at = Some(now);
        }
        self.phase = phase;
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.elapsed + self.started_at.map_or(Duration::ZERO, |at| at.elapsed())
    }

    pub(crate) fn line(&self, header: Option<&str>, interrupt: Option<String>) -> Line<'static> {
        let label = match self.phase {
            WorkspacePhase::Idle => "Ready",
            WorkspacePhase::Working => header.unwrap_or("Working"),
            WorkspacePhase::NeedsInput => "Needs input",
            WorkspacePhase::NeedsApproval => "Needs approval",
            WorkspacePhase::Completed => "Completed",
            WorkspacePhase::Failed => "Failed",
            WorkspacePhase::Interrupted => "Interrupted",
            WorkspacePhase::Closed => "Closed",
        };
        let mut text = format!("• {label}");
        if self.turn_id.is_some() {
            text.push_str(&format!(
                " · {}",
                crate::status_indicator_widget::fmt_elapsed_compact(self.elapsed().as_secs())
            ));
        }
        if self.phase == WorkspacePhase::Working
            && let Some(interrupt) = interrupt
        {
            text.push_str(&format!(" · {interrupt} to interrupt"));
        }
        text.into()
    }
}
