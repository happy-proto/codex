//! Incremental display of a retained conversation, with history scoped to its pane.

use super::resize_reflow::trailing_run_start;
use super::*;
use crate::app_event_sender::SideProjectionRoute;

impl App {
    pub(super) fn deliver_side_projection_notification(
        &mut self,
        thread_id: ThreadId,
        notification: &ServerNotification,
    ) {
        if self.current_displayed_thread_id() == Some(thread_id)
            || self.abandoned_side_threads.contains(&thread_id)
            || matches!(
                notification,
                ServerNotification::ThreadRealtimeSdp(_)
                    | ServerNotification::ThreadRealtimeStarted(_)
                    | ServerNotification::ThreadRealtimeTranscriptDelta(_)
                    | ServerNotification::ThreadRealtimeTranscriptDone(_)
                    | ServerNotification::ThreadRealtimeError(_)
                    | ServerNotification::ThreadRealtimeClosed(_)
                    | ServerNotification::ThreadRealtimeItemAdded(_)
                    | ServerNotification::ThreadRealtimeItemStarted(_)
                    | ServerNotification::ThreadRealtimeItemTranscriptDelta(_)
                    | ServerNotification::ThreadRealtimeItemCompleted(_)
                    | ServerNotification::ThreadRealtimeOutputAudioDelta(_)
            )
        {
            return;
        }
        if let Some(companion) = self.side_companions.get_mut(&thread_id)
            && let Some(widget) = companion.projection.as_mut()
        {
            // These are live events for the original widget, not snapshot replay.
            // Sender scoping and queue suppression provide the controller boundary.
            widget.handle_server_notification(notification.clone(), None);
            widget.sync_side_activity(&mut companion.bottom);
            if let ServerNotification::ItemCompleted(completed) = notification
                && matches!(
                    completed.item.payload,
                    ThreadItemPayload::CommandExecution { .. }
                )
            {
                // Follow already queued history inserts. The widget defers exec ends while an
                // answer streams, but the parked command must stop looking active immediately.
                self.app_event_tx.send(AppEvent::SideProjectionEvent {
                    route: companion.route,
                    event: Box::new(AppEvent::SideCommandCompleted(
                        completed.item.payload.clone(),
                    )),
                });
            }
        }
    }

    pub(super) fn tick_side_projections(&mut self, frames: &tui::FrameRequester) {
        for (thread_id, companion) in &mut self.side_companions {
            let widget = companion.projection.as_deref_mut().or_else(|| {
                self.background_voice
                    .as_deref_mut()
                    .filter(|owner| owner.thread_id() == Some(*thread_id))
            });
            if let Some(widget) = widget {
                if companion.commit_ticks {
                    widget.on_commit_tick();
                    frames.schedule_frame_in(COMMIT_ANIMATION_TICK);
                }
                widget.pre_draw_tick();
                widget.sync_side_activity(&mut companion.bottom);
            }
        }
    }

