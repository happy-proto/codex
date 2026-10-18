//! Completion metadata for live and restored turns. Saved timestamps are authoritative;
//! only a live completion may fall back to the local clock. Labels are deduplicated per thread.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveGenerationSpeedTarget {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub started_at: Instant,
    pub observed_at_ms: i64,
}

impl ChatWidget {
    pub(crate) fn live_generation_speed_target(&self) -> Option<LiveGenerationSpeedTarget> {
        if !self.turn_lifecycle.agent_turn_running {
            return None;
        }
        Some(LiveGenerationSpeedTarget {
            thread_id: self.thread_id?,
            turn_id: self.turn_lifecycle.last_turn_id.clone()?,
            started_at: self.turn_lifecycle.goal_status_active_turn_started_at?,
            observed_at_ms: self.turn_lifecycle.live_generation_speed_started_at_ms,
        })
    }

    pub(crate) fn set_live_generation_speed(&mut self, speed: Option<f64>) {
        self.bottom_pane.set_recent_generation_speed(speed);
    }

    pub(crate) fn completion_cell(
        &mut self,
        turn: &Turn,
        replay_kind: Option<ReplayKind>,
    ) -> Option<history_cell::FinalMessageSeparator> {
        if self
            .turn_lifecycle
            .rendered_completion_turn_ids
            .contains(&turn.id)
        {
            return None;
        }
        let elapsed_seconds = turn
            .duration_ms
            .and_then(|duration| u64::try_from(duration).ok())
            .map(|duration| duration / 1_000)
            .or_else(|| {
                if replay_kind.is_none() {
                    self.bottom_pane
                        .status_elapsed()
                        .map(|elapsed| elapsed.as_secs())
                } else {
                    None
                }
            });
        let completed_at = turn
            .completed_at
            .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, /*nsecs*/ 0))
            .map(|timestamp| timestamp.with_timezone(&Local))
            .or_else(|| replay_kind.is_none().then(Local::now));
        if completed_at.is_none() && elapsed_seconds.is_none() {
            return None;
        }
        self.turn_lifecycle
            .rendered_completion_turn_ids
            .insert(turn.id.clone());
        let generation_speed = (turn.status == TurnStatus::Completed && turn.error.is_none())
            .then(|| {
                self.thread_id.and_then(|thread_id| {
                    self.generation_speeds
                        .get(&(thread_id, turn.id.clone()))
                        .and_then(|record| {
                            record.for_completion(
                                turn.started_at,
                                turn.completed_at,
                                turn.duration_ms,
                            )
                        })
                })
            })
            .flatten();
        let has_record = self.thread_id.is_some_and(|thread_id| {
            self.generation_speeds
                .contains_key(&(thread_id, turn.id.clone()))
        });
        let reason = if turn.status != TurnStatus::Completed || turn.error.is_some() {
            "turn_not_successful"
        } else if !has_record {
            "record_unavailable"
        } else if generation_speed.is_none() {
            "completion_identity_mismatch"
        } else if !generation_speed.is_some_and(|speed| speed.is_finite() && speed >= 0.05) {
            "below_display_threshold_or_invalid"
        } else {
            "displayed"
        };
        tracing::info!(
            event.name = "codex.generation_speed.display",
            thread_id = ?self.thread_id,
            turn_id = %turn.id,
            status = ?turn.status,
            has_error = turn.error.is_some(),
            has_record,
            reason,
            started_at = ?turn.started_at,
            completed_at = ?turn.completed_at,
            duration_ms = ?turn.duration_ms,
            ?generation_speed,
            "generation speed display evaluated"
        );
        let cell = history_cell::FinalMessageSeparator::new(
            elapsed_seconds,
            /*runtime_metrics*/ None,
        )
        .with_generation_speed(generation_speed);
        Some(match completed_at {
            Some(completed_at) => {
                cell.with_completed_at(completed_at, crate::clock_format::ClockFormat::system())
            }
            None => cell,
        })
    }
}
