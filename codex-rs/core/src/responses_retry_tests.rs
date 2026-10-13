use super::ResponsesStreamRequest;
use super::ResponsesStreamRetryState;
use super::handle_response_stream_error;
use super::log_retry;
use crate::realtime_history::RealtimeHistoryState;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing_test::internal::MockWriter;

/// Capacity retries outlive a zero stream budget and settle at five minutes.
#[tokio::test(start_paused = true)]
async fn capacity_retries_are_unbounded_and_capped() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    for request in [
        ResponsesStreamRequest::Sampling,
        ResponsesStreamRequest::RemoteCompactionV2,
    ] {
        let mut retry_state = ResponsesStreamRetryState::default();
        for seconds in [1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300, 300] {
            let started = Instant::now();
            handle_response_stream_error(
                &mut retry_state,
                /*max_retries*/ 0,
                CodexErr::ServerOverloaded,
                &mut client_session,
                &session,
                &step_context,
                request,
            )
            .await
            .expect("capacity retries must continue without server advice");
            let elapsed = started.elapsed();
            assert!(
                (Duration::from_secs(seconds)
                    ..=Duration::from_secs(seconds) + Duration::from_millis(1))
                    .contains(&elapsed),
                "unexpected retry delay {elapsed:?}"
            );
        }
        assert_eq!(retry_state.retries, 0);
        // Capacity failures must not grant ordinary errors any extra retries.
        // Exhaust the separate WebSocket transport fallback before checking the budget.
        client_session.try_switch_fallback_transport(
            &step_context.turn.session_telemetry,
            step_context.turn.model_info(),
        );
        assert!(
            handle_response_stream_error(
                &mut retry_state,
                /*max_retries*/ 0,
                CodexErr::InternalServerError,
                &mut client_session,
                &session,
                &step_context,
                request,
            )
            .await
            .is_err()
        );
    }
}

/// Advice cannot cause a tight retry loop or exceed the user's five-minute cap.
#[tokio::test(start_paused = true)]
async fn capacity_retry_advice_obeys_backoff_and_cap() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    for (advice_seconds, expected_seconds) in [(0, 1), (17, 17), (600, 300)] {
        let mut retry_state = ResponsesStreamRetryState::default();
        let started = Instant::now();
        let advice =
            RetryAfter::from_delay(Duration::from_secs(advice_seconds)).expect("retry deadline");
        handle_response_stream_error(
            &mut retry_state,
            /*max_retries*/ 0,
            CodexErr::ServerOverloaded.with_retry_after(advice),
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await
        .expect("capacity should retry with advice");
        assert!(
            (Duration::from_secs(expected_seconds)
                ..=Duration::from_secs(expected_seconds) + Duration::from_millis(1))
                .contains(&started.elapsed())
        );
    }
}

#[tokio::test(start_paused = true)]
async fn capacity_retry_wait_can_be_cancelled() {
    use codex_async_utils::OrCancelExt;
    use tokio_util::sync::CancellationToken;

    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut retry_state = ResponsesStreamRetryState {
        capacity_retry_delay: Duration::from_secs(300),
        ..Default::default()
    };
    let cancel = CancellationToken::new();
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 0,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .or_cancel(&cancel);
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    cancel.cancel();
    assert!(
        retry.await.is_err(),
        "cancellation must interrupt the sleep"
    );
}