    pub(super) fn handle_side_projection_event(
        &mut self,
        route: SideProjectionRoute,
        event: AppEvent,
    ) {
        let Some(companion) = self
            .side_companions
            .get_mut(&route.thread_id)
            .filter(|companion| companion.route == route)
        else {
            return;
        };
        let mut consolidated = false;
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                if let Some(exec) = cell.as_any().downcast_ref::<crate::exec_cell::ExecCell>()
                    && !exec.is_active()
                    && exec.iter_calls().all(|call| {
                        companion.cells.iter().any(|existing| {
                            existing
                                .as_any()
                                .downcast_ref::<crate::exec_cell::ExecCell>()
                                .is_some_and(|existing| {
                                    existing.iter_calls().any(|known| {
                                        known.call_id == call.call_id && known.output.is_some()
                                    })
                                })
                        })
                    })
                {
                    // The stream's deferred exec end has caught up with its live revision.
                    return;
                }
                companion.cells.push(Arc::from(cell));
            }
            AppEvent::SideCommandCompleted(ThreadItemPayload::CommandExecution {
                id,
                status,
                source,
                aggregated_output,
                exit_code,
                duration_ms,
                ..
            }) => {
                if let Some((index, exec)) =
                    companion
                        .cells
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(index, cell)| {
                            let exec =
                                cell.as_any().downcast_ref::<crate::exec_cell::ExecCell>()?;
                            exec.iter_calls()
                                .any(|call| call.call_id == id)
                                .then(|| (index, exec.clone()))
                        })
                {
                    let mut exec = exec;
                    exec.complete_call(
                        &id,
                        crate::exec_cell::CommandOutput::from_execution(
                            status,
                            exit_code,
                            source,
                            aggregated_output,
                        ),
                        std::time::Duration::from_millis(
                            duration_ms.unwrap_or_default().max(0) as u64
                        ),
                    );
                    let cell: Arc<dyn HistoryCell> = Arc::new(exec);
                    companion
                        .view
                        .replace_range(&companion.cells, index..index + 1, &cell);
                    companion.cells[index] = cell;
                }
            }
            AppEvent::ConsolidateAgentMessage {
                source,
                copy_source,
                cwd,
                inline_visualization_context,
                deferred_history_cell,
                ..
            } => {
                if let Some(cell) = deferred_history_cell {
                    companion.cells.push(Arc::from(cell));
                    companion.view.sync_history_tail(&companion.cells);
                }
                let cell = Arc::new(
                    history_cell::AgentMarkdownCell::new_with_inline_visualizations(
                        source,
                        &cwd,
                        inline_visualization_context,
                    )
                    .with_copy_source(copy_source),
                );
                replace_side_stream::<history_cell::AgentMessageCell>(companion, cell);
                consolidated = true;
            }
            AppEvent::ConsolidateProposedPlan(source) => {
                let cwd = companion
                    .projection
                    .as_ref()
                    .map(|widget| widget.config_ref().cwd.as_path())
                    .or_else(|| {
                        self.background_voice
                            .as_ref()
                            .filter(|owner| owner.thread_id() == Some(route.thread_id))
                            .map(|owner| owner.config_ref().cwd.as_path())
                    })
                    .unwrap_or(self.config.cwd.as_path());
                let cell = Arc::new(history_cell::new_proposed_plan(source, cwd));
                replace_side_stream::<history_cell::ProposedPlanStreamCell>(companion, cell);
                consolidated = true;
            }
            AppEvent::StartCommitAnimation => companion.commit_ticks = true,
            AppEvent::StopCommitAnimation => companion.commit_ticks = false,
            AppEvent::CommitRealtimeTranscriptHistory => {
                if let Some(owner) = companion.projection.as_deref_mut().or_else(|| {
                    self.background_voice
                        .as_deref_mut()
                        .filter(|owner| owner.thread_id() == Some(route.thread_id))
                }) {
                    companion.cells.extend(
                        owner
                            .take_realtime_transcript_history()
                            .into_iter()
                            .map(Arc::from),
                    );
                }
            }
            _ => return,
        }
        companion.view.sync_history_tail(&companion.cells);
        if consolidated {
            if let Some(widget) = companion.projection.as_deref_mut().or_else(|| {
                self.background_voice
                    .as_deref_mut()
                    .filter(|owner| owner.thread_id() == Some(route.thread_id))
            }) {
                widget.note_stream_consolidation_completed();
            }
        }
    }
}

fn replace_side_stream<T: 'static>(
    companion: &mut side::SideCompanion,
    cell: Arc<dyn HistoryCell>,
) {
    let start = trailing_run_start::<T>(&companion.cells);
    let end = companion.cells.len();
    if start < end {
        companion
            .view
            .replace_group(&companion.cells, start..end, &cell);
        companion.cells.splice(start..end, [cell]);
    } else {
        companion.cells.push(cell);
    }
}
