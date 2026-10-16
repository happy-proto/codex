//! Client-observed sampling throughput, paired with exact completed-response usage.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Default)]
pub(crate) struct GenerationSpeedState {
    output_tokens: u64,
    request_duration: Duration,
    active_requests: usize,
    response_ids: HashSet<String>,
    invalid: bool,
}

impl GenerationSpeedState {
    pub(crate) fn measurement(state: &Arc<Mutex<Self>>) -> GenerationSpeedMeasurement {
        state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .active_requests += 1;
        GenerationSpeedMeasurement {
            state: Arc::clone(state),
            started_at: Instant::now(),
            completed: false,
            failed_requests: None,
        }
    }

    pub(crate) fn totals(&self) -> Option<(u64, u64)> {
        tracing::info!(
            event.name = "codex.generation_speed.totals",
            invalid = self.invalid,
            active_requests = self.active_requests,
            completed_responses = self.response_ids.len(),
            output_tokens = self.output_tokens,
            request_duration_us = self.request_duration.as_micros() as u64,
            "generation speed totals evaluated"
        );
        if self.invalid || self.active_requests != 0 || self.response_ids.is_empty() {
            return None;
        }
        let duration_us = u64::try_from(self.request_duration.as_micros()).ok()?;
        (self.output_tokens > 0 && duration_us > 0).then_some((self.output_tokens, duration_us))
    }

    pub(crate) fn invalidate(&mut self) {
        self.invalidate_with_reason("daemon_recovery");
    }

    fn invalidate_with_reason(&mut self, reason: &'static str) {
        tracing::info!(
            event.name = "codex.generation_speed.invalidated",
            reason,
            already_invalid = self.invalid,
            "generation speed invalidated"
        );
        self.invalid = true;
    }

    fn skip_request(&mut self, reason: &'static str) {
        self.active_requests -= 1;
        tracing::info!(
            event.name = "codex.generation_speed.request_skipped",
            reason,
            "generation speed request excluded"
        );
    }

    fn record(&mut self, response_id: &str, output_tokens: Option<i64>, duration: Duration) {
        let Some(tokens) = output_tokens else {
            self.skip_request("missing_usage");
            return;
        };
        self.active_requests -= 1;
        let Ok(output_tokens) = u64::try_from(tokens) else {
            self.invalidate_with_reason("negative_output_tokens");
            return;
        };
        if response_id.is_empty() {
            self.invalidate_with_reason("empty_response_id");
            return;
        }
        if !self.response_ids.insert(response_id.to_string()) {
            self.invalidate_with_reason("duplicate_response_id");
            return;
        }
        if duration.is_zero() {
            self.invalidate_with_reason("zero_request_duration");
            return;
        }
        match (
            self.output_tokens.checked_add(output_tokens),
            self.request_duration.checked_add(duration),
        ) {
            (Some(tokens), Some(elapsed)) => {
                self.output_tokens = tokens;
                self.request_duration = elapsed;
            }
            _ => self.invalidate_with_reason("totals_overflow"),
        }
    }
}

/// Incomplete requests are excluded without discarding other completed measurements.
pub(crate) struct GenerationSpeedMeasurement {
    state: Arc<Mutex<GenerationSpeedState>>,
    started_at: Instant,
    completed: bool,
    failed_requests: Option<(Arc<AtomicU64>, u64)>,
}

impl GenerationSpeedMeasurement {
    pub(crate) fn observe_failures(mut self, counter: Arc<AtomicU64>) -> Self {
        let baseline = counter.load(Ordering::Relaxed);
        self.failed_requests = Some((counter, baseline));
        self
    }

    pub(crate) fn complete(mut self, response_id: &str, output_tokens: Option<i64>) {
        let elapsed = self.started_at.elapsed();
        if self
            .failed_requests
            .as_ref()
            .is_some_and(|(counter, baseline)| counter.load(Ordering::Relaxed) != *baseline)
        {
            self.state
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .skip_request("client_failure_counter_changed");
            self.completed = true;
            return;
        }
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .record(response_id, output_tokens, elapsed);
        self.completed = true;
    }
}

