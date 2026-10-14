//! Live generation speed for the active turn.
//!
//! Token usage reaches the client only after a sampling step finishes, so a live
//! rate has to be derived from the streamed content deltas. Deltas are measured
//! with the same byte heuristic upstream uses to estimate context, then rescaled
//! by the ratio observed on steps whose reported usage covers exactly the
//! streamed content. Steps that stream tool-call arguments (never forwarded to
//! the client) or hidden reasoning would inflate that ratio, so they are
//! excluded from calibration.
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
    Content,
    Reasoning,
}

impl DeltaKind {
    fn is_reasoning(self) -> bool {
        self == Self::Reasoning
    }
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
    reported_tokens: u64,
    reasoning_tokens: u64,
    reasoning_deltas: bool,
}

/// Measures generation speed from streamed deltas.
#[derive(Debug, Default)]
pub(super) struct TokenSpeedTracker {
    window: VecDeque<Sample>,
    segment: Segment,
    /// Deltas seen since the last reported step, across item boundaries.
    step_estimated_tokens: f64,
    step_reasoning_deltas: bool,
    /// Content tokens accumulated for the turn, before calibration.
    turn_tokens: f64,
    /// Decode time accumulated for the turn.
    turn_active: Duration,
    pending: Option<PendingStep>,
    /// A tool call started after the step that is awaiting attribution.
    tool_call_after_step: bool,
    /// Reported-to-estimated ratio learned from clean steps.
    calibration: Option<f64>,
    /// Reported tokens of the last attributed step, used to ignore repeats.
    last_reported_tokens: Option<u64>,
}

impl TokenSpeedTracker {
    /// Drop per-turn state, keeping the calibrated language ratio.
    pub(super) fn start_turn(&mut self) {
        self.window.clear();
        self.segment.reset();
        self.step_estimated_tokens = 0.0;
        self.step_reasoning_deltas = false;
        self.turn_tokens = 0.0;
        self.turn_active = Duration::ZERO;
        self.pending = None;
        self.tool_call_after_step = false;
        self.last_reported_tokens = None;
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
            self.window.push_back(Sample { at: now, tokens });
            self.segment.estimated_tokens += tokens;
            self.segment.first_at.get_or_insert(now);
            self.segment.last_at = Some(now);
            self.segment.deltas = self.segment.deltas.saturating_add(1);
            self.step_estimated_tokens += tokens;
            self.step_reasoning_deltas |= kind.is_reasoning();
            self.drop_expired_samples(now);
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
    pub(super) fn note_tool_call(&mut self) {
        self.close_segment();
        if self.pending.is_some() || self.step_estimated_tokens > 0.0 {
            self.tool_call_after_step = true;
        }
    }

    /// Attribute the usage the app server reported for one finished step.
    pub(super) fn note_step_usage(&mut self, output_tokens: u64, reasoning_tokens: u64) {
        self.close_segment();
        let reported_tokens = output_tokens.saturating_add(reasoning_tokens);
        if reported_tokens == 0 || Some(reported_tokens) == self.last_reported_tokens {
            return;
        }
        self.last_reported_tokens = Some(reported_tokens);
        self.pending = Some(PendingStep {
            estimated_tokens: self.step_estimated_tokens,
            reported_tokens,
            reasoning_tokens,
            reasoning_deltas: self.step_reasoning_deltas,
        });
        self.step_estimated_tokens = 0.0;
        self.step_reasoning_deltas = false;
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
        let invalidated = std::mem::take(&mut self.tool_call_after_step);
        let Some(pending) = self.pending.take() else {
            return;
        };
        if invalidated {
            // Tool arguments were generated as output tokens but never streamed here.
            return;
        }
        if pending.reasoning_tokens > 0 && !pending.reasoning_deltas {
            // Hidden reasoning is reported but not streamed.
            return;
        }
        if pending.estimated_tokens <= 0.0 {
            return;
        }
        let observed = (pending.reported_tokens as f64 / pending.estimated_tokens)
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
    fn window_active_time(&self, now: Instant) -> Duration {
        let (Some(first), Some(last)) = (self.window.front(), self.window.back()) else {
            return Duration::ZERO;
        };
        if self.window.len() == 1 {
            return now
                .saturating_duration_since(first.at)
                .clamp(MIN_ACTIVE_TIME, SHORT_BURST_TIME);
        }
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
}

impl ChatWidget {
    /// Start measuring a new turn.
    pub(super) fn start_token_speed_turn(&mut self) {
        self.token_speed.start_turn();
        self.refresh_status_line();
    }

    /// Record a streamed delta and refresh the live rate on the status line.
    pub(super) fn note_generation_delta(&mut self, delta: &str, kind: DeltaKind) {
        self.token_speed.record_delta(delta, kind, Instant::now());
        self.refresh_status_line();
    }

    /// Record the usage the app server reported for a finished step.
    pub(super) fn note_reported_token_usage(
        &mut self,
        output_tokens: i64,
        reasoning_output_tokens: i64,
    ) {
        self.token_speed.note_step_usage(
            u64::try_from(output_tokens).unwrap_or_default(),
            u64::try_from(reasoning_output_tokens).unwrap_or_default(),
        );
    }

    /// Stop the live rate at a generation boundary.
    pub(super) fn end_generation_segment(&mut self) {
        self.token_speed.end_segment();
        self.refresh_status_line();
    }

    /// Mark a tool call, whose arguments are not streamed to the client.
    pub(super) fn note_tool_call_started(&mut self) {
        self.token_speed.note_tool_call();
    }

    /// Finish the turn, returning the average rate for the completion metadata.
    pub(super) fn finish_token_speed_turn(&mut self) -> Option<f64> {
        let average = self.token_speed.finish_turn();
        self.refresh_status_line();
        average
    }

    /// Live rate rendered for the configurable status line.
    ///
    /// Returns `None` whenever no deltas are streaming, so the item disappears
    /// instead of showing a stale number.
    pub(super) fn live_token_speed_label(&self) -> Option<String> {
        let rate = self.token_speed.live_rate(Instant::now())?;
        Some(format!(
            "{} tok/s",
            crate::token_usage::format_token_rate(rate)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(tracker: &mut TokenSpeedTracker, text: &str, at: Instant) -> Option<f64> {
        tracker.record_delta(text, DeltaKind::Content, at)
    }

    #[test]
    fn live_rate_tracks_recent_deltas() {
        let mut tracker = TokenSpeedTracker::default();
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
        let mut tracker = TokenSpeedTracker::default();
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
        let mut tracker = TokenSpeedTracker::default();
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
        let mut tracker = TokenSpeedTracker::default();
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
        // 200 estimated tokens streamed across both items, 400 reported.
        tracker.note_step_usage(/*output_tokens*/ 400, /*reasoning_tokens*/ 0);
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
        let mut tracker = TokenSpeedTracker::default();
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
    fn hidden_reasoning_does_not_calibrate_generation_speed() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        delta(&mut tracker, &"a".repeat(200), start);
        delta(
            &mut tracker,
            &"a".repeat(200),
            start + Duration::from_secs(2),
        );
        tracker.note_step_usage(/*output_tokens*/ 100, /*reasoning_tokens*/ 800);
        let next = start + Duration::from_secs(3);
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
    fn turn_average_excludes_tool_waiting() {
        let mut tracker = TokenSpeedTracker::default();
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
        let mut tracker = TokenSpeedTracker::default();
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
