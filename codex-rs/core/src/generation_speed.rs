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
        if self.invalid || self.active_requests != 0 || self.response_ids.is_empty() {
            return None;
        }
        let duration_us = u64::try_from(self.request_duration.as_micros()).ok()?;
        (self.output_tokens > 0 && duration_us > 0).then_some((self.output_tokens, duration_us))
    }

    pub(crate) fn invalidate(&mut self) {
        self.invalid = true;
    }

    fn record(&mut self, response_id: &str, output_tokens: Option<i64>, duration: Duration) {
        self.active_requests -= 1;
        let Some(output_tokens) = output_tokens.and_then(|tokens| u64::try_from(tokens).ok())
        else {
            self.invalid = true;
            return;
        };
        if response_id.is_empty()
            || !self.response_ids.insert(response_id.to_string())
            || duration.is_zero()
        {
            self.invalid = true;
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
            _ => self.invalid = true,
        }
    }
}

/// Dropping an incomplete request invalidates the whole turn, including retry attempts.
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
        let output_tokens = if self
            .failed_requests
            .as_ref()
            .is_some_and(|(counter, baseline)| counter.load(Ordering::Relaxed) != *baseline)
        {
            None
        } else {
            output_tokens
        };
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
            state.active_requests -= 1;
            state.invalid = true;
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
    fn missing_negative_duplicate_and_zero_time_usage_invalidate_the_turn() {
        for (id, tokens, duration) in [
            ("second", None, Duration::from_secs(1)),
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
    fn incomplete_attempt_remains_invalid_after_a_successful_retry() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        drop(GenerationSpeedState::measurement(&state));
        GenerationSpeedState::measurement(&state).complete("retry", Some(100));
        assert_eq!(state.lock().unwrap().totals(), None);
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
    fn an_internal_failed_attempt_invalidates_a_completed_response() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        let counter = Arc::new(AtomicU64::new(0));
        let measurement =
            GenerationSpeedState::measurement(&state).observe_failures(Arc::clone(&counter));
        counter.fetch_add(1, Ordering::Relaxed);
        measurement.complete("retried", Some(100));
        assert_eq!(state.lock().unwrap().totals(), None);
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
