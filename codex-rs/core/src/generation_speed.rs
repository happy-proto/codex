//! Client-observed sampling throughput, paired with exact completed-response usage.

use std::collections::HashSet;
use std::collections::VecDeque;
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
    recent_disabled: bool,
    recent: RecentGenerationSpeed,
}

const RECENT_WINDOW: Duration = Duration::from_secs(5 * 60);

/// A clock that advances only while ordinary model requests are in flight.
#[derive(Debug, Default)]
struct RecentGenerationSpeed {
    elapsed: Duration,
    active_since: Option<Instant>,
    active_requests: usize,
    samples: VecDeque<GenerationSpeedSample>,
}

#[derive(Debug)]
struct GenerationSpeedSample {
    ended_at: Duration,
    output_tokens: u64,
    request_duration: Duration,
}

impl RecentGenerationSpeed {
    fn elapsed_at(&self, now: Instant) -> Duration {
        self.elapsed
            + self
                .active_since
                .map(|start| now.saturating_duration_since(start))
                .unwrap_or_default()
    }

    fn begin(&mut self, now: Instant) {
        if self.active_requests == 0 {
            self.active_since = Some(now);
        }
        self.active_requests += 1;
    }

    fn finish(&mut self, now: Instant, sample: Option<(u64, Duration)>) {
        let ended_at = self.elapsed_at(now);
        self.active_requests -= 1;
        if self.active_requests == 0 {
            self.elapsed = ended_at;
            self.active_since = None;
        }
        if let Some((output_tokens, request_duration)) = sample {
            self.samples.push_back(GenerationSpeedSample {
                ended_at,
                output_tokens,
                request_duration,
            });
        }
        self.prune(ended_at);
    }

    fn prune(&mut self, elapsed: Duration) {
        while self
            .samples
            .front()
            .is_some_and(|sample| elapsed.saturating_sub(sample.ended_at) > RECENT_WINDOW)
        {
            self.samples.pop_front();
        }
    }

    fn speed(&mut self, now: Instant) -> Option<f64> {
        self.prune(self.elapsed_at(now));
        let (tokens, duration) = self.samples.iter().try_fold(
            (0_u64, Duration::ZERO),
            |(tokens, duration), sample| {
                Some((
                    tokens.checked_add(sample.output_tokens)?,
                    duration.checked_add(sample.request_duration)?,
                ))
            },
        )?;
        (!duration.is_zero()).then(|| tokens as f64 / duration.as_secs_f64())
    }
}

