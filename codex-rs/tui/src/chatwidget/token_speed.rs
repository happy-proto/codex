//! Turn average of the measured generation speed.
//!
//! The app server reports token usage only once a sampling step finishes, so the
//! average pairs each step's reported output with the decode time this client
//! actually observed for that step. Streaming is timed from the first content
//! delta of a step to the last one, which leaves out the wait for the first
//! token, tool execution, and every other pause between steps.
//!
//! Only non-reasoning output is counted: reasoning tokens are reported even when
//! their text never reaches the client, so their decode time cannot be measured
//! here. A step that streamed a single delta has no measured span either, and a
//! step that was only partly observed (resume, replay, or a retry) mixes tokens
//! and time from different attempts; all of them are left out instead of being
//! reported as an approximation.

use std::time::Duration;
use std::time::Instant;

use super::*;

/// Shortest decode time a step may be credited with.
///
/// Streamed replies are timed between deltas, so a step whose content arrived in
/// one burst would otherwise divide by almost zero.
const MIN_ACTIVE_TIME: Duration = Duration::from_millis(250);

/// One uninterrupted run of streamed content deltas.
#[derive(Clone, Copy, Debug, Default)]
struct Segment {
    first_at: Option<Instant>,
    last_at: Option<Instant>,
    deltas: u32,
}

impl Segment {
    /// Decode time covered by the segment.
    ///
    /// A segment with a single delta has no measured gap, so it cannot say how
    /// fast the model was producing tokens.
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
}

/// Measures the generation speed of the turn in progress.
#[derive(Debug, Default)]
pub(super) struct TokenSpeedTracker {
    segment: Segment,
    /// Decode time measured for the step that has not reported usage yet.
    ///
    /// Item boundaries split a step into segments, and a step's reported tokens
    /// cover all of them, so the segments are summed here first.
    step_active: Duration,
    /// Non-reasoning output tokens and decode time attributed to the turn.
    turn_tokens: u64,
    turn_active: Duration,
    /// Cumulative output tokens of the last attributed step.
    ///
    /// Totals only grow, so comparing them separates a finished step from the
    /// repeated usage notifications that rate-limit refreshes carry mid-step.
    last_total_output_tokens: u64,
    /// Cumulative reasoning tokens of the last attributed step.
    last_total_reasoning_tokens: u64,
    /// The step in progress was only partly observed, so it cannot be measured.
    partial_step: bool,
}

impl TokenSpeedTracker {
    /// Drop per-turn state.
    pub(super) fn start_turn(&mut self) {
        self.segment = Segment::default();
        self.step_active = Duration::ZERO;
        self.turn_tokens = 0;
        self.turn_active = Duration::ZERO;
        self.partial_step = false;
    }

    /// Record one streamed content delta.
    pub(super) fn record_content_delta(&mut self, now: Instant) {
        self.segment.first_at.get_or_insert(now);
        self.segment.last_at = Some(now);
        self.segment.deltas = self.segment.deltas.saturating_add(1);
    }

    /// End the current segment, for example when an item finishes streaming.
    pub(super) fn end_segment(&mut self) {
        self.close_segment();
    }

    /// Note that the step in progress is only partly observed.
    ///
    /// Replayed history was never sampled, and a retry restarts the request, so in
    /// both cases the reported usage covers output this client did not time.
    pub(super) fn note_partially_observed_step(&mut self) {
        self.partial_step = true;
    }

    /// Drop everything measured for a step that was abandoned mid-flight.
    pub(super) fn discard_partial_step(&mut self) {
        self.note_partially_observed_step();
        self.segment = Segment::default();
        self.step_active = Duration::ZERO;
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
            self.segment = Segment::default();
            self.step_active = Duration::ZERO;
            self.partial_step = false;
            self.last_total_output_tokens = total_output_tokens;
            self.last_total_reasoning_tokens = total_reasoning_tokens;
            return;
        }
        if total_output_tokens == self.last_total_output_tokens {
            // Rate-limit refreshes re-report the previous step's totals. The step
            // that is streaming now keeps its own measurement.
            return;
        }
        self.close_segment();
        let output_tokens = total_output_tokens - self.last_total_output_tokens;
        let reasoning_tokens = total_reasoning_tokens
            .saturating_sub(self.last_total_reasoning_tokens)
            .min(output_tokens);
        self.last_total_output_tokens = total_output_tokens;
        self.last_total_reasoning_tokens = total_reasoning_tokens;

