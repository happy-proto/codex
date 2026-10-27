use super::ResponsesStreamRequest;
use super::ResponsesStreamRetryState;
use super::handle_response_stream_error;
use super::log_retry;
use crate::realtime_history::RealtimeHistoryState;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::session::tests::make_session_and_context_with_rx;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::EventMsg;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing_test::internal::MockWriter;

#[test_case::test_case(ResponsesStreamRequest::Sampling; "sampling")]
#[test_case::test_case(ResponsesStreamRequest::RemoteCompactionV2; "compaction")]
#[tokio::test]
async fn websocket_message_too_large_does_not_retry_or_fallback(request: ResponsesStreamRequest) {
    for max_retries in [0, 50] {
        let (session, turn_context, events) = make_session_and_context_with_rx().await;
        let step_context = StepContext::for_test(turn_context);
        let mut client_session = session.services.model_client.new_session();
        let mut retry_state = ResponsesStreamRetryState::default();
        let error = codex_api::map_api_error(codex_api::ApiError::WebSocketClosed {
            code: 1009,
            reason: String::new(),
        });
        let expected_message = error.to_string();
        let error = handle_response_stream_error(
            &mut retry_state,
            max_retries,
            error,
            &mut client_session,
            &session,
            &step_context,
            request,
        )
        .await
        .expect_err("message too large is terminal");
        assert_eq!(error.to_string(), expected_message);
        assert_eq!(retry_state.retries, 0);
        assert_eq!(retry_state.connection_retries, 0);
        while let Ok(event) = events.try_recv() {
            assert!(!matches!(
                event.msg,
                EventMsg::StreamError(_) | EventMsg::Warning(_)
            ));
        }
        // Fallback is still available: the terminal error did not activate it.
        assert!(client_session.try_switch_fallback_transport(
            &step_context.turn.session_telemetry,
            step_context.turn.model_info(),
        ));
    }
}

/// 缺少建议的容量错误仍受原预算约束，首次等待与传输回退都必须显示倒计时。
#[test_case::test_case(ResponsesStreamRequest::Sampling; "sampling")]
#[test_case::test_case(ResponsesStreamRequest::RemoteCompactionV2; "compaction")]
#[tokio::test]
async fn overload_without_advice_waits_one_minute(request: ResponsesStreamRequest) {
    for max_retries in [1, 0] {
        let (session, turn_context, events) = make_session_and_context_with_rx().await;
        let step_context = StepContext::for_test(turn_context);
        let mut client_session = session.services.model_client.new_session();
        let mut retry_state = ResponsesStreamRetryState::default();
        tokio::time::pause();
        let retry = handle_response_stream_error(
            &mut retry_state,
            max_retries,
            CodexErr::ServerOverloaded,
            &mut client_session,
            &session,
            &step_context,
            request,
        );
        tokio::pin!(retry);
        assert!(futures::poll!(&mut retry).is_pending());
        let mut messages = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event.msg {
                EventMsg::StreamError(error) => messages.push(error.message),
                EventMsg::Warning(warning) => messages.push(warning.message),
                _ => {}
            }
        }
        assert!(
            messages
                .iter()
                .any(|message| message.contains("retrying in 60s")),
            "missing overload wait notification: {messages:?}"
        );
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(futures::poll!(&mut retry).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        retry.await.expect("overload should retry after one minute");
        tokio::time::resume();
    }
}

/// 默认等待不能绕过已耗尽的重试预算或重复启用传输回退。
#[tokio::test]
async fn overload_default_wait_does_not_extend_retry_budget() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    client_session.try_switch_fallback_transport(
        &step_context.turn.session_telemetry,
        step_context.turn.model_info(),
    );
    let mut retry_state = ResponsesStreamRetryState {
        retries: 1,
        ..Default::default()
    };
    tokio::time::pause();
    let started = Instant::now();
    let error = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 1,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect_err("exhausted overload must remain terminal");
    assert_eq!(error.to_string(), CodexErr::ServerOverloaded.to_string());
    assert_eq!(retry_state.retries, 1);
    assert_eq!(Instant::now(), started);
}