impl GenerationSpeedState {
    pub(crate) fn measurement(state: &Arc<Mutex<Self>>) -> GenerationSpeedMeasurement {
        let started_at = Instant::now();
        {
            let mut state = state.lock().unwrap_or_else(|err| err.into_inner());
            state.active_requests += 1;
            state.recent.begin(started_at);
        }
        GenerationSpeedMeasurement {
            state: Arc::clone(state),
            started_at,
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
        self.recent_disabled = true;
        self.invalidate_with_reason("daemon_recovery");
    }

    fn recent_speed(&mut self, now: Instant) -> Option<f64> {
        if self.recent_disabled {
            None
        } else {
            self.recent.speed(now)
        }
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

    fn record(
        &mut self,
        response_id: &str,
        output_tokens: Option<i64>,
        duration: Duration,
    ) -> bool {
        let Some(tokens) = output_tokens else {
            self.skip_request("missing_usage");
            return false;
        };
        self.active_requests -= 1;
        let Ok(output_tokens) = u64::try_from(tokens) else {
            self.invalidate_with_reason("negative_output_tokens");
            return false;
        };
        if response_id.is_empty() {
            self.invalidate_with_reason("empty_response_id");
            return false;
        }
        if !self.response_ids.insert(response_id.to_string()) {
            self.invalidate_with_reason("duplicate_response_id");
            return false;
        }
        if duration.is_zero() {
            self.invalidate_with_reason("zero_request_duration");
            return false;
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
        // Turn-level invalidity does not discard an otherwise valid recent sample.
        true
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
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.started_at);
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if self
            .failed_requests
            .as_ref()
            .is_some_and(|(counter, baseline)| counter.load(Ordering::Relaxed) != *baseline)
        {
            state.skip_request("client_failure_counter_changed");
            state.recent.finish(now, None);
            self.completed = true;
            return;
        }
        let valid = state.record(response_id, output_tokens, elapsed);
        let sample = output_tokens
            .filter(|_| valid)
            .map(|tokens| (tokens as u64, elapsed));
        state.recent.finish(now, sample);
        self.completed = true;
    }
}

impl Drop for GenerationSpeedMeasurement {
    fn drop(&mut self) {
        if !self.completed {
            let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
            state.skip_request("request_dropped_before_completion");
            state.recent.finish(Instant::now(), None);
        }
    }
}

/// Private local side channel; the upstream app-server protocol stays unchanged.
pub(crate) struct GenerationSpeedReporter(tokio::task::JoinHandle<()>);

impl GenerationSpeedReporter {
    pub(crate) async fn start(
        state: &Arc<Mutex<GenerationSpeedState>>,
        state_db: crate::StateDbHandle,
        thread_id: codex_protocol::ThreadId,
        turn_id: String,
    ) -> Self {
        let run_id = uuid::Uuid::new_v4().to_string();
        // Reset before publishing TurnStarted, so replay cannot reuse a pre-crash value.
        let initialized = matches!(
            tokio::time::timeout(
                Duration::from_millis(100),
                state_db.start_live_generation_speed(thread_id, &turn_id, &run_id),
            )
            .await,
            Ok(Ok(()))
        );
        let state = Arc::downgrade(state);
        Self(tokio::spawn(async move {
            let mut initialized = initialized;
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if !initialized {
                    // A new thread's metadata may not exist until its first input is persisted.
                    initialized = matches!(
                        tokio::time::timeout(
                            Duration::from_millis(100),
                            state_db.start_live_generation_speed(thread_id, &turn_id, &run_id),
                        )
                        .await,
                        Ok(Ok(()))
                    );
                    if !initialized {
                        continue;
                    }
                }
                let Some(state) = state.upgrade() else {
                    break;
                };
                let speed = {
                    let mut state = state.lock().unwrap_or_else(|err| err.into_inner());
                    state.recent_speed(Instant::now())
                };
                // Never hold the measurement lock across I/O or delay model sampling.
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    state_db.update_live_generation_speed(thread_id, &turn_id, &run_id, speed),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => tracing::debug!(%err, "live generation speed write failed"),
                    Err(err) => tracing::debug!(%err, "live generation speed write timed out"),
                }
            }
        }))
    }
}

impl Drop for GenerationSpeedReporter {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_generation_speed_weights_completed_calls_and_freezes_during_tools() {
        let start = Instant::now();
        let mut recent = RecentGenerationSpeed::default();
        recent.begin(start);
        recent.finish(
            start + Duration::from_secs(10),
            Some((100, Duration::from_secs(10))),
        );
        let after_tools = start + Duration::from_secs(1810);
        assert_eq!(recent.speed(after_tools), Some(10.0));
        recent.begin(after_tools);
        // An unfinished request advances the window, but contributes neither tokens nor duration.
        assert_eq!(
            recent.speed(after_tools + Duration::from_secs(30)),
            Some(10.0)
        );
        recent.finish(
            after_tools + Duration::from_secs(30),
            Some((900, Duration::from_secs(30))),
        );
        assert_eq!(
            recent.speed(after_tools + Duration::from_secs(30)),
            Some(25.0)
        );
    }

    #[test]
    fn recent_generation_speed_ages_out_by_effective_end_time_and_keeps_long_calls_whole() {
        let start = Instant::now();
        let mut recent = RecentGenerationSpeed::default();
        recent.begin(start);
        recent.finish(
            start + Duration::from_secs(10),
            Some((100, Duration::from_secs(10))),
        );
        recent.begin(start + Duration::from_secs(1000));
        assert_eq!(recent.speed(start + Duration::from_secs(1300)), Some(10.0));
        assert_eq!(recent.speed(start + Duration::from_secs(1301)), None);
        recent.finish(
            start + Duration::from_secs(1600),
            Some((12000, Duration::from_secs(600))),
        );
        assert_eq!(recent.speed(start + Duration::from_secs(1600)), Some(20.0));
        // Tool execution after the long call does not age the sample.
        assert_eq!(recent.speed(start + Duration::from_secs(10000)), Some(20.0));
    }

