//! Live generation speed for the active turn.
//!
//! Token usage reaches the client only after a sampling step finishes, so a live
//! rate has to be derived from the streamed content deltas. Deltas are measured
//! with the same byte heuristic upstream uses to estimate context, then rescaled
//! by the ratio observed on steps whose reported usage covers exactly the
//! streamed content.
//!
//! Calibration compares the step's non-reasoning output tokens against the
//! streamed message and plan deltas. Reasoning is excluded from both sides
//! because its tokens are reported even when its text never reaches the client,
//! and steps that streamed a tool call are skipped because tool arguments are
//! output tokens the client never sees.
//!
//! Rates therefore describe the speed of the delta stream, not the billed token
//! count. The turn average additionally excludes boundaries such as tool
//! execution, so a turn that spends most of its wall time running commands does
//! not report that time as generation.

use std::collections::VecDeque;
use std::ops::RangeInclusive;
use std::time::Duration;
use std::time::Instant;

use super::*;

/// Whether a started item means the turn handed over to a tool.
///
/// Tool-call arguments are generated as output tokens but stream only to the
/// model client, so a started tool call also marks the step whose reported usage
/// can no longer be attributed to the streamed deltas alone.
pub(super) fn item_starts_tool_call(item: &ThreadItem) -> bool {
    matches!(
        item,
        ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
            | ThreadItem::WebSearch(_)
            | ThreadItem::ImageGeneration(_)
            | ThreadItem::CollabAgentToolCall { .. }
    )
}

/// Deltas older than this are dropped from the live rate.
const LIVE_WINDOW: Duration = Duration::from_secs(5);
/// Newest delta older than this stops the live rate from being reported.
const LIVE_STALE: Duration = Duration::from_millis(1_500);
/// Shortest decode time a rate may be divided by.
const MIN_ACTIVE_TIME: Duration = Duration::from_millis(250);
/// Decode time assumed for a burst too short to measure on its own.
const SHORT_BURST_TIME: Duration = Duration::from_millis(1_000);
/// Upstream estimates context with 4 bytes per token; deltas use the same scale.
const BYTES_PER_TOKEN: f64 = 4.0;
/// Reported usage only matches streamed content inside this range.
const CALIBRATION_RANGE: RangeInclusive<f64> = 0.25..=4.0;

/// Which stream a delta belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeltaKind {
    /// Message or plan text, which calibration compares against reported usage.
    Content,
    /// Reasoning text, which is reported as a subset of the output tokens.
    Reasoning,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    at: Instant,
    tokens: f64,
}

/// One uninterrupted run of deltas, bounded by turn, item, tool, or step boundaries.
#[derive(Clone, Copy, Debug, Default)]
struct Segment {
    estimated_tokens: f64,
    first_at: Option<Instant>,
    last_at: Option<Instant>,
    deltas: u32,
}