        let visible_tokens = output_tokens - reasoning_tokens;
        let step_active = std::mem::take(&mut self.step_active);
        let partially_observed = std::mem::take(&mut self.partial_step);
        if !partially_observed && visible_tokens > 0 && step_active >= MIN_ACTIVE_TIME {
            self.turn_tokens = self.turn_tokens.saturating_add(visible_tokens);
            self.turn_active += step_active;
        }
    }

    /// Finish the turn and return its average rate.
    pub(super) fn finish_turn(&mut self) -> Option<f64> {
        self.segment = Segment::default();
        self.step_active = Duration::ZERO;
        if self.turn_tokens == 0 || self.turn_active < MIN_ACTIVE_TIME {
            return None;
        }
        Some(self.turn_tokens as f64 / self.turn_active.as_secs_f64())
    }

    /// Fold the in-flight segment's measured time into the current step.
    fn close_segment(&mut self) {
        let segment = std::mem::take(&mut self.segment);
        if let Some(active) = segment.active_time() {
            self.step_active += active;
        }
    }
}

impl ChatWidget {
    /// Start measuring a new turn.
    pub(super) fn start_token_speed_turn(&mut self) {
        self.token_speed.start_turn();
    }

    /// Record a streamed content delta.
    pub(super) fn note_generation_delta(&mut self) {
        self.token_speed.record_content_delta(Instant::now());
    }

    /// Note that replayed history makes the step in progress only partly observed.
    pub(super) fn note_partially_observed_step(&mut self) {
        self.token_speed.note_partially_observed_step();
    }

    /// Drop a partly streamed step after a retryable stream error.
    pub(super) fn note_retried_step(&mut self) {
        self.token_speed.discard_partial_step();
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

    /// Stop measuring the current segment at an item boundary.
    pub(super) fn end_generation_segment(&mut self) {
        self.token_speed.end_segment();
    }

    /// Finish the turn, returning the average rate for the completion metadata.
    pub(super) fn finish_token_speed_turn(&mut self) -> Option<f64> {
        self.token_speed.finish_turn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(tracker: &mut TokenSpeedTracker, at: Instant) {
        tracker.record_content_delta(at);
    }

    #[test]
    fn average_pairs_reported_output_with_measured_decode_time() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(4));
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn reasoning_tokens_are_excluded_from_the_numerator() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(2));
        // 200 output tokens of which 100 were reasoning.
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 100,
        );

        let average = tracker.finish_turn().expect("turn average");

        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn a_single_delta_step_has_no_measurable_span() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        tracker.note_step_usage(
            /*total_output_tokens*/ 500, /*total_reasoning_tokens*/ 0,
        );

        assert_eq!(tracker.finish_turn(), None);
    }

    #[test]
    fn a_partially_observed_step_is_left_out() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        tracker.note_partially_observed_step();
        tracker.note_step_usage(
            /*total_output_tokens*/ 400, /*total_reasoning_tokens*/ 0,
        );
        let resumed = start + Duration::from_secs(2);
        content(&mut tracker, resumed);
        content(&mut tracker, resumed + Duration::from_secs(2));
        tracker.note_step_usage(
            /*total_output_tokens*/ 500, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn a_retried_step_drops_its_partial_output_and_backoff() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        // The first attempt streams content and completes an item before breaking.
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(1));
        tracker.end_segment();
        tracker.discard_partial_step();
        // After a 20s backoff the retry streams its own content and reports it.
        let retry = start + Duration::from_secs(20);
        content(&mut tracker, retry);
        content(&mut tracker, retry + Duration::from_secs(4));
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        // Counting the abandoned attempt would report 80, and counting the backoff
        // as well would report about 18.
        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn repeated_usage_notifications_keep_the_step_measurement() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(2));
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        // A refresh reports the same totals while the next step is streaming.
        content(&mut tracker, start + Duration::from_secs(4));
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );
        content(&mut tracker, start + Duration::from_secs(6));
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        // Both steps measured 2s and reported 100 tokens.
        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn a_restarted_session_resets_the_reported_baseline() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(2));
        tracker.note_step_usage(
            /*total_output_tokens*/ 5_000, /*total_reasoning_tokens*/ 0,
        );

        // Switching threads restarts the totals; the resumed step must be measured
        // on its own instead of inheriting the previous session's output.
        tracker.start_turn();
        content(&mut tracker, start + Duration::from_secs(4));
        content(&mut tracker, start + Duration::from_secs(6));
        tracker.note_step_usage(
            /*total_output_tokens*/ 100, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        assert!((average - 50.0).abs() < 0.001, "average was {average}");
    }

    #[test]
    fn segments_of_one_step_are_summed() {
        let mut tracker = TokenSpeedTracker::default();
        let start = Instant::now();
        tracker.start_turn();
        // A commentary item streams and completes, then the final answer follows.
        content(&mut tracker, start);
        content(&mut tracker, start + Duration::from_secs(2));
        tracker.end_segment();
        content(&mut tracker, start + Duration::from_secs(6));
        content(&mut tracker, start + Duration::from_secs(10));
        tracker.note_step_usage(
            /*total_output_tokens*/ 200, /*total_reasoning_tokens*/ 0,
        );

        let average = tracker.finish_turn().expect("turn average");

        // 200 tokens over the two measured segments (2s + 4s), not over the whole
        // 10s including the gap between them.
        assert!((average - 33.333).abs() < 0.001, "average was {average}");
    }
}