    #[test]
    fn recent_generation_speed_expires_individual_samples_and_counts_unmeasurable_waits() {
        let start = Instant::now();
        let mut recent = RecentGenerationSpeed::default();
        recent.begin(start);
        recent.finish(
            start + Duration::from_secs(10),
            Some((100, Duration::from_secs(10))),
        );
        recent.begin(start + Duration::from_secs(10));
        recent.finish(
            start + Duration::from_secs(110),
            Some((3000, Duration::from_secs(100))),
        );
        recent.begin(start + Duration::from_secs(110));
        recent.finish(start + Duration::from_secs(311), None);
        assert_eq!(recent.speed(start + Duration::from_secs(311)), Some(30.0));
        recent.begin(start + Duration::from_secs(311));
        recent.finish(start + Duration::from_secs(411), None);
        assert_eq!(recent.speed(start + Duration::from_secs(411)), None);
    }

    #[test]
    fn recent_generation_speed_includes_zero_output_and_counts_overlapping_time_once() {
        let start = Instant::now();
        let mut recent = RecentGenerationSpeed::default();
        recent.begin(start);
        recent.begin(start + Duration::from_secs(5));
        recent.finish(
            start + Duration::from_secs(10),
            Some((100, Duration::from_secs(10))),
        );
        recent.finish(
            start + Duration::from_secs(15),
            Some((0, Duration::from_secs(10))),
        );
        assert_eq!(recent.elapsed, Duration::from_secs(15));
        assert_eq!(recent.speed(start + Duration::from_secs(1000)), Some(5.0));
        let mut empty = RecentGenerationSpeed::default();
        assert_eq!(empty.speed(start), None);
        empty.begin(start);
        empty.finish(
            start + Duration::from_secs(1),
            Some((0, Duration::from_secs(1))),
        );
        assert_eq!(empty.speed(start + Duration::from_secs(1)), Some(0.0));
    }

    #[test]
    fn recent_generation_speed_measurement_uses_terminal_usage_and_excludes_failed_calls() {
        let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
        let measurement = GenerationSpeedState::measurement(&state);
        assert!(state.lock().unwrap().recent.samples.is_empty());
        measurement.complete("valid", Some(200));
        assert_eq!(state.lock().unwrap().recent.samples.len(), 1);
        GenerationSpeedState::measurement(&state).complete("missing", None);
        drop(GenerationSpeedState::measurement(&state));
        let counter = Arc::new(AtomicU64::new(0));
        let measurement =
            GenerationSpeedState::measurement(&state).observe_failures(Arc::clone(&counter));
        counter.fetch_add(1, Ordering::Relaxed);
        measurement.complete("retried", Some(500));
        let state = state.lock().unwrap();
        assert_eq!(state.recent.samples.len(), 1);
        assert_eq!(state.recent.samples[0].output_tokens, 200);
        assert_eq!(state.recent.active_requests, 0);
        assert!(state.recent.active_since.is_none());
    }

    #[test]
    fn recent_generation_speed_preserves_valid_samples_around_invalid_calls() {
        for (response_id, tokens) in [("bad", Some(-1)), ("", Some(100)), ("first", Some(100))] {
            let state = Arc::new(Mutex::new(GenerationSpeedState::default()));
            GenerationSpeedState::measurement(&state).complete("first", Some(100));
            GenerationSpeedState::measurement(&state).complete(response_id, tokens);
            GenerationSpeedState::measurement(&state).complete("last", Some(300));
            let mut state = state.lock().unwrap();
            assert_eq!(state.totals(), None);
            assert_eq!(state.recent.samples.len(), 2);
            assert_eq!(state.recent.samples[0].output_tokens, 100);
            assert_eq!(state.recent.samples[1].output_tokens, 300);
            assert!(state.recent_speed(Instant::now()).is_some());
            state.invalidate();
            assert_eq!(state.recent_speed(Instant::now()), None);
        }
    }

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