impl Segment {
    /// Decode time covered by the segment.
    ///
    /// A segment that streamed a single delta has no measured gap, so it is
    /// excluded from time accounting instead of being charged a guessed
    /// duration; its tokens drop out of the turn average for the same reason.
    fn active_time(&self) -> Option<Duration> {
        let (Some(first_at), Some(last_at)) = (self.first_at, self.last_at) else {
            return None;
        };
        (self.deltas > 1).then(|| {
            last_at
                .saturating_duration_since(first_at)
                .max(MIN_ACTIVE_TIME)
        })
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Reported usage for one finished sampling step, awaiting attribution.
#[derive(Clone, Copy, Debug)]
struct PendingStep {
    estimated_tokens: f64,
    /// Output tokens that were not reasoning tokens.
    visible_tokens: u64,
    /// A tool call was reported for this step, whose arguments never stream here.
    tool_call: bool,
}

/// Measures generation speed from streamed deltas.
#[derive(Debug, Default)]
pub(super) struct TokenSpeedTracker {
    window: VecDeque<Sample>,
    segment: Segment,
    /// Message and plan deltas seen since the last reported step, across item boundaries.
    step_estimated_tokens: f64,
    /// Content tokens accumulated for the turn, before calibration.
    turn_tokens: f64,
    /// Decode time accumulated for the turn.
    turn_active: Duration,
    /// Whether the live window is worth maintaining for the status line.
    live_enabled: bool,
    pending: Option<PendingStep>,
    /// A tool call reported after the pending step's usage belongs to that step.
    pending_tool_call: bool,
    /// A tool call reported while a step is streaming belongs to that step.
    step_tool_call: bool,
    /// The step in progress was only partly observed, so it cannot calibrate.
    partial_step: bool,
    /// Reported-to-estimated ratio learned from clean steps.
    calibration: Option<f64>,
    /// Cumulative output tokens of the last attributed step.
    ///
    /// Totals only grow, so comparing them separates a finished step from the
    /// repeated usage notifications that rate-limit refreshes carry mid-step.
    last_total_output_tokens: u64,
    /// Cumulative reasoning tokens of the last attributed step.
    last_total_reasoning_tokens: u64,
}

impl TokenSpeedTracker {
    /// Drop per-turn state, keeping the calibrated language ratio.
    pub(super) fn start_turn(&mut self) {
        self.window.clear();
        self.segment.reset();
        self.step_estimated_tokens = 0.0;
        self.turn_tokens = 0.0;
        self.turn_active = Duration::ZERO;
        self.pending = None;
        self.pending_tool_call = false;
        self.step_tool_call = false;
        self.partial_step = false;
    }

    /// Record one streamed delta; returns the live rate when one can be reported.
    pub(super) fn record_delta(
        &mut self,
        delta: &str,
        kind: DeltaKind,
        now: Instant,
    ) -> Option<f64> {
        // A delta that starts a new step proves the previous step streamed no tool
        // call, which is what makes its reported usage comparable to our estimate.
        self.commit_pending_step();

        if !delta.is_empty() {
            let tokens = delta.len() as f64 / BYTES_PER_TOKEN;
            self.segment.estimated_tokens += tokens;
            self.segment.first_at.get_or_insert(now);
            self.segment.last_at = Some(now);
            self.segment.deltas = self.segment.deltas.saturating_add(1);
            if kind == DeltaKind::Content {
                self.step_estimated_tokens += tokens;
            }
            if self.live_enabled {
                self.window.push_back(Sample { at: now, tokens });
                self.drop_expired_samples(now);
            }
        }

        self.live_rate(now)
    }

    /// Finish the current segment without ending the turn or the step.
    ///
    /// Callers use this where the model stops producing streamed content but the
    /// step is not over yet: item completion and abnormal turn termination.
    /// Item boundaries only split timing into segments, so the step estimate
    /// used for calibration survives them.
    pub(super) fn end_segment(&mut self) {
        self.close_segment();
    }

    /// Note that a tool call started; its streamed arguments never reach us.
    ///
    /// A call reported while the streaming step already produced content belongs
    /// to that step. Otherwise it can belong either to the step that just reported
    /// usage or to the next step before its first delta, so both are excluded
    /// instead of guessing which one lost its arguments to the tool.
    pub(super) fn note_tool_call(&mut self) {
        self.close_segment();
        self.step_tool_call = true;
        if self.step_estimated_tokens <= 0.0 {
            self.pending_tool_call = true;
        }
    }

    /// Note that replayed history means the step in progress is only partly seen.
    ///
    /// Replayed deltas are not sampled, so the resumed step's reported usage would
    /// be compared against an estimate that misses everything streamed before the
    /// switch.
    pub(super) fn note_partially_observed_step(&mut self) {
        self.partial_step = true;
    }

    /// Drop everything streamed for a step that was abandoned mid-flight.
    ///
    /// The retry restarts the request, so the earlier deltas neither belong to the
    /// step that will report usage nor to its decode time.
    pub(super) fn discard_partial_step(&mut self) {
        self.partial_step = true;
        self.segment.reset();
        self.window.clear();
        self.step_estimated_tokens = 0.0;
        self.step_tool_call = false;
    }

    /// Enable or disable live-rate tracking for the status line.
    pub(super) fn set_live_enabled(&mut self, enabled: bool) {
        self.live_enabled = enabled;
        if !enabled {
            self.window.clear();
        }
    }

    /// Attribute the cumulative usage the app server reported for one finished step.
    ///
    /// The session totals only grow, which separates a finished step from the
    /// repeated notifications that rate-limit refreshes carry mid-step. Lower
    /// totals mean a new session replaced this one, so the baseline is reset
    /// without attributing anything.
    pub(super) fn note_step_usage(
        &mut self,
        total_output_tokens: u64,
        total_reasoning_tokens: u64,
    ) {
        if total_output_tokens < self.last_total_output_tokens {
            // A new session starts from lower totals; nothing here belongs to the
            // step that was streaming.
            self.close_segment();
            self.pending = None;
            self.pending_tool_call = false;
            self.step_tool_call = false;
            self.step_estimated_tokens = 0.0;
            self.partial_step = false;
            self.last_total_output_tokens = total_output_tokens;
            self.last_total_reasoning_tokens = total_reasoning_tokens;
            return;
        }
        if total_output_tokens == self.last_total_output_tokens {
            // Rate-limit refreshes re-report the previous step's totals. The step
            // that is streaming now keeps its segment and its estimate.
            return;
        }
        self.close_segment();
        // Attribute the step that was pending before this one replaces it.
        self.commit_pending_step();
        let output_tokens = total_output_tokens - self.last_total_output_tokens;
        let reasoning_tokens = total_reasoning_tokens
            .saturating_sub(self.last_total_reasoning_tokens)
            .min(output_tokens);
        self.last_total_output_tokens = total_output_tokens;
        self.last_total_reasoning_tokens = total_reasoning_tokens;
        if std::mem::take(&mut self.partial_step) {
            self.step_tool_call = false;
        } else {
            self.pending = Some(PendingStep {
                estimated_tokens: self.step_estimated_tokens,
                visible_tokens: output_tokens - reasoning_tokens,
                tool_call: std::mem::take(&mut self.step_tool_call),
            });
        }
        self.step_estimated_tokens = 0.0;
    }

    /// Finish the turn and return its average rate.
    pub(super) fn finish_turn(&mut self) -> Option<f64> {
        self.close_segment();
        self.commit_pending_step();
        self.window.clear();
        self.turn_rate()
    }

    /// Rate over the recent delta window, or `None` while nothing is streaming.
    pub(super) fn live_rate(&self, now: Instant) -> Option<f64> {
        if !self.live_enabled {
            return None;
        }
        // A single delta cannot measure a rate: it would report the whole burst
        // against the assumed short-burst duration.
        if self.window.len() < 2 {
            return None;
        }
        let newest_at = self.window.back()?.at;
        if now.saturating_duration_since(newest_at) > LIVE_STALE {
            return None;
        }
        let tokens: f64 = self.window.iter().map(|sample| sample.tokens).sum();
        if tokens <= 0.0 {
            return None;
        }
        let active = self.window_active_time(now);
        if active < MIN_ACTIVE_TIME {
            return None;
        }
        Some(tokens / active.as_secs_f64() * self.calibration.unwrap_or(1.0))
    }

    /// Fold the in-flight segment into the turn totals and drop its samples.
    fn close_segment(&mut self) {
        let segment = std::mem::take(&mut self.segment);
        if let Some(active) = segment.active_time() {
            self.turn_tokens += segment.estimated_tokens;
            self.turn_active += active;
        }
        self.window.clear();
    }

    /// Turn reported usage into a calibration ratio for later steps.
    fn commit_pending_step(&mut self) {
        let Some(pending) = self.pending.take() else {
            // Keep the flags: a tool call can be reported before the usage of the
            // step it ends.
            return;
        };
        if std::mem::take(&mut self.pending_tool_call) || pending.tool_call {
            // Tool arguments were generated as output tokens but never streamed here.
            return;
        }
        if pending.visible_tokens == 0 || pending.estimated_tokens <= 0.0 {
            return;
        }
        let observed = (pending.visible_tokens as f64 / pending.estimated_tokens)
            .clamp(*CALIBRATION_RANGE.start(), *CALIBRATION_RANGE.end());
        self.calibration = Some(match self.calibration {
            Some(previous) => (previous + observed) / 2.0,
            None => observed,
        });
    }

    fn turn_rate(&self) -> Option<f64> {
        if self.turn_active < MIN_ACTIVE_TIME || self.turn_tokens <= 0.0 {
            return None;
        }
        let tokens = self.turn_tokens * self.calibration.unwrap_or(1.0);
        Some(tokens / self.turn_active.as_secs_f64())
    }

    /// Decode time inside the live window.
    ///
    /// Gaps between kept samples count as decode time, so a stalled stream reports
    /// a lower rate instead of keeping the last burst rate. Bursts shorter than a
    /// second report the rate of their first second instead of an unbounded peak.
    /// Callers pass at least two samples.
    fn window_active_time(&self, now: Instant) -> Duration {
        let Some(last) = self.window.back() else {
            return Duration::ZERO;
        };
        let mut active = now.saturating_duration_since(last.at);
        let mut previous: Option<Instant> = None;
        for sample in &self.window {
            if let Some(previous) = previous {
                active += sample.at.saturating_duration_since(previous);
            }
            previous = Some(sample.at);
        }
        active.max(SHORT_BURST_TIME)
    }

    fn drop_expired_samples(&mut self, now: Instant) {
        while self
            .window
            .front()
            .is_some_and(|sample| now.saturating_duration_since(sample.at) > LIVE_WINDOW)
        {
            self.window.pop_front();
        }
    }

    #[cfg(test)]
    pub(super) fn calibration(&self) -> Option<f64> {
        self.calibration
    }
}

impl ChatWidget {
    /// Start measuring a new turn.
    pub(super) fn start_token_speed_turn(&mut self) {
        self.token_speed.start_turn();
        self.refresh_live_token_speed();
    }

    /// Record a streamed delta and refresh the live rate on the status line.
    pub(super) fn note_generation_delta(&mut self, delta: &str, kind: DeltaKind) {
        let rate = self.token_speed.record_delta(delta, kind, Instant::now());
        self.publish_live_token_speed(rate);
    }

    /// Note that replayed history makes the step in progress only partly observed.
    pub(super) fn note_partially_observed_step(&mut self) {
        self.token_speed.note_partially_observed_step();
    }

    /// Drop a partly streamed step after a retryable stream error.
    ///
    /// The retry restarts the request, so the earlier deltas neither belong to the
    /// step that will report usage nor to its decode time.
    pub(super) fn note_retried_step(&mut self) {
        self.token_speed.discard_partial_step();
        self.refresh_live_token_speed();
    }

    /// Track the live window only while the status line shows the speed item.
    pub(super) fn set_token_speed_live_enabled(&mut self, enabled: bool) {
        self.token_speed.set_live_enabled(enabled);
    }

    /// Record the cumulative usage the app server reported for a finished step.
    pub(super) fn note_reported_token_usage(
        &mut self,
        total_output_tokens: i64,
        total_reasoning_output_tokens: i64,
    ) {
        self.token_speed.note_step_usage(
            u64::try_from(total_output_tokens).unwrap_or_default(),
            u64::try_from(total_reasoning_output_tokens).unwrap_or_default(),
        );
    }

    /// Stop the live rate at a generation boundary.
    pub(super) fn end_generation_segment(&mut self) {
        self.token_speed.end_segment();
        self.refresh_live_token_speed();
    }

    /// Mark a tool call, whose arguments are not streamed to the client.
    pub(super) fn note_tool_call_started(&mut self) {
        self.token_speed.note_tool_call();
    }

    /// Finish the turn, returning the average rate for the completion metadata.
    pub(super) fn finish_token_speed_turn(&mut self) -> Option<f64> {
        let average = self.token_speed.finish_turn();
        self.refresh_live_token_speed();
        average
    }

    /// Value of the `tokens-per-second` status-line item.
    pub(super) fn live_token_speed_label(&self) -> Option<String> {
        let rate = self.token_speed.live_rate(Instant::now())?;
        Some(format!(
            "{} tok/s",
            crate::token_usage::format_token_rate(rate)
        ))
    }

    /// Recompute the live rate and publish it to the status line.
    pub(super) fn refresh_live_token_speed(&mut self) {
        let rate = self.token_speed.live_rate(Instant::now());
        self.publish_live_token_speed(rate);
    }

    /// Push a measured rate to the status line, or retract the previous value.
    ///
    /// The status line stores rendered text, so the value has to be pushed again
    /// whenever it changes and retracted when deltas stop. Retraction is driven
    /// by a scheduled frame that re-enters here through `pre_draw_tick`. While a
    /// rate is live a frame always stays pending: drop it for an unchanged value
    /// and an unrelated redraw could consume the only deadline and leave the
    /// number on screen. The same guard keeps the per-delta path free when the
    /// item is not configured.
    fn publish_live_token_speed(&mut self, rate: Option<f64>) {
        if !self.status_line_uses_token_speed {
            self.token_speed_label = None;
            return;
        }
        let label =
            rate.map(|rate| format!("{} tok/s", crate::token_usage::format_token_rate(rate)));
        if label != self.token_speed_label {
            self.token_speed_label = label;
            self.refresh_status_line_only();
        }
        if self.token_speed_label.is_some() {
            self.frame_requester.schedule_frame_in(LIVE_STALE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tracker with live-rate tracking enabled, as when the status line shows it.
    fn live_tracker() -> TokenSpeedTracker {
        let mut tracker = TokenSpeedTracker::default();
        tracker.set_live_enabled(true);
        tracker
    }

    fn delta(tracker: &mut TokenSpeedTracker, text: &str, at: Instant) -> Option<f64> {
        tracker.record_delta(text, DeltaKind::Content, at)
    }

    #[test]
    fn disabled_live_tracking_keeps_the_turn_average() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        delta(&mut tracker, &"a".repeat(400), start);
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );

        assert_eq!(tracker.live_rate(start + Duration::from_secs(4)), None);

        let average = tracker.finish_turn().expect("turn average");

        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn live_rate_tracks_recent_deltas() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // 400 bytes over 2s is 100 estimated tokens over 2s.
        assert_eq!(delta(&mut tracker, &"a".repeat(200), start), None);
        let rate = delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(2),
        )
        .expect("two samples produce a live rate");

        assert!((rate - 50.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn live_rate_disappears_once_deltas_stop() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(&mut tracker, "b", start + Duration::from_millis(500));

        assert!(
            tracker
                .live_rate(start + Duration::from_millis(1_000))
                .is_some()
        );
        assert!(tracker.live_rate(start + Duration::from_secs(5)).is_none());
    }

    #[test]
    fn calibration_rescales_estimates_from_clean_steps() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(2),
        );
        // The step streamed 100 estimated tokens but reported 200.
        tracker.note_step_usage(/*output_tokens*/ 200, /*reasoning_tokens*/ 0);
        let next = start + Duration::from_secs(3);
        delta(&mut tracker, &"a".repeat(200), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(200),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        // 50 estimated tokens per second, doubled by the calibration ratio.
        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn calibration_survives_an_item_boundary_inside_the_step() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // A commentary item completes before the step reports its usage.
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(1),
        );
        tracker.end_segment();
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );
        // 300 estimated tokens streamed across both items, 600 reported.
        tracker.note_step_usage(/*output_tokens*/ 600, /*reasoning_tokens*/ 0);
        let next = start + Duration::from_secs(5);
        delta(&mut tracker, &"a".repeat(200), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(200),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn tool_calls_do_not_calibrate_generation_speed() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(2),
        );
        // The tool call can be reported before the usage of the step it ends.
        tracker.note_tool_call();
        tracker.note_step_usage(/*output_tokens*/ 900, /*reasoning_tokens*/ 0);
        let next = start + Duration::from_secs(10);
        delta(&mut tracker, &"a".repeat(200), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(200),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        assert!((rate - 50.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn tool_calls_reported_before_the_rest_of_the_step_still_exclude_it() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // The tool call is reported first, then the response streams more text.
        tracker.note_tool_call();
        delta(&mut tracker, &"a".repeat(400), start);
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 900, /*total_reasoning_tokens*/ 0,
        );
        let next = start + Duration::from_secs(4);
        delta(&mut tracker, &"a".repeat(400), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(400),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn tool_calls_during_a_step_exclude_only_that_step() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // Step A: 100 estimated tokens, 100 reported, no tool call.
        delta(&mut tracker, &"a".repeat(400), start);
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        // Step B streams content and then requests a search of its own.
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        tracker.note_tool_call();
        tracker.note_step_usage(
            /*total_output_tokens*/ 1_000, /*total_reasoning_tokens*/ 0,
        );
        // Step C streams another 100 estimated tokens.
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );
        let rate = delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(6),
        )
        .expect("live rate");

        // Step A calibrated the estimate; step B's tool arguments never reached
        // the estimate, so calibrating it would have reported 250 instead.
        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn a_partially_observed_step_does_not_calibrate() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // Only the tail of this step was sampled after switching back to the thread.
        delta(&mut tracker, &"a".repeat(400), start);
        tracker.note_partially_observed_step();
        tracker.note_step_usage(
            /*total_output_tokens*/ 400, /*total_reasoning_tokens*/ 0,
        );
        let next = start + Duration::from_secs(2);
        delta(&mut tracker, &"a".repeat(400), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(400),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        // Calibrating the partial step would have reported 400 instead.
        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn a_retried_step_drops_its_partial_output_and_backoff() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        tracker.start_turn();
        // The first attempt streams 200 estimated tokens over 1s and then breaks.
        delta(&mut tracker, &"a".repeat(400), start);
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(1),
        );
        tracker.discard_partial_step();
        // After a 20s backoff the retry streams 200 estimated tokens and reports them.
        let retry = start + Duration::from_secs(20);
        delta(&mut tracker, &"a".repeat(400), retry);
        delta(
            &mut tracker,
            &"a".repeat(400),
            retry + Duration::from_secs(4),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        // Counting the abandoned attempt would report 80, and counting the backoff
        // as well would report about 18.
        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn repeated_usage_notifications_keep_the_segment_together() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        tracker.start_turn();
        delta(&mut tracker, &"a".repeat(400), start);
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );
        // A refresh reports the same totals while the second step is mid-segment.
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(10),
        );

        let average = tracker.finish_turn().expect("turn average");

        // Both segments keep two deltas: 400 estimated tokens over 8s. Splitting
        // the second segment would leave single-delta segments and report 100.
        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn reasoning_tokens_are_excluded_from_calibration() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(2),
        );
        // The step reported 200 output tokens, 100 of which were reasoning; only
        // the 100 streamed tokens are comparable to the estimate.
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 100,
        );
        let next = start + Duration::from_secs(3);
        delta(&mut tracker, &"a".repeat(200), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(200),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        // The estimate was never rescaled, so the raw rate stands.
        assert!((rate - 50.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn repeated_usage_notifications_do_not_reset_the_step() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        // First step: 100 estimated tokens, 100 reported.
        delta(&mut tracker, &"a".repeat(400), start);
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        // A rate-limit refresh re-reports the same totals while the next step streams.
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        // The second step streams another 200 estimated tokens and reports them.
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 300, /*total_reasoning_tokens*/ 0,
        );

        let next = start + Duration::from_secs(6);
        delta(&mut tracker, &"a".repeat(400), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(400),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        // Both steps reported one token per estimated token, so the estimate is
        // unchanged: a refresh that reset the step would have doubled it.
        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn a_restarted_session_resets_the_reported_baseline() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(400), start);
        tracker.note_step_usage(
            /*total_output_tokens*/ 5_000, /*total_reasoning_tokens*/ 0,
        );

        // Switching threads restarts the totals, which must not look like a step
        // whose reported usage is the whole previous session.
        tracker.start_turn();
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(2),
        );
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        let next = start + Duration::from_secs(4);
        delta(&mut tracker, &"a".repeat(400), next);
        let rate = delta(
            &mut tracker,
            &"a".repeat(400),
            next + Duration::from_secs(2),
        )
        .expect("live rate");

        // The previous session's totals never reach the estimate, so no scaling
        // is applied: 200 estimated tokens over 2s.
        assert!((rate - 100.0).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn turn_average_excludes_tool_waiting() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        tracker.start_turn();
        // First step: 200 estimated tokens over 4s, then a 30s tool call.
        delta(&mut tracker, &"a".repeat(400), start);
        delta(
            &mut tracker,
            &"a".repeat(400),
            start + Duration::from_secs(4),
        );
        tracker.note_tool_call();
        // Second step: another 200 estimated tokens over 4s.
        let resumed = start + Duration::from_secs(34);
        delta(&mut tracker, &"a".repeat(400), resumed);
        delta(
            &mut tracker,
            &"a".repeat(400),
            resumed + Duration::from_secs(4),
        );

        let average = tracker.finish_turn().expect("turn average");

        // 400 estimated tokens over 8s of decode, not over the 38s wall time.
        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn start_turn_drops_the_previous_window() {
        let mut tracker = live_tracker();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(1),
        );

        tracker.start_turn();

        assert!(tracker.live_rate(start + Duration::from_secs(1)).is_none());
        assert_eq!(tracker.live_rate(start + Duration::from_secs(2)), None);
    }
}