/// Excluding Guardian must not disable capacity retries for ordinary subagents.
#[tokio::test(start_paused = true)]
async fn ordinary_subagent_capacity_failure_retries_without_stream_budget() {
    use codex_protocol::protocol::SessionSource;
    use codex_protocol::protocol::SubAgentSource;

    for source in [
        SubAgentSource::Review,
        SubAgentSource::Other("worker".to_string()),
    ] {
        let (session, mut turn_context) = make_session_and_context().await;
        turn_context.session_source = SessionSource::SubAgent(source);
        let step_context = StepContext::for_test(Arc::new(turn_context));
        let mut client_session = session.services.model_client.new_session();
        let mut retry_state = ResponsesStreamRetryState::default();
        handle_response_stream_error(
            &mut retry_state,
            /*max_retries*/ 0,
            CodexErr::ServerOverloaded,
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await
        .expect("ordinary subagents retain unbounded capacity retries");
        assert_eq!(retry_state.capacity_retries, 1);
        assert_eq!(retry_state.retries, 0);
    }
}

/// Both Guardian source representations retain the approval retry policy.
#[tokio::test(start_paused = true)]
async fn internal_capacity_failure_is_not_retried_indefinitely() {
    use codex_protocol::protocol::InternalSessionSource;
    use codex_protocol::protocol::SessionSource;
    use codex_protocol::protocol::SubAgentSource;

    for source in [
        SessionSource::Internal(InternalSessionSource::Guardian),
        SessionSource::Internal(InternalSessionSource::MemoryConsolidation),
        SessionSource::SubAgent(SubAgentSource::Other(
            crate::guardian::GUARDIAN_REVIEWER_NAME.to_string(),
        )),
    ] {
        let (session, mut turn_context) = make_session_and_context().await;
        turn_context.session_source = source;
        let step_context = StepContext::for_test(Arc::new(turn_context));
        let mut client_session = session.services.model_client.new_session();
        let mut retry_state = ResponsesStreamRetryState::default();
        let result = handle_response_stream_error(
            &mut retry_state,
            /*max_retries*/ 0,
            CodexErr::ServerOverloaded,
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await;
        assert!(
            matches!(result, Err(err) if matches!(err.details(), codex_protocol::error::CodexErrorDetails::ServerOverloaded))
        );
        assert_eq!(retry_state.capacity_retries, 0);
    }
}

#[tokio::test]
async fn sampling_retry_logs_stream_error_context() {
    let (_session, turn_context) = make_session_and_context().await;
    let buffer: &'static std::sync::Mutex<Vec<u8>> =
        Box::leak(Box::new(std::sync::Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(MockWriter::new(buffer))
        .finish();
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

    log_retry(
        ResponsesStreamRequest::Sampling,
        &turn_context,
        &CodexErr::Stream("websocket closed by server before response.completed".to_string()),
        /*retries*/ 2,
        /*max_retries*/ 5,
        Duration::from_secs(1),
    );

    let logs = String::from_utf8(
        buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    )
    .expect("retry log should be valid utf-8");
    assert!(logs.contains("stream disconnected - retrying sampling request"));
    assert!(logs.contains(&format!("turn_id={}", turn_context.sub_id)));
    assert!(logs.contains("retries=2"));
    assert!(logs.contains("max_retries=5"));
    assert!(logs.contains(
        "sampling_error=stream disconnected before completion: websocket closed by server before response.completed"
    ));
}

/// Time spent reporting a retry must count toward the server's original deadline.
#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "test holds the event-delivery lock to delay notification while virtual time advances"
)]
async fn stream_retry_preserves_deadline_across_delayed_notification() {
    let (mut session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    session.realtime_history = Some(Mutex::new(RealtimeHistoryState::default()));
    let mut client_session = session.services.model_client.new_session();
    // The second retry emits a notification even when release builds hide the first.
    let mut retry_state = ResponsesStreamRetryState {
        retries: 1,
        ..Default::default()
    };

    tokio::time::pause();
    let advice = RetryAfter::from_delay(Duration::from_secs(10)).expect("retry deadline");
    tokio::time::advance(Duration::from_secs(4)).await;

    // Hold the event-delivery lock so reporting the error consumes part of the deadline.
    let history_guard = session.realtime_history.as_ref().unwrap().lock().await;
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 2,
        CodexErr::InternalServerError.with_retry_after(advice),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(4)).await;
    drop(history_guard);

    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(futures::poll!(&mut retry).is_pending());
    retry.await.expect("retry should be allowed");
    // Tokio rounds timer deadlines up to the next millisecond.
    let resumed_at = Instant::now();
    assert!(
        (advice.deadline()..=advice.deadline() + Duration::from_millis(1)).contains(&resumed_at),
        "retry resumed at {resumed_at:?}, expected {advice:?}"
    );
}