/// 包括零等待在内的有效建议必须保留原期限，不能被默认一分钟替换。
#[test_case::test_case(0; "zero")]
#[test_case::test_case(10; "short")]
#[test_case::test_case(120; "long")]
#[tokio::test]
async fn overload_preserves_server_retry_deadline(seconds: u64) {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut retry_state = ResponsesStreamRetryState::default();
    tokio::time::pause();
    let advice = RetryAfter::from_delay(Duration::from_secs(seconds)).expect("retry deadline");
    let elapsed = seconds.min(4);
    tokio::time::advance(Duration::from_secs(elapsed)).await;
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 1,
        CodexErr::ServerOverloaded.with_retry_after(advice),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    if seconds > elapsed {
        assert!(futures::poll!(&mut retry).is_pending());
        tokio::time::advance(Duration::from_secs(seconds - elapsed - 1)).await;
        assert!(futures::poll!(&mut retry).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    retry.await.expect("advised overload should retry");
    assert!(
        (advice.deadline()..=advice.deadline() + Duration::from_millis(1))
            .contains(&Instant::now())
    );
}

/// 第五十次重试的本地指数退避不能超过一分钟。
#[tokio::test]
async fn local_retry_backoff_is_capped_at_one_minute() {
    let (session, turn_context, events) = make_session_and_context_with_rx().await;
    let step_context = StepContext::for_test(turn_context);
    let mut client_session = session.services.model_client.new_session();
    let mut retry_state = ResponsesStreamRetryState {
        retries: 49,
        ..Default::default()
    };
    tokio::time::pause();
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 50,
        CodexErr::Stream("connection reset".into()),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    let mut messages = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let EventMsg::StreamError(error) = event.msg {
            messages.push(error.message);
        }
    }
    assert_eq!(messages, vec!["Reconnecting... 50/50 (retrying in 60s)"]);
    tokio::time::advance(Duration::from_secs(59)).await;
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    retry
        .await
        .expect("local retry should resume after one minute");
}

/// 即使没有服务端建议，首次和后续本地退避都报告等待及原始错误。
#[tokio::test]
async fn every_local_retry_reports_wait_and_reason() {
    let (session, turn_context, events) = make_session_and_context_with_rx().await;
    let step_context = StepContext::for_test(turn_context);
    let mut client_session = session.services.model_client.new_session();
    let mut retry_state = ResponsesStreamRetryState::default();
    tokio::time::pause();
    for attempt in 1..=2 {
        let retry = handle_response_stream_error(
            &mut retry_state,
            /*max_retries*/ 2,
            CodexErr::Stream("connection reset".into()),
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        );
        tokio::pin!(retry);
        assert!(futures::poll!(&mut retry).is_pending());
        let mut messages = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let EventMsg::StreamError(error) = event.msg {
                messages.push(error);
            }
        }
        let [error] = messages.as_slice() else {
            panic!("expected exactly one notification on attempt {attempt}: {messages:?}");
        };
        assert_eq!(
            error.message,
            format!("Reconnecting... {attempt}/2 (retrying in 1s)")
        );
        assert_eq!(
            error.additional_details.as_deref(),
            Some("stream disconnected before completion: connection reset")
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        retry.await.expect("local retry should remain allowed");
    }
}

/// 服务端指定的首次重试和传输回退等待必须先通知客户端，再遵守原期限。
#[tokio::test]
async fn advised_retry_reports_wait_before_sleeping() {
    for max_retries in [1, 0] {
        let (session, turn_context, events) = make_session_and_context_with_rx().await;
        let step_context = StepContext::for_test(turn_context);
        let mut client_session = session.services.model_client.new_session();
        let mut retry_state = ResponsesStreamRetryState::default();
        tokio::time::pause();
        let advice = RetryAfter::from_delay(Duration::from_secs(10)).expect("retry deadline");
        let retry = handle_response_stream_error(
            &mut retry_state,
            max_retries,
            CodexErr::InternalServerError.with_retry_after(advice),
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        );
        tokio::pin!(retry);
        assert!(futures::poll!(&mut retry).is_pending());
        let mut messages = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event.msg {
                EventMsg::StreamError(error) => messages.push(error.message),
                EventMsg::Warning(warning) => messages.push(warning.message),
                _ => {}
            }
        }
        assert!(
            messages
                .iter()
                .any(|message| message.contains("retrying in 10s")),
            "missing wait notification before sleep: {messages:?}"
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        assert!(futures::poll!(&mut retry).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        retry.await.expect("advised retry should remain allowed");
        tokio::time::resume();
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
    // Use a later retry to verify that reporting time does not restart the deadline.
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