impl Drop for GenerationSpeedMeasurement {
    fn drop(&mut self) {
        if !self.completed {
            let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
            state.skip_request("request_dropped_before_completion");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_token_and_request_time_pairs_without_tool_waits() {
        let mut state = GenerationSpeedState::default();
        state.active_requests = 2;
        state.record("first", Some(100), Duration::from_secs(1));
        assert_eq!(state.totals(), None);
        // A long tool/user wait between these responses never enters either measurement.
        state.record("second", Some(100), Duration::from_secs(3));
        assert_eq!(state.totals(), Some((200, 4_000_000)));
    }

    #[test]
    fn negative_duplicate_and_zero_time_usage_invalidate_the_turn() {
        for (id, tokens, duration) in [
            ("second", Some(-1), Duration::from_secs(1)),
            ("first", Some(10), Duration::from_secs(1)),
            ("", Some(10), Duration::from_secs(1)),
            ("second", Some(10), Duration::ZERO),
        ] {
            let mut state = GenerationSpeedState::default();
            state.active_requests = 2;
            state.record("first", Some(100), Duration::from_secs(1));
            state.record(id, tokens, duration);
            assert_eq!(state.totals(), None);
        }
    }

    #[test]
    fn missing_usage_discards_both_tokens_and_time_without_losing_valid_samples() {
        for missing_index in 0..3 {
            let mut state = GenerationSpeedState::default();
            state.active_requests = 3;
            let mut valid = [("first", 100, 1), ("second", 200, 3)].into_iter();
            for index in 0..3 {
                if index == missing_index {
                    state.record("missing", None, Duration::from_secs(60));
                } else {
                    let (id, tokens, seconds) = valid.next().unwrap();
                    state.record(id, Some(tokens), Duration::from_secs(seconds));
                }
            }
            assert_eq!(state.totals(), Some((300, 4_000_000)));
        }
    }

    #[test]
    fn dropped_request_preserves_samples_before_and_after_it() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        {
            let mut state = state.lock().unwrap();
            state.active_requests = 1;
            state.record("first", Some(100), Duration::from_secs(1));
        }
        drop(GenerationSpeedState::measurement(&state));
        let mut state = state.lock().unwrap();
        assert_eq!(state.totals(), Some((100, 1_000_000)));
        state.active_requests = 1;
        state.record("follow-up", Some(200), Duration::from_secs(3));
        assert_eq!(state.totals(), Some((300, 4_000_000)));
    }

    #[test]
    fn only_incomplete_requests_produce_no_speed() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        drop(GenerationSpeedState::measurement(&state));
        GenerationSpeedState::measurement(&state).complete("missing", None);
        let state = state.lock().unwrap();
        assert_eq!(state.active_requests, 0);
        assert_eq!(state.totals(), None);
    }

    #[test]
    fn empty_and_zero_output_turns_have_no_speed() {
        let mut state = GenerationSpeedState::default();
        assert_eq!(state.totals(), None);
        state.active_requests = 1;
        state.record("empty", Some(0), Duration::from_secs(1));
        assert_eq!(state.totals(), None);
    }

    #[test]
    fn an_internal_failed_attempt_discards_only_its_measurement() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        let counter = Arc::new(AtomicU64::new(0));
        let measurement =
            GenerationSpeedState::measurement(&state).observe_failures(Arc::clone(&counter));
        counter.fetch_add(1, Ordering::Relaxed);
        measurement.complete("retried", Some(100));
        let mut state = state.lock().unwrap();
        assert_eq!(state.active_requests, 0);
        assert_eq!(state.totals(), None);
        state.active_requests = 1;
        state.record("follow-up", Some(200), Duration::from_secs(4));
        assert_eq!(state.totals(), Some((200, 4_000_000)));
    }

    #[test]
    fn recovered_turn_does_not_publish_only_its_new_requests() {
        let mut state = GenerationSpeedState::default();
        state.invalidate();
        state.active_requests = 1;
        state.record("after-recovery", Some(100), Duration::from_secs(1));
        assert_eq!(state.totals(), None);
    }
}
